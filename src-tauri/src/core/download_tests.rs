//! Unit tests for `download`. Extracted from the source module; wired back
//! via `#[cfg(test)] #[path = "download_tests.rs"] mod tests;` so private items
//! remain accessible through `super::*`.

use super::*;
use std::path::PathBuf;
use std::sync::Arc;

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
            expected_hash: Some(ExpectedHash::Sha512("a".repeat(128))),
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
    assert!(
        json.contains("\"expectedHash\""),
        "expectedHash missing from JSON"
    );
    // Confirm rename_all = "camelCase" actually fired: no snake_case field names
    // may appear. If the attribute were removed, "expected_hash" would appear
    // instead of "expectedHash".
    assert!(
        !json.contains("\"expected_hash\""),
        "snake_case 'expected_hash' leaked — rename_all missing?"
    );
    assert!(
        !json.contains("\"bytes_done\""),
        "snake_case 'bytes_done' leaked — rename_all missing?"
    );

    let round_tripped: DownloadPlan = serde_json::from_str(&json).expect("deserialize failed");

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
    assert!(
        !needs_download(&path, &expected),
        "valid dest must be skipped"
    );
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
    assert!(
        needs_download(&path, &expected),
        "corrupt dest must trigger re-download"
    );
    let _ = std::fs::remove_file(&path);
}

