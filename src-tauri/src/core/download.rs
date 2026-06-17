//! Concurrent, hash-verified download engine.
//!
//! Executes a [`DownloadPlan`] (a list of [`DownloadItem`]s), verifies each
//! file's hash on completion, and streams progress through a [`ProgressSink`].
//! No Minecraft-specific logic lives here — the engine is resolver-agnostic.

use std::io::Read;
use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
// `sha1::Digest` and `sha2::Digest` are re-exports of the same `digest::Digest`
// trait; import once via sha1 and it covers both hasher types.
use sha1::Digest;

// ---------------------------------------------------------------------------
// Hash discriminant
// ---------------------------------------------------------------------------

/// The expected hash for a single download item.
///
/// The engine supports SHA-1 (used by Mojang asset objects), SHA-512
/// (used by Modrinth files), and SHA-256 (used by Adoptium/Temurin JRE
/// checksums). CurseForge fingerprints are out of scope until Phase 5.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum ExpectedHash {
    /// SHA-1 hex digest.
    Sha1(String),
    /// SHA-256 hex digest.
    Sha256(String),
    /// SHA-512 hex digest.
    Sha512(String),
}

// ---------------------------------------------------------------------------
// Plan items
// ---------------------------------------------------------------------------

/// A single file to download.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadItem {
    /// Source URL.
    pub url: String,
    /// Absolute destination path on disk.
    pub dest: PathBuf,
    /// Expected hash; `None` disables verification (not recommended).
    pub expected_hash: Option<ExpectedHash>,
    /// Expected file size in bytes; `None` if unknown.
    pub size: Option<u64>,
}

/// An ordered list of [`DownloadItem`]s to execute as a unit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadPlan {
    pub items: Vec<DownloadItem>,
}

impl DownloadPlan {
    pub fn new(items: Vec<DownloadItem>) -> Self {
        Self { items }
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// A per-item download failure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadError {
    /// Network or HTTP-level failure.
    Network(String),
    /// Received bytes did not match the expected hash.
    HashMismatch { expected: ExpectedHash, got: String },
    /// File-system I/O failure.
    Io(String),
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DownloadError::Network(msg) => write!(f, "network error: {msg}"),
            DownloadError::HashMismatch { expected, got } => {
                write!(f, "hash mismatch: expected {expected:?}, got {got}")
            }
            DownloadError::Io(msg) => write!(f, "io error: {msg}"),
        }
    }
}

impl std::error::Error for DownloadError {}

// ---------------------------------------------------------------------------
// Progress abstraction
// ---------------------------------------------------------------------------

/// Progress update emitted per chunk while a download is in flight.
#[derive(Debug, Clone)]
pub struct ProgressUpdate {
    /// Source URL of the item in progress.
    pub url: String,
    /// Bytes received so far for this item.
    pub bytes_done: u64,
    /// Total expected bytes for this item; `None` if the server didn't send
    /// `Content-Length`.
    pub bytes_total: Option<u64>,
}

/// Abstraction over progress emission, so the engine core is testable without
/// a live Tauri runtime.
///
/// Implementors: [`NoOpSink`] (tests / one-shot callers that don't need
/// progress) and the Tauri-event sink added in CP-4.
pub trait ProgressSink: Send + Sync {
    fn report(&self, update: ProgressUpdate);
}

/// A [`ProgressSink`] that discards all updates. Zero overhead.
pub struct NoOpSink;

impl ProgressSink for NoOpSink {
    fn report(&self, _update: ProgressUpdate) {}
}

/// A [`ProgressSink`] that collects updates into a `Mutex<Vec>` for
/// inspection in tests.
#[cfg(test)]
pub struct CapturingSink {
    pub updates: std::sync::Mutex<Vec<ProgressUpdate>>,
}

