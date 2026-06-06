//! Concurrent, hash-verified download engine.
//!
//! Executes a [`DownloadPlan`] (a list of [`DownloadItem`]s), verifies each
//! file's hash on completion, and streams progress through a [`ProgressSink`].
//! No Minecraft-specific logic lives here — the engine is resolver-agnostic.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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
