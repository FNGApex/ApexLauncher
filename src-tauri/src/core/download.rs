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
/// The engine supports SHA-1 (used by Mojang asset objects) and SHA-512
/// (used by Modrinth files). CurseForge fingerprints are out of scope until
/// Phase 5.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum ExpectedHash {
    /// SHA-1 hex digest.
    Sha1(String),
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
    HashMismatch {
        expected: ExpectedHash,
        got: String,
    },
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

/// An incremental hasher that wraps either a SHA-1 or SHA-512 computation.
///
/// Call [`update`](Self::update) with successive byte slices (e.g. network
/// chunks or read-buffer chunks), then [`finalize`](Self::finalize) to obtain
/// the lowercase hex digest. Designed so CP-3 can feed the same hasher with
/// chunks as they arrive from the network — no second file read needed.
pub enum IncrementalHasher {
    Sha1(sha1::Sha1),
    Sha512(sha2::Sha512),
}

impl IncrementalHasher {
    /// Creates a new hasher matching the discriminant of `expected`.
    pub fn for_expected(expected: &ExpectedHash) -> Self {
        match expected {
            ExpectedHash::Sha1(_) => IncrementalHasher::Sha1(sha1::Sha1::new()),
            ExpectedHash::Sha512(_) => IncrementalHasher::Sha512(sha2::Sha512::new()),
        }
    }

    /// Feed the next chunk of bytes into the hasher.
    pub fn update(&mut self, data: &[u8]) {
        match self {
            IncrementalHasher::Sha1(h) => h.update(data),
            IncrementalHasher::Sha512(h) => h.update(data),
        }
    }