#[cfg(test)]
impl CapturingSink {
    pub fn new() -> Self {
        Self {
            updates: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[cfg(test)]
impl ProgressSink for CapturingSink {
    fn report(&self, update: ProgressUpdate) {
        self.updates.lock().unwrap().push(update);
    }
}

// ---------------------------------------------------------------------------
// Incremental hashing
// ---------------------------------------------------------------------------

/// An incremental hasher that wraps a SHA-1, SHA-256, or SHA-512 computation.
///
/// Call [`update`](Self::update) with successive byte slices (e.g. network
/// chunks or read-buffer chunks), then [`finalize`](Self::finalize) to obtain
/// the lowercase hex digest. Designed so CP-3 can feed the same hasher with
/// chunks as they arrive from the network — no second file read needed.
pub enum IncrementalHasher {
    Sha1(sha1::Sha1),
    Sha256(sha2::Sha256),
    Sha512(sha2::Sha512),
}

impl IncrementalHasher {
    /// Creates a new hasher matching the discriminant of `expected`.
    pub fn for_expected(expected: &ExpectedHash) -> Self {
        match expected {
            ExpectedHash::Sha1(_) => IncrementalHasher::Sha1(sha1::Sha1::new()),
            ExpectedHash::Sha256(_) => IncrementalHasher::Sha256(sha2::Sha256::new()),
            ExpectedHash::Sha512(_) => IncrementalHasher::Sha512(sha2::Sha512::new()),
        }
    }

    /// Feed the next chunk of bytes into the hasher.
    pub fn update(&mut self, data: &[u8]) {
        match self {
            IncrementalHasher::Sha1(h) => h.update(data),
            IncrementalHasher::Sha256(h) => h.update(data),
            IncrementalHasher::Sha512(h) => h.update(data),
        }
    }

    /// Consume the hasher and return the lowercase hex digest.
    pub fn finalize(self) -> String {
        match self {
            IncrementalHasher::Sha1(h) => hex::encode(h.finalize()),
            IncrementalHasher::Sha256(h) => hex::encode(h.finalize()),
            IncrementalHasher::Sha512(h) => hex::encode(h.finalize()),
        }
    }
}

// ---------------------------------------------------------------------------
// Verify + dedupe
// ---------------------------------------------------------------------------

/// Chunk size used when reading files for verification.
///
/// 64 KiB balances memory usage and syscall overhead.
const VERIFY_CHUNK: usize = 64 * 1024;

/// Hash the file at `path` incrementally and compare the result against
/// `expected`.
///
/// Returns `true` if the file exists and its digest matches `expected`.
/// Returns `false` if the file does not exist, cannot be read, or the
/// digest differs.
///
/// Reads in [`VERIFY_CHUNK`]-byte chunks — never loads the whole file.
pub(crate) fn verify(path: &Path, expected: &ExpectedHash) -> bool {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut reader = std::io::BufReader::with_capacity(VERIFY_CHUNK, file);
    let mut hasher = IncrementalHasher::for_expected(expected);
    let mut buf = vec![0u8; VERIFY_CHUNK];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buf[..n]),
            Err(_) => return false,
        }
    }
    let actual = hasher.finalize();
    let expected_hex = match expected {
        ExpectedHash::Sha1(h) | ExpectedHash::Sha256(h) | ExpectedHash::Sha512(h) => h,
    };
    // Case-insensitive comparison in case the caller stored an uppercase digest.
    actual.eq_ignore_ascii_case(expected_hex)
}

/// Returns `true` when the destination already exists and its hash matches
/// `expected` — meaning the download can be skipped (dedupe).
///
/// If `expected` is `None`, the file is assumed to need downloading (no hash
/// to compare against).
pub fn needs_download(dest: &Path, expected: &Option<ExpectedHash>) -> bool {
    match expected {
        Some(hash) => !verify(dest, hash),
        None => true,
    }
}

// ---------------------------------------------------------------------------
// HTTP client builder
// ---------------------------------------------------------------------------

