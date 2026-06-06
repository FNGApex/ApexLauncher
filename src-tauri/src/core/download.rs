//! Concurrent, hash-verified download engine.
//!
//! Executes a [`DownloadPlan`] (a list of [`DownloadItem`]s), verifies each
//! file's hash on completion, and streams progress through a [`ProgressSink`].
//! No Minecraft-specific logic lives here — the engine is resolver-agnostic.

use std::io::Read;
use std::path::{Path, PathBuf};

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