    /// Consume the hasher and return the lowercase hex digest.
    pub fn finalize(self) -> String {
        match self {
            IncrementalHasher::Sha1(h) => hex::encode(h.finalize()),
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
pub fn verify(path: &Path, expected: &ExpectedHash) -> bool {
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
        ExpectedHash::Sha1(h) | ExpectedHash::Sha512(h) => h,
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
    let (mut file, mut hasher, mut bytes_done) =
        if status == reqwest::StatusCode::PARTIAL_CONTENT && resume_offset > 0 {
            // 206: server will send bytes[resume_offset..]. Seed the hasher
            // by reading the existing partial bytes through it first.
            let f = std::fs::OpenOptions::new()
                .write(true)
                .append(true)
                .open(&part_path)
                .map_err(|e| DownloadError::Io(e.to_string()))?;

            let hasher = if let Some(expected) = &item.expected_hash {
                let mut h = IncrementalHasher::for_expected(expected);
                seed_hasher_from_file(&part_path, resume_offset, &mut h)?;
                Some(h)
            } else {
                None
            };

            (f, hasher, resume_offset)
        } else {
            // 200 or any non-206: restart from scratch; truncate .part.
            let f = std::fs::File::create(&part_path)
                .map_err(|e| DownloadError::Io(e.to_string()))?;

            let hasher = item
                .expected_hash
                .as_ref()
                .map(IncrementalHasher::for_expected);

            (f, hasher, 0u64)
        };

    // --- Stream body: write to .part and feed hasher ---
    let total = item
        .size
        .or_else(|| resp.content_length().map(|l| l + resume_offset));

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
            ExpectedHash::Sha1(s) | ExpectedHash::Sha512(s) => s,
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Constructs a [`DownloadPlan`] and round-trips it through JSON.
    /// Verifies that field names are camelCase and that both `ExpectedHash`
    /// variants survive serde unchanged.
    #[test]
    fn download_plan_serde_round_trip() {
        let plan = DownloadPlan::new(vec![
            DownloadItem {
                url: "https://resources.download.minecraft.net/ab/abcdef1234".to_owned(),
                dest: PathBuf::from("/tmp/assets/abcdef1234"),
                expected_hash: Some(ExpectedHash::Sha1(
                    "abcdef1234abcdef1234abcdef1234abcdef1234".to_owned(),
                )),
                size: Some(1024),
            },
            DownloadItem {
                url: "https://cdn.modrinth.com/data/somefile.jar".to_owned(),
                dest: PathBuf::from("/tmp/mods/somefile.jar"),
                expected_hash: Some(ExpectedHash::Sha512(
                    "a".repeat(128),
                )),
                size: None,
            },
            DownloadItem {
                url: "https://example.com/no-hash".to_owned(),
                dest: PathBuf::from("/tmp/no-hash"),
                expected_hash: None,
                size: None,
            },
        ]);

        let json = serde_json::to_string(&plan).expect("serialize failed");

        // Spot-check camelCase field names appear in the output.
        assert!(json.contains("\"expectedHash\""), "expectedHash missing from JSON");
        assert!(json.contains("\"bytes_done\"") == false, "snake_case leaked");

        let round_tripped: DownloadPlan =
            serde_json::from_str(&json).expect("deserialize failed");

        assert_eq!(plan, round_tripped);
    }

    /// Verifies the `Sha1` variant serializes with the discriminant `"type":"sha1"`.
    #[test]
    fn expected_hash_sha1_tag() {
        let h = ExpectedHash::Sha1("deadbeef".to_owned());
        let json = serde_json::to_string(&h).unwrap();
        assert!(json.contains("\"sha1\""), "sha1 tag missing: {json}");
        assert!(json.contains("deadbeef"));
        let rt: ExpectedHash = serde_json::from_str(&json).unwrap();
        assert_eq!(h, rt);
    }

    /// Verifies the `Sha512` variant serializes with the discriminant `"type":"sha512"`.
    #[test]
    fn expected_hash_sha512_tag() {
        let h = ExpectedHash::Sha512("cafebabe".to_owned());
        let json = serde_json::to_string(&h).unwrap();
        assert!(json.contains("\"sha512\""), "sha512 tag missing: {json}");
        let rt: ExpectedHash = serde_json::from_str(&json).unwrap();
        assert_eq!(h, rt);
    }

    /// Verifies `DownloadError` variants serialize and round-trip.
    #[test]
    fn download_error_serde_round_trip() {
        let errors = vec![
            DownloadError::Network("connection refused".to_owned()),
            DownloadError::HashMismatch {
                expected: ExpectedHash::Sha1("abc".to_owned()),
                got: "def".to_owned(),
            },
            DownloadError::Io("permission denied".to_owned()),
        ];

        for err in &errors {
            let json = serde_json::to_string(err).expect("serialize failed");
            let rt: DownloadError = serde_json::from_str(&json).expect("deserialize failed");
            assert_eq!(err, &rt);
        }
    }

    // -----------------------------------------------------------------------
    // CP-2: Hashing + dedupe tests
    // -----------------------------------------------------------------------

    /// SHA-1 of empty bytes must equal the NIST/RFC test vector.
    #[test]
    fn incremental_sha1_empty() {
        let mut h = IncrementalHasher::for_expected(&ExpectedHash::Sha1(String::new()));
        h.update(b"");
        assert_eq!(h.finalize(), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    }

    /// SHA-1 of "abc" must equal the well-known FIPS 180 test vector.
    #[test]
    fn incremental_sha1_abc() {
        let mut h = IncrementalHasher::for_expected(&ExpectedHash::Sha1(String::new()));
        h.update(b"abc");
        assert_eq!(h.finalize(), "a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    /// SHA-1 fed in two chunks must equal the single-pass digest.
    #[test]
    fn incremental_sha1_chunked_equals_oneshot() {
        let mut h = IncrementalHasher::for_expected(&ExpectedHash::Sha1(String::new()));
        h.update(b"ab");
        h.update(b"c");
        assert_eq!(h.finalize(), "a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    /// SHA-512 of empty bytes must equal the RFC 4634 test vector.
    #[test]
    fn incremental_sha512_empty() {
        let mut h = IncrementalHasher::for_expected(&ExpectedHash::Sha512(String::new()));
        h.update(b"");
        assert_eq!(
            h.finalize(),
            "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e",
        );
    }

    /// SHA-512 of "abc" must equal the RFC 4634 test vector.
    #[test]
    fn incremental_sha512_abc() {
        let mut h = IncrementalHasher::for_expected(&ExpectedHash::Sha512(String::new()));
        h.update(b"abc");
        assert_eq!(
            h.finalize(),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f",
        );
    }

    /// SHA-512 fed in multiple chunks must equal the single-pass digest.
    #[test]
    fn incremental_sha512_chunked_equals_oneshot() {
        let mut h = IncrementalHasher::for_expected(&ExpectedHash::Sha512(String::new()));
        h.update(b"ab");
        h.update(b"c");
        let chunked = h.finalize();

        let mut h2 = IncrementalHasher::for_expected(&ExpectedHash::Sha512(String::new()));
        h2.update(b"abc");
        assert_eq!(chunked, h2.finalize());
    }

    /// `verify` returns true for a file whose content matches the expected SHA-1.
    #[test]
    fn verify_sha1_match() {
        let dir = std::env::temp_dir();
        let path = dir.join("cp2_verify_sha1_match.bin");
        std::fs::write(&path, b"abc").unwrap();
        let expected = ExpectedHash::Sha1("a9993e364706816aba3e25717850c26c9cd0d89d".to_owned());
        assert!(verify(&path, &expected), "expected verify to return true");
        let _ = std::fs::remove_file(&path);
    }

    /// `verify` returns false when the expected SHA-1 does not match the file.
    #[test]
    fn verify_sha1_mismatch() {
        let dir = std::env::temp_dir();
        let path = dir.join("cp2_verify_sha1_mismatch.bin");
        std::fs::write(&path, b"corrupted content").unwrap();
        let expected = ExpectedHash::Sha1("a9993e364706816aba3e25717850c26c9cd0d89d".to_owned());
        assert!(!verify(&path, &expected), "expected verify to return false");
        let _ = std::fs::remove_file(&path);
    }

    /// `verify` returns false for a file that does not exist.
    #[test]
    fn verify_nonexistent_file() {
        let path = std::path::Path::new("/tmp/cp2_definitely_does_not_exist_xyz.bin");
        let expected = ExpectedHash::Sha1("a9993e364706816aba3e25717850c26c9cd0d89d".to_owned());
        assert!(!verify(path, &expected));
    }

    /// `verify` returns true for a file whose content matches the expected SHA-512.
    #[test]
    fn verify_sha512_match() {
        let dir = std::env::temp_dir();
        let path = dir.join("cp2_verify_sha512_match.bin");
        std::fs::write(&path, b"abc").unwrap();
        let expected = ExpectedHash::Sha512(
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f".to_owned(),
        );
        assert!(verify(&path, &expected), "expected verify to return true");
        let _ = std::fs::remove_file(&path);
    }

    /// `needs_download` returns false (skip) when dest exists and hash matches.
    #[test]
    fn needs_download_skip_when_valid() {
        let dir = std::env::temp_dir();
        let path = dir.join("cp2_dedupe_valid.bin");
        std::fs::write(&path, b"abc").unwrap();
        let expected = Some(ExpectedHash::Sha1(
            "a9993e364706816aba3e25717850c26c9cd0d89d".to_owned(),
        ));
        assert!(!needs_download(&path, &expected), "valid dest must be skipped");
        let _ = std::fs::remove_file(&path);
    }

    /// `needs_download` returns true when dest exists but hash does NOT match.
    #[test]
    fn needs_download_redownload_when_corrupt() {
        let dir = std::env::temp_dir();
        let path = dir.join("cp2_dedupe_corrupt.bin");
        std::fs::write(&path, b"corrupted").unwrap();
        let expected = Some(ExpectedHash::Sha1(
            "a9993e364706816aba3e25717850c26c9cd0d89d".to_owned(),
        ));
        assert!(needs_download(&path, &expected), "corrupt dest must trigger re-download");
        let _ = std::fs::remove_file(&path);
    }

    /// `needs_download` returns true when dest does not exist.
    #[test]
    fn needs_download_when_missing() {
        let path = std::path::Path::new("/tmp/cp2_dedupe_missing_xyz.bin");
        let expected = Some(ExpectedHash::Sha1(
            "a9993e364706816aba3e25717850c26c9cd0d89d".to_owned(),
        ));
        assert!(needs_download(path, &expected), "missing dest must be downloaded");
    }

    /// `needs_download` returns true when `expected` is None (no hash to verify).
    #[test]
    fn needs_download_when_no_hash() {
        let dir = std::env::temp_dir();
        let path = dir.join("cp2_dedupe_nohash.bin");
        std::fs::write(&path, b"anything").unwrap();
        assert!(needs_download(&path, &None), "no hash → always download");
        let _ = std::fs::remove_file(&path);
    }

    // -----------------------------------------------------------------------
    // CP-3 tests: single-file download, resume, mismatch, I/O error, dedupe
    // -----------------------------------------------------------------------

    /// Spawn a minimal HTTP/1.1 server on a random port using a raw tokio
    /// `TcpListener`. Supports:
    ///   - `Range` header → `206 Partial Content` with the requested slice.
    ///   - `no_range = true` → always reply `200` (simulates a server that
    ///     ignores Range).
    ///   - `body` is the full file bytes the server "has".
    ///   - `bad_body` overrides what the server sends (so hash will mismatch).
    #[cfg(test)]
    struct MockServer {
        addr: std::net::SocketAddr,
    }

    #[cfg(test)]
    impl MockServer {
        async fn start(body: Vec<u8>, no_range: bool, bad_body: Option<Vec<u8>>) -> Self {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            use tokio::net::TcpListener;

            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();

            tokio::spawn(async move {
                // Accept exactly one connection, serve one request, then exit.
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut req_buf = vec![0u8; 4096];
                let n = stream.read(&mut req_buf).await.unwrap();
                let req_str = String::from_utf8_lossy(&req_buf[..n]);

                // Parse Range header if present.
                let range_start: Option<u64> = if !no_range {
                    req_str
                        .lines()
                        .find(|l| l.to_ascii_lowercase().starts_with("range:"))
                        .and_then(|l| l.split_once(':'))
                        .map(|(_, v)| v.trim())
                        .and_then(|v| v.strip_prefix("bytes="))
                        .and_then(|v| v.trim_end_matches('-').parse().ok())
                } else {
                    None
                };

                let send_body = bad_body.as_deref().unwrap_or(&body);

                let response = if let Some(start) = range_start {
                    let slice = &body[start as usize..];
                    let send_slice = if bad_body.is_some() {
                        send_body
                    } else {
                        slice
                    };
                    format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\n\r\n",
                        send_slice.len(),
                        start,
                        body.len() - 1,
                        body.len()
                    )
                    .into_bytes()
                    .into_iter()
                    .chain(send_slice.iter().copied())
                    .collect::<Vec<u8>>()
                } else {
                    let send_slice = send_body;
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                        send_slice.len()
                    )
                    .into_bytes()
                    .into_iter()
                    .chain(send_slice.iter().copied())
                    .collect::<Vec<u8>>()
                };

                stream.write_all(&response).await.unwrap();
            });

            MockServer { addr }
        }

        fn url(&self) -> String {
            format!("http://{}", self.addr)
        }
    }

    /// Compute SHA-1 hex of a byte slice (test helper).
    fn sha1_hex(data: &[u8]) -> String {
        let mut h = IncrementalHasher::for_expected(&ExpectedHash::Sha1(String::new()));
        h.update(data);
        h.finalize()
    }

    /// Create a unique temp directory for a test (no external dep).
    fn test_tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cp3_test_{name}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Full download: file lands at dest with correct content and hash.
    #[tokio::test]
    async fn cp3_full_download_lands_at_dest() {
        let body = b"hello download world".to_vec();
        let hash = sha1_hex(&body);
        let server = MockServer::start(body.clone(), false, None).await;

        let dir = test_tmp_dir("full_download");
        // Use a subdir to exercise parent-dir creation.
        let dest = dir.join("subdir").join("file.bin");

        let client = build_client().unwrap();
        let item = DownloadItem {
            url: server.url(),
            dest: dest.clone(),
            expected_hash: Some(ExpectedHash::Sha1(hash)),
            size: None,
        };

        download_item(&client, &item, &NoOpSink).await.unwrap();

        let on_disk = std::fs::read(&dest).unwrap();
        assert_eq!(on_disk, body, "file content must match what the server sent");
        assert!(!part_path_for(&dest).exists(), ".part must not remain after success");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Hash mismatch: .part deleted, DownloadError::HashMismatch returned, no dest.
    #[tokio::test]
    async fn cp3_hash_mismatch_errors_and_cleans_up() {
        let real_body = b"real content".to_vec();
        let bad_body = b"tampered!!  ".to_vec();
        let hash = sha1_hex(&real_body);
        let server = MockServer::start(real_body.clone(), false, Some(bad_body)).await;

        let dir = test_tmp_dir("hash_mismatch");
        let dest = dir.join("file.bin");

        let client = build_client().unwrap();
        let item = DownloadItem {
            url: server.url(),
            dest: dest.clone(),
            expected_hash: Some(ExpectedHash::Sha1(hash.clone())),
            size: None,
        };

        let err = download_item(&client, &item, &NoOpSink).await.unwrap_err();
        match err {
            DownloadError::HashMismatch { .. } => {}
            other => panic!("expected HashMismatch, got {other:?}"),
        }
        assert!(!dest.exists(), "dest must not exist after mismatch");
        assert!(!part_path_for(&dest).exists(), ".part must be deleted after mismatch");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Resume (206): seeded .part + 206-capable server → correct final file.
    #[tokio::test]
    async fn cp3_resume_206_produces_correct_file() {
        let body = b"first part second part".to_vec();
        let hash = sha1_hex(&body);
        let seed_len = 10usize;
        let seed = body[..seed_len].to_vec();

        let server = MockServer::start(body.clone(), false, None).await;

        let dir = test_tmp_dir("resume_206");
        let dest = dir.join("resume.bin");
        let part = part_path_for(&dest);

        std::fs::write(&part, &seed).unwrap();

        let client = build_client().unwrap();
        let item = DownloadItem {
            url: server.url(),
            dest: dest.clone(),
            expected_hash: Some(ExpectedHash::Sha1(hash)),
            size: None,
        };

        download_item(&client, &item, &NoOpSink).await.unwrap();

        let on_disk = std::fs::read(&dest).unwrap();
        assert_eq!(on_disk, body, "resumed file must equal full body");
        assert!(!part.exists(), ".part must be gone after success");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Resume restart (200): server ignores Range → file restarted cleanly.
    #[tokio::test]
    async fn cp3_resume_200_restarts_cleanly() {
        let body = b"clean restart content here".to_vec();
        let hash = sha1_hex(&body);
        let seed = b"stale partial data".to_vec();

        // no_range = true: server always returns 200
        let server = MockServer::start(body.clone(), true, None).await;

        let dir = test_tmp_dir("resume_200");
        let dest = dir.join("restart.bin");
        let part = part_path_for(&dest);

        std::fs::write(&part, &seed).unwrap();

        let client = build_client().unwrap();
        let item = DownloadItem {
            url: server.url(),
            dest: dest.clone(),
            expected_hash: Some(ExpectedHash::Sha1(hash)),
            size: None,
        };

        download_item(&client, &item, &NoOpSink).await.unwrap();

        let on_disk = std::fs::read(&dest).unwrap();
        assert_eq!(on_disk, body, "restarted file must equal full body");
        assert!(!part.exists(), ".part must be gone after success");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Dedupe: valid dest → no network call (server not even started listening).
    #[tokio::test]
    async fn cp3_dedupe_skips_network() {
        let body = b"already here".to_vec();
        let hash = sha1_hex(&body);

        let dir = test_tmp_dir("dedupe");
        let dest = dir.join("cached.bin");
        std::fs::write(&dest, &body).unwrap();

        let client = build_client().unwrap();
        // URL points at a port nothing is listening on — would fail if network hit.
        let item = DownloadItem {
            url: "http://127.0.0.1:1".to_owned(),
            dest: dest.clone(),
            expected_hash: Some(ExpectedHash::Sha1(hash)),
            size: None,
        };

        download_item(&client, &item, &NoOpSink).await.unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// F-3: I/O error creating .part → DownloadError::Io, not HashMismatch.
    ///
    /// Places a regular *file* at the path where the .part *directory* would
    /// need to be created.  `File::create` on `<file>/file.bin.part` fails with
    /// "not a directory" (Unix) / "not a valid path" (Windows) before any hash
    /// computation can occur.  The error must surface as `DownloadError::Io`,
    /// never `DownloadError::HashMismatch` (F-3).
    #[tokio::test]
    async fn cp3_io_error_surfaces_as_io_not_mismatch() {
        let body = b"some data".to_vec();
        let hash = sha1_hex(&body);
        let server = MockServer::start(body.clone(), false, None).await;

        let dir = test_tmp_dir("io_error_f3");
        // Place a regular file at what would be the dest's parent path.
        // File::create("<that_file>/file.bin.part") fails cross-platform.
        let blocker = dir.join("not_a_dir");
        std::fs::write(&blocker, b"I am a file, not a dir").unwrap();
        let dest = blocker.join("file.bin");

        let client = build_client().unwrap();
        let item = DownloadItem {
            url: server.url(),
            dest: dest.clone(),
            expected_hash: Some(ExpectedHash::Sha1(hash)),
            size: None,
        };

        let err = download_item(&client, &item, &NoOpSink).await.unwrap_err();
        match err {
            DownloadError::Io(_) => {}
            DownloadError::HashMismatch { .. } => {
                panic!("I/O error must not be reported as HashMismatch (F-3)")
            }
            other => panic!("unexpected error: {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -----------------------------------------------------------------------
    // CP-1 tests (unchanged below)
    // -----------------------------------------------------------------------

    /// Smoke-tests that `NoOpSink` and `CapturingSink` satisfy the trait.
    #[test]
    fn progress_sink_impls_compile() {
        let noop = NoOpSink;
        noop.report(ProgressUpdate {
            url: "https://example.com".to_owned(),
            bytes_done: 512,
            bytes_total: Some(1024),
        });

        let capture = CapturingSink::new();
        capture.report(ProgressUpdate {
            url: "https://example.com".to_owned(),
            bytes_done: 100,
            bytes_total: None,
        });
        let updates = capture.updates.lock().unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].bytes_done, 100);
    }
}