/// Build a `reqwest::Client` with the same user-agent as `core/meta.rs`.
///
/// Callers that already hold a client pass it in; this is provided so tests
/// and the eventual executor can share one instance.
pub fn build_client() -> Result<reqwest::Client, DownloadError> {
    reqwest::Client::builder()
        .user_agent(concat!("modloader/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| DownloadError::Network(e.to_string()))
}

// ---------------------------------------------------------------------------
// Single-file download
// ---------------------------------------------------------------------------

/// Download a single [`DownloadItem`], stream-hashing its bytes, and write
/// the result to disk atomically.
///
/// # Behaviour
///
/// 1. **Dedupe**: if `needs_download` returns false the function returns
///    immediately with no network call.
/// 2. **Resume**: if `<dest>.part` already exists with N bytes, sends
///    `Range: bytes=N-`.  A `206 Partial Content` response resumes
///    (existing bytes are fed through the hasher first); a `200 OK`
///    truncates and restarts.
/// 3. **Atomic rename**: on hash match, `<dest>.part` is renamed to `<dest>`.
///    On mismatch, `.part` is deleted and `DownloadError::HashMismatch` is
///    returned.
/// 4. **Error distinction (F-3)**: all I/O failures surface as
///    `DownloadError::Io`, never as `HashMismatch`.
pub async fn download_item(
    client: &reqwest::Client,
    item: &DownloadItem,
    sink: &dyn ProgressSink,
) -> Result<(), DownloadError> {
    // --- Dedupe ---
    if !needs_download(&item.dest, &item.expected_hash) {
        return Ok(());
    }

    // --- Determine .part path and any existing resume offset ---
    let part_path = part_path_for(&item.dest);

    let resume_offset: u64 = if part_path.exists() {
        std::fs::metadata(&part_path)
            .map_err(|e| DownloadError::Io(e.to_string()))?
            .len()
    } else {
        0
    };

    // --- Issue HTTP request, optionally with Range header ---
    let mut req = client.get(&item.url);
    if resume_offset > 0 {
        req = req.header("Range", format!("bytes={}-", resume_offset));
    }
    let resp = req
        .send()
        .await
        .map_err(|e| DownloadError::Network(e.to_string()))?;

    let status = resp.status();
    if !status.is_success() {
        return Err(DownloadError::Network(format!(
            "HTTP {status} for {}",
            item.url
        )));
    }

    // Ensure the destination parent directory exists before creating .part.
    if let Some(parent) = item.dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| DownloadError::Io(e.to_string()))?;
    }

    // --- Decide resume vs restart based on server response ---
    //
    // F-5 TOCTOU guard: after opening the .part file in append mode, re-read
    // its actual length. If it diverged from `resume_offset` (another writer
    // modified the file between the first `metadata()` call and the open), the
    // Range request we sent used a stale offset. We must abort this response,
    // issue a fresh full GET (no Range header), and restart from byte 0 rather
    // than producing a guaranteed HashMismatch on otherwise-valid data.
    let (mut file, mut hasher, mut bytes_done, resp) =
        if status == reqwest::StatusCode::PARTIAL_CONTENT && resume_offset > 0 {
            // 206: server will send bytes[resume_offset..].
            let f = std::fs::OpenOptions::new()
                .write(true)
                .append(true)
                .open(&part_path)
                .map_err(|e| DownloadError::Io(e.to_string()))?;

            let actual_offset = f
                .metadata()
                .map_err(|e| DownloadError::Io(e.to_string()))?
                .len();

            if actual_offset != resume_offset {
                // Offset diverged — the 206 body starts at the wrong position.
                // Drop the stale handle and the stale response; truncate .part;
                // issue a fresh unconditional GET and stream from byte 0.
                drop(f);
                drop(resp);
                let f2 = std::fs::File::create(&part_path)
                    .map_err(|e| DownloadError::Io(e.to_string()))?;
                let hasher = item
                    .expected_hash
                    .as_ref()
                    .map(IncrementalHasher::for_expected);
                // Re-issue the request without a Range header.
                let resp2 = client
                    .get(&item.url)
                    .send()
                    .await
                    .map_err(|e| DownloadError::Network(e.to_string()))?;
                if !resp2.status().is_success() {
                    return Err(DownloadError::Network(format!(
                        "HTTP {} on restart for {}",
                        resp2.status(),
                        item.url
                    )));
                }
                (f2, hasher, 0u64, resp2)
            } else {
                // Seed the hasher by reading the existing partial bytes through it.
                let hasher = if let Some(expected) = &item.expected_hash {
                    let mut h = IncrementalHasher::for_expected(expected);
                    seed_hasher_from_file(&part_path, resume_offset, &mut h)?;
                    Some(h)
                } else {
                    None
                };
                (f, hasher, resume_offset, resp)
            }
        } else {
            // 200 or any non-206: restart from scratch; truncate .part.
            let f =
                std::fs::File::create(&part_path).map_err(|e| DownloadError::Io(e.to_string()))?;

            let hasher = item
                .expected_hash
                .as_ref()
                .map(IncrementalHasher::for_expected);

            (f, hasher, 0u64, resp)
        };

    // --- Stream body: write to .part and feed hasher ---
    // On a 206 response, `content_length()` is bytes-REMAINING (what the server
    // will send), not the full-file size. Adding `resume_offset` reconstructs
    // the full-file total. On a 200, `resume_offset` is 0 so the addition is
    // a no-op.
    let total = item
        .size
        .or_else(|| resp.content_length().map(|l| l + bytes_done));

    let mut stream = resp.bytes_stream();
    use std::io::Write;
    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| DownloadError::Network(e.to_string()))?;
        file.write_all(&chunk)
            .map_err(|e| DownloadError::Io(e.to_string()))?;
        if let Some(h) = hasher.as_mut() {
            h.update(&chunk);
        }
        bytes_done += chunk.len() as u64;
        sink.report(ProgressUpdate {
            url: item.url.clone(),
            bytes_done,
            bytes_total: total,
        });
    }

    // Flush and close the file before renaming.
    file.flush().map_err(|e| DownloadError::Io(e.to_string()))?;
    drop(file);

    // --- Verify hash ---
    if let (Some(h), Some(expected)) = (hasher, &item.expected_hash) {
        let actual = h.finalize();
        let expected_hex = match expected {
            ExpectedHash::Sha1(s) | ExpectedHash::Sha256(s) | ExpectedHash::Sha512(s) => s,
        };
        if !actual.eq_ignore_ascii_case(expected_hex) {
            // Cleanup .part — best-effort, ignore secondary I/O errors.
            let _ = std::fs::remove_file(&part_path);
            return Err(DownloadError::HashMismatch {
                expected: expected.clone(),
                got: actual,
            });
        }
    }

    // --- Atomic rename (.part → dest; parent already created above) ---
    std::fs::rename(&part_path, &item.dest).map_err(|e| DownloadError::Io(e.to_string()))?;

    Ok(())
}