/// `needs_download` returns true when dest does not exist.
#[test]
fn needs_download_when_missing() {
    let path = std::path::Path::new("/tmp/cp2_dedupe_missing_xyz.bin");
    let expected = Some(ExpectedHash::Sha1(
        "a9993e364706816aba3e25717850c26c9cd0d89d".to_owned(),
    ));
    assert!(
        needs_download(path, &expected),
        "missing dest must be downloaded"
    );
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
// CP-1 SHA-256 tests: serde tag, incremental hasher, verify, mock-server
// -----------------------------------------------------------------------

/// `Sha256` variant serializes with `"type":"sha256"` tag and round-trips.
#[test]
fn expected_hash_sha256_tag() {
    let h = ExpectedHash::Sha256("abcd1234".to_owned());
    let json = serde_json::to_string(&h).unwrap();
    assert!(json.contains("\"sha256\""), "sha256 tag missing: {json}");
    assert!(json.contains("abcd1234"));
    let rt: ExpectedHash = serde_json::from_str(&json).unwrap();
    assert_eq!(h, rt);
}

/// SHA-256 of "abc" must equal the well-known test vector.
#[test]
fn incremental_sha256_abc() {
    let mut h = IncrementalHasher::for_expected(&ExpectedHash::Sha256(String::new()));
    h.update(b"abc");
    assert_eq!(
        h.finalize(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

/// SHA-256 produces the same result whether data arrives in one shot or in
/// two chunks.
#[test]
fn incremental_sha256_chunked_equals_oneshot() {
    let data = b"hello sha256 world";
    let mut h_one = IncrementalHasher::for_expected(&ExpectedHash::Sha256(String::new()));
    h_one.update(data);
    let expected = h_one.finalize();

    let mut h_two = IncrementalHasher::for_expected(&ExpectedHash::Sha256(String::new()));
    h_two.update(&data[..8]);
    h_two.update(&data[8..]);
    assert_eq!(h_two.finalize(), expected);
}

/// `verify` returns true when the file content matches the expected SHA-256.
#[test]
fn verify_sha256_match() {
    let dir = std::env::temp_dir();
    let path = dir.join("cp1_verify_sha256_match.bin");
    std::fs::write(&path, b"abc").unwrap();
    let expected = ExpectedHash::Sha256(
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_owned(),
    );
    assert!(verify(&path, &expected), "expected verify to return true");
    let _ = std::fs::remove_file(&path);
}

/// Full sha256 download: file lands at dest with correct content.
#[tokio::test]
async fn cp1_sha256_full_download_lands_at_dest() {
    let body = b"sha256 download body".to_vec();
    let hash = sha256_hex(&body);
    let server = MockServer::start(body.clone(), false, None).await;

    let dir = test_tmp_dir("cp1_sha256_full");
    let dest = dir.join("file.bin");

    let client = build_client().unwrap();
    let item = DownloadItem {
        url: server.url(),
        dest: dest.clone(),
        expected_hash: Some(ExpectedHash::Sha256(hash)),
        size: None,
    };

    download_item(&client, &item, &NoOpSink).await.unwrap();

    let on_disk = std::fs::read(&dest).unwrap();
    assert_eq!(
        on_disk, body,
        "file content must match what the server sent"
    );
    assert!(
        !part_path_for(&dest).exists(),
        ".part must not remain after success"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Sha256 mismatch: .part deleted, HashMismatch returned, no dest.
#[tokio::test]
async fn cp1_sha256_hash_mismatch_errors_and_cleans_up() {
    let real_body = b"correct sha256 content".to_vec();
    let bad_body = b"tampered sha256 content".to_vec();
    let hash = sha256_hex(&real_body);
    let server = MockServer::start(real_body.clone(), false, Some(bad_body)).await;

    let dir = test_tmp_dir("cp1_sha256_mismatch");
    let dest = dir.join("file.bin");

    let client = build_client().unwrap();
    let item = DownloadItem {
        url: server.url(),
        dest: dest.clone(),
        expected_hash: Some(ExpectedHash::Sha256(hash)),
        size: None,
    };

    let err = download_item(&client, &item, &NoOpSink).await.unwrap_err();
    match err {
        DownloadError::HashMismatch { .. } => {}
        other => panic!("expected HashMismatch, got {other:?}"),
    }
    assert!(!dest.exists(), "dest must not exist after mismatch");
    assert!(
        !part_path_for(&dest).exists(),
        ".part must be deleted after mismatch"
    );
    let _ = std::fs::remove_dir_all(&dir);
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
                let send_slice = if bad_body.is_some() { send_body } else { slice };
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

/// Compute SHA-256 hex of a byte slice (test helper).
fn sha256_hex(data: &[u8]) -> String {
    let mut h = IncrementalHasher::for_expected(&ExpectedHash::Sha256(String::new()));
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
    assert_eq!(
        on_disk, body,
        "file content must match what the server sent"
    );
    assert!(
        !part_path_for(&dest).exists(),
        ".part must not remain after success"
    );
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
    assert!(
        !part_path_for(&dest).exists(),
        ".part must be deleted after mismatch"
    );
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
// CP-4 tests: concurrent executor + progress + concurrency bound
// -----------------------------------------------------------------------

/// A multi-connection mock server for CP-4 concurrency tests.
///
/// Tracks the maximum number of simultaneous in-flight connections seen
/// at any point during the test via an atomic high-water mark.
///
/// Hardened per F-7: reads the full HTTP request (not just 4096 bytes),
/// and atomically increments/decrements the concurrent-in-flight counter
/// so the bound assertion is not vacuous.
#[cfg(test)]
struct MultiMockServer {
    addr: std::net::SocketAddr,
    /// High-water mark: the maximum concurrent-in-flight connections seen.
    max_concurrent: Arc<std::sync::atomic::AtomicUsize>,
    /// Total number of body-serving requests handled (200 responses).
    /// Used by the CP-1 cancel test to prove no further downloads start.
    served: Arc<std::sync::atomic::AtomicUsize>,
}

#[cfg(test)]
impl MultiMockServer {
    /// Start a server that responds to any number of connections.
    ///
    /// Each connection receives `body` (same body for every request).
    /// A 404 is returned for any request whose URL path contains "bad".
    ///
    /// `delay_ms`: milliseconds to hold each connection open before sending
    /// the response body. A non-zero value makes concurrent requests overlap
    /// so the high-water mark can reach ≥ 2 in the concurrency bound test.
    async fn start(body: Vec<u8>, delay_ms: u64) -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let max_concurrent = Arc::new(AtomicUsize::new(0));
        let served = Arc::new(AtomicUsize::new(0));
        let current = Arc::new(AtomicUsize::new(0));
        let max_clone = Arc::clone(&max_concurrent);
        let served_clone = Arc::clone(&served);
        let body = Arc::new(body);

        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let current = Arc::clone(&current);
                let max_clone = Arc::clone(&max_clone);
                let served = Arc::clone(&served_clone);
                let body = Arc::clone(&body);

                tokio::spawn(async move {
                    // Track in-flight: increment on entry, update high-water mark,
                    // decrement on exit.
                    let prev = current.fetch_add(1, Ordering::SeqCst);
                    let now = prev + 1;
                    // Update max if this is a new high.
                    let mut cur_max = max_clone.load(Ordering::SeqCst);
                    while now > cur_max {
                        match max_clone.compare_exchange(
                            cur_max,
                            now,
                            Ordering::SeqCst,
                            Ordering::SeqCst,
                        ) {
                            Ok(_) => break,
                            Err(m) => cur_max = m,
                        }
                    }

                    // Read the full HTTP request (headers end at \r\n\r\n).
                    // F-7: read until we see the header terminator, not just 4096.
                    let mut req_bytes = Vec::new();
                    let mut buf = [0u8; 1024];
                    loop {
                        let n = stream.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        req_bytes.extend_from_slice(&buf[..n]);
                        if req_bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let req_str = String::from_utf8_lossy(&req_bytes);

                    // Hold the connection open so concurrent requests overlap and
                    // the high-water mark can reach ≥ 2 (F-8 anti-vacuousness).
                    if delay_ms > 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    }

                    // Return 404 for paths containing "bad".
                    let is_bad = req_str.lines().next().map_or(false, |l| l.contains("bad"));
                    let response = if is_bad {
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec()
                    } else {
                        served.fetch_add(1, Ordering::SeqCst);
                        let mut resp =
                            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len())
                                .into_bytes();
                        resp.extend_from_slice(&body);
                        resp
                    };

                    let _ = stream.write_all(&response).await;
                    // Decrement after response is sent (connection complete).
                    current.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });

        MultiMockServer {
            addr,
            max_concurrent,
            served,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}/{}", self.addr, path)
    }
}

/// A CapturingSink that also atomically tracks concurrent reporters,
/// giving a high-water mark of simultaneous progress reports.
#[cfg(test)]
struct ConcurrencyTrackingSink {
    updates: std::sync::Mutex<Vec<ProgressUpdate>>,
}

#[cfg(test)]
impl ConcurrencyTrackingSink {
    fn new() -> Self {
        Self {
            updates: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[cfg(test)]
impl ProgressSink for ConcurrencyTrackingSink {
    fn report(&self, update: ProgressUpdate) {
        // Verify url and bytes_total fields are populated (F-2 regression guard).
        let _ = &update.url;
        let _ = &update.bytes_total;
        self.updates.lock().unwrap().push(update);
    }
}

/// Multi-item plan: all items download, outcomes all Ok, file contents correct.
#[tokio::test]
async fn cp4_multi_item_plan_all_succeed() {
    let body = b"cp4 test content".to_vec();
    let hash = sha1_hex(&body);
    let server = MultiMockServer::start(body.clone(), 0).await;

    let dir = test_tmp_dir("cp4_multi");
    let client = build_client().unwrap();

    let items: Vec<DownloadItem> = (0..4)
        .map(|i| DownloadItem {
            url: server.url(&format!("file{i}")),
            dest: dir.join(format!("file{i}.bin")),
            expected_hash: Some(ExpectedHash::Sha1(hash.clone())),
            size: None,
        })
        .collect();

    let plan = DownloadPlan::new(items.clone());
    let sink = ConcurrencyTrackingSink::new();
    let result = execute_plan(&client, &plan, &sink, 4).await;

    assert_eq!(result.outcomes.len(), 4, "must have one outcome per item");
    for outcome in &result.outcomes {
        match &outcome.status {
            ItemStatus::Ok => {}
            other => panic!("expected Ok, got {other:?} for {}", outcome.url),
        }
    }
    // Verify file contents on disk.
    for item in &items {
        let on_disk = std::fs::read(&item.dest).unwrap();
        assert_eq!(on_disk, body, "content mismatch for {:?}", item.dest);
    }
    // Progress sink received at least one update per item.
    let updates = sink.updates.lock().unwrap();
    assert!(
        updates.len() >= 4,
        "expected ≥4 progress updates, got {}",
        updates.len()
    );
    // F-2: url field is populated in at least one update.
    assert!(
        updates.iter().all(|u| !u.url.is_empty()),
        "url must be populated"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Failing item (404) does not abort the plan; other items succeed.
#[tokio::test]
async fn cp4_failing_item_does_not_abort_plan() {
    let body = b"good content here".to_vec();
    let hash = sha1_hex(&body);
    let server = MultiMockServer::start(body.clone(), 0).await;

    let dir = test_tmp_dir("cp4_partial_fail");
    let client = build_client().unwrap();

    // Item 0: good. Item 1: "bad" path → 404. Item 2: good. Item 3: good.
    let items = vec![
        DownloadItem {
            url: server.url("good0"),
            dest: dir.join("good0.bin"),
            expected_hash: Some(ExpectedHash::Sha1(hash.clone())),
            size: None,
        },
        DownloadItem {
            url: server.url("bad1"),
            dest: dir.join("bad1.bin"),
            expected_hash: Some(ExpectedHash::Sha1(hash.clone())),
            size: None,
        },
        DownloadItem {
            url: server.url("good2"),
            dest: dir.join("good2.bin"),
            expected_hash: Some(ExpectedHash::Sha1(hash.clone())),
            size: None,
        },
        DownloadItem {
            url: server.url("good3"),
            dest: dir.join("good3.bin"),
            expected_hash: Some(ExpectedHash::Sha1(hash.clone())),
            size: None,
        },
    ];

    let plan = DownloadPlan::new(items.clone());
    let sink = NoOpSink;
    let result = execute_plan(&client, &plan, &sink, 4).await;

    assert_eq!(result.outcomes.len(), 4);

    // Good items succeeded.
    let good_urls = [
        items[0].url.as_str(),
        items[2].url.as_str(),
        items[3].url.as_str(),
    ];
    for outcome in result
        .outcomes
        .iter()
        .filter(|o| good_urls.contains(&o.url.as_str()))
    {
        match &outcome.status {
            ItemStatus::Ok => {}
            other => panic!("expected Ok for {}, got {other:?}", outcome.url),
        }
    }

    // Bad item failed.
    let bad_outcome = result
        .outcomes
        .iter()
        .find(|o| o.url == items[1].url)
        .unwrap();
    match &bad_outcome.status {
        ItemStatus::Failed { .. } => {}
        other => panic!("expected Failed for bad item, got {other:?}"),
    }

    // Good files exist on disk.
    assert!(items[0].dest.exists(), "good0 must be on disk");
    assert!(items[2].dest.exists(), "good2 must be on disk");
    assert!(items[3].dest.exists(), "good3 must be on disk");
    assert!(!items[1].dest.exists(), "bad1 must not be on disk");

    let _ = std::fs::remove_dir_all(&dir);
}

/// Concurrency bound: in-flight connections never exceed the semaphore limit,
/// and the test is non-vacuous: ≥ 2 connections are observed in flight at once
/// (F-8). The mock holds each connection for 20 ms so requests overlap.
#[tokio::test]
async fn cp4_concurrency_bound_not_exceeded() {
    let body = b"bound test content".to_vec();
    let hash = sha1_hex(&body);
    // 20 ms hold per connection: long enough for ≥ 2 to overlap, short enough
    // that the test completes in well under a second (8 items × 20 ms / 3 = ~54 ms).
    let server = MultiMockServer::start(body.clone(), 20).await;

    let dir = test_tmp_dir("cp4_bound");
    let client = build_client().unwrap();
    let concurrency = 3usize;

    // 8 items, bound = 3: only 3 in-flight at a time.
    let items: Vec<DownloadItem> = (0..8)
        .map(|i| DownloadItem {
            url: server.url(&format!("item{i}")),
            dest: dir.join(format!("item{i}.bin")),
            expected_hash: Some(ExpectedHash::Sha1(hash.clone())),
            size: None,
        })
        .collect();

    let plan = DownloadPlan::new(items);
    let sink = NoOpSink;
    let result = execute_plan(&client, &plan, &sink, concurrency).await;

    // All 8 must succeed.
    assert_eq!(result.outcomes.len(), 8);
    for outcome in &result.outcomes {
        match &outcome.status {
            ItemStatus::Ok => {}
            other => panic!("expected Ok, got {other:?} for {}", outcome.url),
        }
    }

    // The mock server tracks max simultaneous connections.
    let observed_max = server
        .max_concurrent
        .load(std::sync::atomic::Ordering::SeqCst);
    assert!(
        observed_max <= concurrency,
        "max concurrent connections {observed_max} exceeded bound {concurrency}"
    );
    // Verify the test is non-vacuous: ≥ 2 connections were in flight simultaneously,
    // proving the executor ran concurrently (F-8). The 20 ms mock delay guarantees
    // overlap when concurrency > 1.
    assert!(
        observed_max >= 2,
        "max concurrent connections {observed_max} < 2 — concurrent execution not proven (F-8)"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Dedupe in executor: items whose dest already exists with correct hash are Skipped.
#[tokio::test]
async fn cp4_executor_dedupes_existing_files() {
    let body = b"already cached content".to_vec();
    let hash = sha1_hex(&body);
    // No server needed — dedupe should prevent any network access.
    let dir = test_tmp_dir("cp4_dedupe");
    let client = build_client().unwrap();

    let dest = dir.join("cached.bin");
    std::fs::write(&dest, &body).unwrap();

    let items = vec![DownloadItem {
        url: "http://127.0.0.1:1/would-fail".to_owned(),
        dest: dest.clone(),
        expected_hash: Some(ExpectedHash::Sha1(hash.clone())),
        size: None,
    }];

    let plan = DownloadPlan::new(items);
    let sink = NoOpSink;
    let result = execute_plan(&client, &plan, &sink, 4).await;

    assert_eq!(result.outcomes.len(), 1);
    match &result.outcomes[0].status {
        ItemStatus::Skipped => {}
        other => panic!("expected Skipped, got {other:?}"),
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// PlanResult round-trips through serde (IPC-boundary check).
#[test]
fn plan_result_serde_round_trip() {
    let result = PlanResult {
        outcomes: vec![
            ItemOutcome {
                url: "https://a.example".to_owned(),
                status: ItemStatus::Ok,
            },
            ItemOutcome {
                url: "https://b.example".to_owned(),
                status: ItemStatus::Skipped,
            },
            ItemOutcome {
                url: "https://c.example".to_owned(),
                status: ItemStatus::Failed {
                    error: "HTTP 404".to_owned(),
                },
            },
        ],
    };
    let json = serde_json::to_string(&result).expect("serialize");
    // rename_all = "camelCase" on ItemStatus produces deterministic lowercase tags.
    // Asserting exact form so removing the attribute would fail this test.
    assert!(
        json.contains("\"ok\""),
        "expected lowercase \"ok\" tag — rename_all = \"camelCase\" missing?"
    );
    assert!(
        !json.contains("\"Ok\""),
        "\"Ok\" must not appear — rename_all must produce \"ok\""
    );
    assert!(
        json.contains("\"skipped\""),
        "expected lowercase \"skipped\" tag"
    );
    assert!(
        !json.contains("\"Skipped\""),
        "\"Skipped\" must not appear — rename_all must produce \"skipped\""
    );
    assert!(
        json.contains("\"failed\""),
        "expected lowercase \"failed\" tag"
    );
    assert!(
        !json.contains("\"Failed\""),
        "\"Failed\" must not appear — rename_all must produce \"failed\""
    );
    let rt: PlanResult = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(rt.outcomes.len(), 3);
}

// -----------------------------------------------------------------------
// F-5 test: TOCTOU guard on .part resume
// -----------------------------------------------------------------------

/// F-5 TOCTOU guard: if the `.part` file grows between the first `metadata()`
/// call and the append-mode open, `download_item` detects the divergence,
/// issues a fresh full GET, and produces the correct final file.
///
/// Design: two `Arc<Notify>` gates make the race deterministic —
///   1. Mock reads the Range request, notifies `req1_received`, then waits
///      for `continue_req1` before sending the 206.
///   2. This task: on `req1_received`, appends GARBAGE to `.part`, then
///      signals `continue_req1` so the mock can proceed.
///
/// Sequence:
///   A. `download_item` calls metadata() → resume_offset=5; sends Range GET.
///   B. Mock receives GET, fires `req1_received`, suspends.
///   C. This task appends GARBAGE → .part is now 12 bytes, then fires `continue_req1`.
///   D. Mock sends 206 headers + body; `send().await` returns.
///   E. `download_item` opens .part in append mode → actual_offset=12 ≠ 5 → guard fires.
///   F. Guard truncates .part, issues fresh GET → mock serves 200 + full body.
///   G. Final file = full body, hash passes.
#[tokio::test]
async fn f5_toctou_part_grows_triggers_clean_restart() {
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::Notify;

    let full_body = b"abcdefghijklmnopqrstuvwxyz".to_vec(); // 26 bytes
    let hash = sha1_hex(&full_body);
    let seed_len: usize = 5;

    // Coordination gates.
    let req1_received = Arc::new(Notify::new());
    let continue_req1 = Arc::new(Notify::new());
    let req1_rx = Arc::clone(&req1_received);
    let cont_tx = Arc::clone(&continue_req1);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let body_arc = Arc::new(full_body.clone());

    // Mock: serves request 1 (206, gated) then request 2 (200, immediate).
    tokio::spawn({
        let body_arc = Arc::clone(&body_arc);
        async move {
            // --- Request 1: Range ---
            if let Ok((mut s, _)) = listener.accept().await {
                let body = Arc::clone(&body_arc);
                let req1_notify = Arc::clone(&req1_rx);
                let cont_notify = Arc::clone(&cont_tx);
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let _ = s.read(&mut buf).await;
                    // Signal test: request received, .part can be corrupted now.
                    req1_notify.notify_one();
                    // Wait: test signals us to proceed.
                    cont_notify.notified().await;
                    // Respond 206. Connection: close prevents socket reuse.
                    let sl = seed_len;
                    let slice = &body[sl..];
                    let hdr = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\n\
                             Content-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                        slice.len(),
                        sl,
                        body.len() - 1,
                        body.len()
                    );
                    let mut out = hdr.into_bytes();
                    out.extend_from_slice(slice);
                    let _ = s.write_all(&out).await;
                });
            }
            // --- Request 2: full GET (restart) ---
            if let Ok((mut s, _)) = listener.accept().await {
                let body = Arc::clone(&body_arc);
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let _ = s.read(&mut buf).await;
                    let hdr = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let mut out = hdr.into_bytes();
                    out.extend_from_slice(&body);
                    let _ = s.write_all(&out).await;
                });
            }
        }
    });

    let dir = test_tmp_dir("f5_toctou");
    let dest = dir.join("toctou.bin");
    let part = part_path_for(&dest);
    std::fs::write(&part, &full_body[..seed_len]).unwrap();

    // No idle-pool: ensures restart GET opens a fresh TCP connection.
    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(0)
        .build()
        .unwrap();

    let item = DownloadItem {
        url: format!("http://{addr}"),
        dest: dest.clone(),
        expected_hash: Some(ExpectedHash::Sha1(hash.clone())),
        size: None,
    };

    // Concurrently corrupt .part AFTER req1 is received, BEFORE mock responds.
    let part_clone = part.clone();
    let corruptor = tokio::spawn(async move {
        req1_received.notified().await;
        tokio::task::spawn_blocking(move || {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&part_clone)
                .unwrap();
            f.write_all(b"GARBAGE").unwrap();
        })
        .await
        .unwrap();
        continue_req1.notify_one();
    });

    // Must succeed: guard detects actual_offset(12) ≠ resume_offset(5), restarts.
    download_item(&client, &item, &NoOpSink).await.unwrap();
    corruptor.await.unwrap();

    let on_disk = std::fs::read(&dest).unwrap();
    assert_eq!(
        on_disk, full_body,
        "final file must match full body after TOCTOU restart"
    );
    assert!(!part.exists(), ".part must be gone after success");
    let _ = std::fs::remove_dir_all(&dir);
}

// -----------------------------------------------------------------------
// CP-1 (rework) test: download cancel seam
// -----------------------------------------------------------------------

/// Cancel seam: once the token trips while a plan is in flight, no further
/// item starts a download. Items that never acquired a permit yield
/// `CancellableStatus::Cancelled`, and the result flags the run as cancelled so
/// the caller can tell it apart from a completed run.
///
/// Determinism: `concurrency = 1` forces strictly serial item execution. The
/// first item holds the only permit and the mock holds the connection open for
/// `delay`; meanwhile a spawned task trips the token. By the time item 1
/// releases its permit, the token is tripped, so items 2..N short-circuit
/// before acquiring a permit — no second request reaches the server.
#[tokio::test]
async fn cp1_cancel_stops_further_downloads_and_is_distinguishable() {
    let body = b"cancel seam content".to_vec();
    let hash = sha1_hex(&body);
    // 200 ms hold on the first (and only) in-flight connection — long enough
    // for the cancel task to trip the token before item 1 releases its permit.
    let server = MultiMockServer::start(body.clone(), 200).await;

    let dir = test_tmp_dir("cp1_cancel");
    let client = build_client().unwrap();

    // 4 items; concurrency = 1 → strictly serial.
    let items: Vec<DownloadItem> = (0..4)
        .map(|i| DownloadItem {
            url: server.url(&format!("cancel{i}")),
            dest: dir.join(format!("cancel{i}.bin")),
            expected_hash: Some(ExpectedHash::Sha1(hash.clone())),
            size: None,
        })
        .collect();
    let plan = DownloadPlan::new(items);

    let cancel = CancelToken::new();
    // Trip the token shortly after the run starts — well within item 1's 200 ms hold.
    let cancel_trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        cancel_trigger.cancel();
    });

    let sink = NoOpSink;
    let result = execute_plan_cancellable(&client, &plan, &sink, 1, &cancel).await;

    // The result distinguishes a cancelled run from a completed one.
    assert!(result.cancelled, "result must report the run as cancelled");

    // Exactly one body-serving request reached the server (item 0 only).
    let served = server.served.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        served, 1,
        "only the first item may download; got {served} served requests"
    );

    // One outcome per item; at least one Cancelled outcome present.
    assert_eq!(result.outcomes.len(), 4);
    let cancelled = result
        .outcomes
        .iter()
        .filter(|o| matches!(o.status, CancellableStatus::Cancelled))
        .count();
    assert!(
        cancelled >= 1,
        "expected ≥1 Cancelled outcome, got {cancelled}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// A plan run with a never-tripped token completes exactly as before:
/// the result is not flagged cancelled and no Cancelled outcomes appear.
#[tokio::test]
async fn cp1_untripped_token_completes_normally() {
    let body = b"untripped content".to_vec();
    let hash = sha1_hex(&body);
    let server = MultiMockServer::start(body.clone(), 0).await;

    let dir = test_tmp_dir("cp1_untripped");
    let client = build_client().unwrap();

    let items: Vec<DownloadItem> = (0..3)
        .map(|i| DownloadItem {
            url: server.url(&format!("ok{i}")),
            dest: dir.join(format!("ok{i}.bin")),
            expected_hash: Some(ExpectedHash::Sha1(hash.clone())),
            size: None,
        })
        .collect();
    let plan = DownloadPlan::new(items);

    let cancel = CancelToken::new();
    let sink = NoOpSink;
    let result = execute_plan_cancellable(&client, &plan, &sink, 4, &cancel).await;

    assert!(!result.cancelled, "untripped run must not be cancelled");
    assert_eq!(result.outcomes.len(), 3);
    for outcome in &result.outcomes {
        match &outcome.status {
            CancellableStatus::Ran(ItemStatus::Ok) => {}
            other => panic!("expected Ran(Ok), got {other:?} for {}", outcome.url),
        }
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

// -----------------------------------------------------------------------
// F2-1: item_done fires once per plan item (success / fail / skip mix)
// -----------------------------------------------------------------------

/// `item_done` fires exactly once per plan item for a mix of success, skip,
/// and fail outcomes.  Uses a 3-item plan: one already-on-disk (Skipped), one
/// normal download (Ok), one bad URL (Failed).
///
/// The skipped item fires `item_done(url, true)` in the early-dedup path.
/// The ok item fires `item_done(url, true)` after the download completes.
/// The bad item fires `item_done(url, false)` after the 404.
#[tokio::test]
async fn f2_1_item_done_fires_once_per_item_success_fail_skip_mix() {
    let body = b"item_done test body".to_vec();
    let hash = sha1_hex(&body);
    let server = MultiMockServer::start(body.clone(), 0).await;

    let dir = test_tmp_dir("f2_1_item_done_mix");
    let client = build_client().unwrap();

    // Item 0: pre-populate on disk so it gets Skipped (dedupe).
    let skipped_dest = dir.join("skipped.bin");
    std::fs::write(&skipped_dest, &body).unwrap();

    // Item 1: normal download (Ok).
    let ok_dest = dir.join("ok.bin");

    // Item 2: bad URL → 404 → Failed.
    let bad_dest = dir.join("bad.bin");

    let items = vec![
        DownloadItem {
            url: server.url("skipped"),
            dest: skipped_dest.clone(),
            expected_hash: Some(ExpectedHash::Sha1(hash.clone())),
            size: None,
        },
        DownloadItem {
            url: server.url("ok"),
            dest: ok_dest.clone(),
            expected_hash: Some(ExpectedHash::Sha1(hash.clone())),
            size: None,
        },
        DownloadItem {
            url: server.url("bad"),
            dest: bad_dest.clone(),
            expected_hash: Some(ExpectedHash::Sha1(hash.clone())),
            size: None,
        },
    ];

    let plan = DownloadPlan::new(items.clone());
    let sink = CapturingItemSink::new();
    let cancel = CancelToken::new();
    let result = execute_plan_cancellable(&client, &plan, &sink, 4, &cancel).await;

    // All three items must be in the result.
    assert_eq!(result.outcomes.len(), 3, "expected 3 outcomes");

    // item_done must fire exactly once per item — 3 total.
    let recorded = sink.items.lock().unwrap();
    assert_eq!(recorded.len(), 3, "item_done must fire exactly 3 times, got {}", recorded.len());

    // Verify per-item URLs and success flags.
    // The skipped item is processed before the download phase, so it appears
    // first in recorded order; ok and bad order is non-deterministic (concurrent),
    // so we match by URL.
    let find = |url_suffix: &str| recorded.iter().find(|(u, _)| u.contains(url_suffix)).cloned();

    let (_, skipped_ok) = find("skipped").expect("skipped item_done missing");
    assert!(skipped_ok, "skipped item must be success=true");

    let (_, ok_ok) = find("/ok").expect("ok item_done missing");
    assert!(ok_ok, "ok item must be success=true");

    let (_, bad_ok) = find("bad").expect("bad item_done missing");
    assert!(!bad_ok, "bad (404) item must be success=false");

    let _ = std::fs::remove_dir_all(&dir);
}

// -----------------------------------------------------------------------
// MD5 hash support tests (ATLauncher CP-2)
// -----------------------------------------------------------------------

/// `Md5` variant serializes with `"type":"md5"` tag and round-trips.
#[test]
fn expected_hash_md5_tag() {
    let h = ExpectedHash::Md5("5d41402abc4b2a76b9719d911017c592".to_owned());
    let json = serde_json::to_string(&h).unwrap();
    assert!(json.contains("\"md5\""), "md5 tag missing: {json}");
    assert!(json.contains("5d41402abc4b2a76b9719d911017c592"));
    let rt: ExpectedHash = serde_json::from_str(&json).unwrap();
    assert_eq!(h, rt);
}

/// `verify` returns true for a file whose content matches the expected MD5.
/// MD5("hello") = 5d41402abc4b2a76b9719d911017c592 (well-known vector).
#[test]
fn verify_md5_match() {
    let dir = std::env::temp_dir();
    let path = dir.join("cp2_atl_verify_md5_match.bin");
    std::fs::write(&path, b"hello").unwrap();
    let expected = ExpectedHash::Md5("5d41402abc4b2a76b9719d911017c592".to_owned());
    assert!(verify(&path, &expected), "expected verify to return true for MD5 match");
    let _ = std::fs::remove_file(&path);
}

/// `verify` returns false when the expected MD5 does not match the file content.
#[test]
fn verify_md5_mismatch() {
    let dir = std::env::temp_dir();
    let path = dir.join("cp2_atl_verify_md5_mismatch.bin");
    std::fs::write(&path, b"wrong content").unwrap();
    let expected = ExpectedHash::Md5("5d41402abc4b2a76b9719d911017c592".to_owned());
    assert!(!verify(&path, &expected), "expected verify to return false for MD5 mismatch");
    let _ = std::fs::remove_file(&path);
}

/// `item_done` fires for cancelled items (success=false) and the total count
/// still equals the plan length.
#[tokio::test]
async fn f2_1_item_done_fires_for_cancelled_items() {
    let body = b"cancel item_done body".to_vec();
    let hash = sha1_hex(&body);
    // 200 ms hold: ensures the cancel token trips before item 1 starts.
    let server = MultiMockServer::start(body.clone(), 200).await;

    let dir = test_tmp_dir("f2_1_item_done_cancel");
    let client = build_client().unwrap();

    // 3 items; concurrency = 1 so item 0 holds the permit while 1 and 2 wait.
    let items: Vec<DownloadItem> = (0..3)
        .map(|i| DownloadItem {
            url: server.url(&format!("ci{i}")),
            dest: dir.join(format!("ci{i}.bin")),
            expected_hash: Some(ExpectedHash::Sha1(hash.clone())),
            size: None,
        })
        .collect();

    let plan = DownloadPlan::new(items.clone());
    let cancel = CancelToken::new();
    let cancel_trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        cancel_trigger.cancel();
    });

    let sink = CapturingItemSink::new();
    let result = execute_plan_cancellable(&client, &plan, &sink, 1, &cancel).await;

    assert!(result.cancelled, "run must be flagged cancelled");

    // item_done must fire for every item in the plan — including cancelled ones.
    let recorded = sink.items.lock().unwrap();
    assert_eq!(
        recorded.len(),
        items.len(),
        "item_done must fire for all {} items, got {}",
        items.len(),
        recorded.len()
    );

    // At least one cancelled item must have success=false.
    let false_count = recorded.iter().filter(|(_, ok)| !ok).count();
    assert!(false_count >= 1, "expected ≥1 success=false from cancelled items");

    let _ = std::fs::remove_dir_all(&dir);
}