/// Returns the `.part` path for a given destination path.
fn part_path_for(dest: &Path) -> PathBuf {
    let mut p = dest.as_os_str().to_os_string();
    p.push(".part");
    PathBuf::from(p)
}

/// Reads `len` bytes from `path` and feeds them into `hasher`.
///
/// Used to seed the hasher when resuming a partial download (the bytes
/// already on disk must be included in the final digest). Surfaces I/O
/// failures as `DownloadError::Io`.
fn seed_hasher_from_file(
    path: &Path,
    len: u64,
    hasher: &mut IncrementalHasher,
) -> Result<(), DownloadError> {
    let file = std::fs::File::open(path).map_err(|e| DownloadError::Io(e.to_string()))?;
    let mut reader = std::io::BufReader::with_capacity(VERIFY_CHUNK, file);
    let mut buf = vec![0u8; VERIFY_CHUNK];
    let mut remaining = len as usize;
    while remaining > 0 {
        let to_read = remaining.min(VERIFY_CHUNK);
        let n = reader
            .read(&mut buf[..to_read])
            .map_err(|e| DownloadError::Io(e.to_string()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        remaining -= n;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Aggregate result types
// ---------------------------------------------------------------------------

/// The outcome of a single item within an executed [`DownloadPlan`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemOutcome {
    /// The URL of the item this outcome belongs to.
    pub url: String,
    /// Whether the item succeeded, was skipped (dedupe), or failed.
    pub status: ItemStatus,
}

/// Per-item execution status returned by [`execute_plan`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ItemStatus {
    /// Downloaded and verified successfully.
    Ok,
    /// Destination already existed with a matching hash; no network request made.
    Skipped,
    /// Download or verification failed.
    Failed { error: String },
}

/// Aggregated result returned by [`execute_plan`].
///
/// Every item in the plan is represented exactly once. A failed item does NOT
/// abort the others — all items run to completion, and the caller inspects the
/// outcomes to decide what to do next.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanResult {
    pub outcomes: Vec<ItemOutcome>,
}

// ---------------------------------------------------------------------------
// Cancellation
// ---------------------------------------------------------------------------

/// A cheap, cloneable cancel signal for an in-progress [`execute_plan`] run.
///
/// Cancellation is **cooperative and edge-driven**: each pending item checks
/// the token *before acquiring a semaphore permit*. Once tripped, no new permit
/// is acquired and no new item download starts. Items already holding a permit
/// (in-flight) finish normally — that is acceptable; the higher layer discards
/// staging on cancel, so a finished-but-cancelled item is harmless.
///
/// Clones share the same underlying flag, so a token handed to a worker can be
/// tripped from anywhere (e.g. a `cancel_task` command).
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl CancelToken {
    /// Create a fresh, untripped token.
    pub fn new() -> Self {
        Self::default()
    }

    /// Trip the token. Idempotent.
    pub fn cancel(&self) {
        self.flag.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Returns `true` once [`cancel`](Self::cancel) has been called.
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// Per-item status for a *cancellable* run.
///
/// Wraps the normal [`ItemStatus`] and adds a `Cancelled` case for items that
/// the cancel token tripped before they acquired a permit (no network request
/// made). Kept separate from `ItemStatus` so the non-cancellable
/// [`execute_plan`] contract and its existing consumers are untouched.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CancellableStatus {
    /// The item ran to a terminal [`ItemStatus`] (`Ok` / `Skipped` / `Failed`).
    Ran(ItemStatus),
    /// The run was cancelled before this item acquired a permit; no network
    /// request was made.
    Cancelled,
}

/// Per-item outcome for a cancellable run (URL + [`CancellableStatus`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancellableOutcome {
    pub url: String,
    pub status: CancellableStatus,
}

/// Aggregated result of [`execute_plan_cancellable`].
///
/// Distinct from [`PlanResult`] so adding the cancel seam doesn't perturb the
/// existing `execute_plan` consumers. `cancelled` lets the caller tell a
/// cancelled run apart from a fully-completed one even when in-flight items
/// happened to finish; `outcomes` holds one entry per plan item.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancellablePlanResult {
    pub outcomes: Vec<CancellableOutcome>,
    /// `true` if the run's [`CancelToken`] was tripped before the plan finished.
    pub cancelled: bool,
}

// ---------------------------------------------------------------------------
// Concurrent executor
// ---------------------------------------------------------------------------

/// Execute a [`DownloadPlan`] concurrently, bounded by a semaphore.
///
/// # Parameters
/// - `client` — shared `reqwest` client (cheap to clone, shares a connection pool).
/// - `plan` — the list of items to download.
/// - `sink` — progress sink; called per chunk from each item in parallel.
/// - `concurrency` — maximum number of simultaneous in-flight downloads.
///   Must be ≥ 1. Typical values: 8–16.
///
/// # Behaviour
/// - Items are started in order but run concurrently up to `concurrency`.
/// - A failing item does NOT abort the others — all items run, and the caller
///   inspects [`PlanResult::outcomes`] to handle partial failures.
/// - Items whose dest already exists and matches the hash are marked
///   [`ItemStatus::Skipped`] without issuing a network request.
///
/// This is the uncancellable entry point — it runs the cancellable engine with
/// a never-tripped token and maps the result down to a [`PlanResult`]. Existing
/// callers keep the unchanged 4-arg signature and `PlanResult` return type. New
/// callers that need to stop an in-progress plan use [`execute_plan_cancellable`].
pub async fn execute_plan(
    client: &reqwest::Client,
    plan: &DownloadPlan,
    sink: &(impl ProgressSink + Sync),
    concurrency: usize,
) -> PlanResult {
    let cancellable =
        execute_plan_cancellable(client, plan, sink, concurrency, &CancelToken::new()).await;
    // An untripped token never yields a `Cancelled` item, so every outcome is
    // `Ran(_)` — unwrap to the flat `ItemStatus`.
    let outcomes = cancellable
        .outcomes
        .into_iter()
        .map(|o| ItemOutcome {
            url: o.url,
            status: match o.status {
                CancellableStatus::Ran(status) => status,
                CancellableStatus::Cancelled => {
                    unreachable!("execute_plan passes an untripped token; no item can be cancelled")
                }
            },
        })
        .collect();
    PlanResult { outcomes }
}

/// Cancellable variant of [`execute_plan`] — the cancel seam.
///
/// Same concurrent, hash-verified download behaviour as [`execute_plan`], plus:
/// - **Cancellation:** each pending item checks `cancel` *before acquiring a
///   permit*. Once the token is tripped, no further item starts a download;
///   such items are reported as [`CancellableStatus::Cancelled`]. In-flight
///   items (already holding a permit) finish normally.
/// - [`CancellablePlanResult::cancelled`] is set when the token was tripped, so
///   the caller can tell a cancelled run from a fully-completed one.
pub async fn execute_plan_cancellable(
    client: &reqwest::Client,
    plan: &DownloadPlan,
    sink: &(impl ProgressSink + Sync),
    concurrency: usize,
    cancel: &CancelToken,
) -> CancellablePlanResult {
    use futures_util::stream::{FuturesUnordered, StreamExt as _};
    use std::sync::Arc;
    use tokio::sync::Semaphore;

    // Separate the plan items into those that need downloading vs those
    // that can be skipped immediately (dedupe short-circuit).
    let mut outcomes: Vec<CancellableOutcome> = Vec::with_capacity(plan.items.len());
    let download_items: Vec<&DownloadItem> = plan
        .items
        .iter()
        .filter(|item| {
            if !needs_download(&item.dest, &item.expected_hash) {
                outcomes.push(CancellableOutcome {
                    url: item.url.clone(),
                    status: CancellableStatus::Ran(ItemStatus::Skipped),
                });
                false // skip
            } else {
                true // needs download
            }
        })
        .collect();

    log::info!(
        "download: starting plan — {} item(s) to download, {} already present",
        download_items.len(),
        outcomes.len()
    );

    if download_items.is_empty() {
        return CancellablePlanResult {
            outcomes,
            cancelled: cancel.is_cancelled(),
        };
    }

    // Semaphore limits the number of simultaneous in-flight downloads.
    let semaphore = Arc::new(Semaphore::new(concurrency));

    // Build a FuturesUnordered from all items that need downloading.
    // Each future checks the cancel token, then acquires a semaphore permit
    // before issuing the request, releasing it (by drop) when done. The permit
    // is `OwnedSemaphorePermit` so it does not borrow the semaphore.
    let pending: FuturesUnordered<_> = download_items
        .into_iter()
        .map(|item| {
            let sem = Arc::clone(&semaphore);
            async move {
                // Acquire a permit first (bounds concurrency), then re-check the
                // cancel token *after winning the permit* — the decisive gate.
                // `FuturesUnordered` polls every item future eagerly, so a check
                // before `acquire` would race the trip; checking once the permit
                // is held means a tripped run starts no further download. In-flight
                // items (already past this gate) finish normally.
                let _permit = sem.acquire_owned().await.expect("semaphore closed");
                if cancel.is_cancelled() {
                    return CancellableOutcome {
                        url: item.url.clone(),
                        status: CancellableStatus::Cancelled,
                    };
                }
                let status = match download_item(client, item, sink).await {
                    Ok(()) => ItemStatus::Ok,
                    Err(e) => ItemStatus::Failed {
                        error: e.to_string(),
                    },
                };
                // Permit dropped here → slot released.
                CancellableOutcome {
                    url: item.url.clone(),
                    status: CancellableStatus::Ran(status),
                }
            }
        })
        .collect();

    // Collect all outcomes, running up to `concurrency` at a time.
    let mut downloaded: Vec<CancellableOutcome> = pending.collect().await;
    for outcome in &downloaded {
        if let CancellableStatus::Ran(ItemStatus::Failed { error }) = &outcome.status {
            log::warn!("download: item failed — url={} error={error}", outcome.url);
        }
    }
    outcomes.append(&mut downloaded);

    CancellablePlanResult {
        outcomes,
        cancelled: cancel.is_cancelled(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "download_tests.rs"]
mod tests;
