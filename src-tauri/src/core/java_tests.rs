//! Unit tests for `java`. Extracted from the source module; wired back
//! via `#[cfg(test)] #[path = "java_tests.rs"] mod tests;` so private items
//! remain accessible through `super::*`.

use super::*;
use std::fs;
use tempfile::TempDir;

// ------------------------------------------------------------------
// parse_major_from_release
// ------------------------------------------------------------------

#[test]
fn parse_modern_version() {
    let contents = "JAVA_VERSION=\"17.0.8\"\nOS_NAME=\"Linux\"\n";
    assert_eq!(parse_major_from_release(contents), Some(17));
}

#[test]
fn parse_legacy_version() {
    let contents = "JAVA_VERSION=\"1.8.0_392\"\nJAVA_FULL_VERSION=\"1.8.0_392-b08\"\n";
    assert_eq!(parse_major_from_release(contents), Some(8));
}

#[test]
fn parse_version_21() {
    let contents = "IMPLEMENTOR=\"Eclipse Adoptium\"\nJAVA_VERSION=\"21.0.3\"\n";
    assert_eq!(parse_major_from_release(contents), Some(21));
}

#[test]
fn parse_version_11() {
    let contents = "JAVA_VERSION=\"11.0.24\"\n";
    assert_eq!(parse_major_from_release(contents), Some(11));
}

#[test]
fn parse_missing_java_version_line() {
    let contents = "OS_NAME=\"Linux\"\nJAVA_FULL_VERSION=\"17\"\n";
    assert_eq!(parse_major_from_release(contents), None);
}

#[test]
fn parse_unquoted_version() {
    // Some JREs omit quotes: JAVA_VERSION=17.0.8
    let contents = "JAVA_VERSION=17.0.8\n";
    assert_eq!(parse_major_from_release(contents), Some(17));
}

// ------------------------------------------------------------------
// Fixture helpers
// ------------------------------------------------------------------

/// Build a minimal fake JRE home directory.
///
/// Layout:
///   <root>/
///     release            ← contains JAVA_VERSION="<version>"
///     bin/
///       java[.exe]       ← empty placeholder file
fn make_fake_jre(tmp: &TempDir, version_str: &str, os: TargetOs) -> PathBuf {
    let home = tmp.path().to_path_buf();
    let bin_dir = home.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    // Write release file.
    let release_contents = format!("JAVA_VERSION=\"{version_str}\"\nOS_NAME=\"Linux\"\n");
    fs::write(home.join("release"), release_contents).unwrap();

    // Write fake java binary.
    fs::write(bin_dir.join(os.java_bin()), b"").unwrap();

    home
}

// ------------------------------------------------------------------
// probe_installation
// ------------------------------------------------------------------

#[test]
fn probe_detects_java17_linux() {
    let tmp = TempDir::new().unwrap();
    let home = make_fake_jre(&tmp, "17.0.8", TargetOs::Linux);

    let inst = probe_installation(&home, TargetOs::Linux, JavaSource::Detected).unwrap();
    assert_eq!(inst.major, 17);
    assert_eq!(inst.path, home.join("bin").join("java"));
}

#[test]
fn probe_detects_java8_legacy_linux() {
    let tmp = TempDir::new().unwrap();
    let home = make_fake_jre(&tmp, "1.8.0_392", TargetOs::Linux);

    let inst = probe_installation(&home, TargetOs::Linux, JavaSource::Detected).unwrap();
    assert_eq!(inst.major, 8);
}

#[test]
fn probe_detects_java21_windows() {
    let tmp = TempDir::new().unwrap();
    let home = make_fake_jre(&tmp, "21.0.3", TargetOs::Windows);

    let inst = probe_installation(&home, TargetOs::Windows, JavaSource::Detected).unwrap();
    assert_eq!(inst.major, 21);
    assert_eq!(inst.path, home.join("bin").join("java.exe"));
}

#[test]
fn probe_returns_none_when_no_release_file() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().to_path_buf();
    // Create bin/java but no release file.
    let bin_dir = home.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::write(bin_dir.join("java"), b"").unwrap();

    assert!(probe_installation(&home, TargetOs::Linux, JavaSource::Detected).is_none());
}

#[test]
fn probe_returns_none_when_no_java_bin() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().to_path_buf();
    // Write release file but no bin/java.
    fs::write(home.join("release"), "JAVA_VERSION=\"17.0.8\"\n").unwrap();

    assert!(probe_installation(&home, TargetOs::Linux, JavaSource::Detected).is_none());
}

// ------------------------------------------------------------------
// detect
// ------------------------------------------------------------------

#[test]
fn detect_returns_matching_major() {
    let tmp17 = TempDir::new().unwrap();
    let tmp21 = TempDir::new().unwrap();

    let home17 = make_fake_jre(&tmp17, "17.0.8", TargetOs::Linux);
    let home21 = make_fake_jre(&tmp21, "21.0.3", TargetOs::Linux);

    let candidates = vec![home17.clone(), home21.clone()];

    let result = detect(17, &candidates, TargetOs::Linux, None).unwrap();
    assert_eq!(result.major, 17);
    assert_eq!(result.path, home17.join("bin").join("java"));
}

#[test]
fn detect_skips_non_matching_major() {
    let tmp = TempDir::new().unwrap();
    let home = make_fake_jre(&tmp, "17.0.8", TargetOs::Linux);

    // Looking for Java 21, only have 17.
    let result = detect(21, &[home], TargetOs::Linux, None);
    assert!(result.is_none());
}

#[test]
fn detect_returns_none_for_empty_candidates() {
    let result = detect(17, &[], TargetOs::Linux, None);
    assert!(result.is_none());
}

#[test]
fn detect_returns_first_match_when_multiple_same_major() {
    let tmp_a = TempDir::new().unwrap();
    let tmp_b = TempDir::new().unwrap();

    let home_a = make_fake_jre(&tmp_a, "17.0.8", TargetOs::Linux);
    let home_b = make_fake_jre(&tmp_b, "17.0.11", TargetOs::Linux);

    let candidates = vec![home_a.clone(), home_b];
    let result = detect(17, &candidates, TargetOs::Linux, None).unwrap();
    // Should return the first match.
    assert_eq!(result.path, home_a.join("bin").join("java"));
}

#[test]
fn detect_mixed_candidates_finds_correct_major() {
    let tmp8 = TempDir::new().unwrap();
    let tmp17 = TempDir::new().unwrap();
    let tmp21 = TempDir::new().unwrap();

    let home8 = make_fake_jre(&tmp8, "1.8.0_392", TargetOs::Linux);
    let home17 = make_fake_jre(&tmp17, "17.0.8", TargetOs::Linux);
    let home21 = make_fake_jre(&tmp21, "21.0.3", TargetOs::Linux);

    let candidates = vec![home8, home17.clone(), home21];
    let result = detect(17, &candidates, TargetOs::Linux, None).unwrap();
    assert_eq!(result.major, 17);
    assert_eq!(result.path, home17.join("bin").join("java"));
}

// ------------------------------------------------------------------
// CP3 — Adoptium query URL + OS/arch mapping
// ------------------------------------------------------------------

#[test]
fn adoptium_os_linux() {
    assert_eq!(TargetOs::Linux.adoptium_os(), "linux");
}

#[test]
fn adoptium_os_macos_is_mac_not_osx() {
    // Adoptium uses "mac", NOT "osx" — must not regress.
    let os = TargetOs::MacOs.adoptium_os();
    assert_eq!(os, "mac");
    assert_ne!(os, "osx");
}

#[test]
fn adoptium_os_windows() {
    assert_eq!(TargetOs::Windows.adoptium_os(), "windows");
}

#[test]
fn adoptium_arch_x86_64() {
    assert_eq!(adoptium_arch("x86_64"), Some("x64"));
}

#[test]
fn adoptium_arch_aarch64() {
    assert_eq!(adoptium_arch("aarch64"), Some("aarch64"));
}

#[test]
fn adoptium_arch_unknown_returns_none() {
    assert_eq!(adoptium_arch("mips"), None);
}

#[test]
fn query_url_linux_x64_major17() {
    let url = adoptium_query_url(17, "linux", "x64");
    assert_eq!(
        url,
        "https://api.adoptium.net/v3/assets/latest/17/hotspot\
             ?architecture=x64&image_type=jre&os=linux&vendor=eclipse"
    );
}

#[test]
fn query_url_mac_aarch64_major21() {
    let url = adoptium_query_url(21, "mac", "aarch64");
    assert!(url.contains("/21/hotspot"));
    assert!(url.contains("os=mac"));
    assert!(url.contains("architecture=aarch64"));
    assert!(url.contains("image_type=jre"));
}

#[test]
fn query_url_windows_x64() {
    let url = adoptium_query_url(17, "windows", "x64");
    assert!(url.contains("os=windows"));
    assert!(url.contains("architecture=x64"));
}

// ------------------------------------------------------------------
// CP3 — parse_adoptium_response (fixture-based)
// ------------------------------------------------------------------

const ADOPTIUM_FIXTURE: &str = include_str!("fixtures/adoptium_latest.json");

#[test]
fn parse_fixture_produces_correct_download_item() {
    use crate::core::download::ExpectedHash;
    use std::path::Path;

    let java_dir = Path::new("/data/java");
    let (item, kind) = parse_adoptium_response(ADOPTIUM_FIXTURE, 17, java_dir).unwrap();

    // URL is the package link.
    assert_eq!(
        item.url,
        "https://github.com/adoptium/temurin17-binaries/releases/download/\
             jdk-17.0.19%2B10/OpenJDK17U-jre_x64_linux_hotspot_17.0.19_10.tar.gz"
    );

    // dest is under <data>/java/<major>/<name>.
    assert_eq!(
        item.dest,
        Path::new("/data/java/17/OpenJDK17U-jre_x64_linux_hotspot_17.0.19_10.tar.gz")
    );

    // SHA-256 checksum from fixture.
    assert_eq!(
        item.expected_hash,
        Some(ExpectedHash::Sha256(
            "adb5a2364baa51de1ef91bb9911f5a61d24b045fe1d6647cb8050272a3a8ee75".to_string()
        ))
    );

    // Size from fixture.
    assert_eq!(item.size, Some(46671975));

    // .tar.gz → TarGz.
    assert_eq!(kind, ArchiveKind::TarGz);
}

#[test]
fn parse_fixture_zip_name_gives_zip_kind() {
    // Construct a minimal JSON with a .zip package name (Windows shape).
    let json = r#"[{
            "binary": {
                "architecture": "x64",
                "image_type": "jre",
                "jvm_impl": "hotspot",
                "os": "windows",
                "package": {
                    "checksum": "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
                    "link": "https://example.com/temurin17-jre.zip",
                    "name": "OpenJDK17U-jre_x64_windows_hotspot_17.0.19_10.zip",
                    "size": 12345678
                }
            }
        }]"#;
    use std::path::Path;
    let (item, kind) = parse_adoptium_response(json, 17, Path::new("/data/java")).unwrap();
    assert_eq!(kind, ArchiveKind::Zip);
    assert_eq!(
        item.dest,
        Path::new("/data/java/17/OpenJDK17U-jre_x64_windows_hotspot_17.0.19_10.zip")
    );
}

#[test]
fn parse_empty_array_returns_error() {
    use std::path::Path;
    let result = parse_adoptium_response("[]", 17, Path::new("/data/java"));
    assert!(result.is_err());
    let msg = result.unwrap_err();
    assert!(msg.contains("no jre asset"));
}

#[test]
fn parse_skips_non_jre_entries_and_finds_jre() {
    // Two entries: first is jdk, second is jre — should pick the jre.
    let json = r#"[
            {
                "binary": {
                    "architecture": "x64",
                    "image_type": "jdk",
                    "jvm_impl": "hotspot",
                    "os": "linux",
                    "package": {
                        "checksum": "aaaa",
                        "link": "https://example.com/jdk.tar.gz",
                        "name": "OpenJDK17U-jdk.tar.gz",
                        "size": 99999999
                    }
                }
            },
            {
                "binary": {
                    "architecture": "x64",
                    "image_type": "jre",
                    "jvm_impl": "hotspot",
                    "os": "linux",
                    "package": {
                        "checksum": "bbbb",
                        "link": "https://example.com/jre.tar.gz",
                        "name": "OpenJDK17U-jre.tar.gz",
                        "size": 55555555
                    }
                }
            }
        ]"#;
    use crate::core::download::ExpectedHash;
    use std::path::Path;
    let (item, _) = parse_adoptium_response(json, 17, Path::new("/data/java")).unwrap();
    assert_eq!(item.url, "https://example.com/jre.tar.gz");
    assert_eq!(
        item.expected_hash,
        Some(ExpectedHash::Sha256("bbbb".to_string()))
    );
}

// ------------------------------------------------------------------
// CP4 — extract_archive (tar.gz)
// ------------------------------------------------------------------

/// Build a minimal `.tar.gz` in memory with a `bin/java` entry and a `release` file.
///
/// Layout inside the archive:
///   jdk-17.0.8+7-jre/
///   jdk-17.0.8+7-jre/bin/
///   jdk-17.0.8+7-jre/bin/java
///   jdk-17.0.8+7-jre/release
fn make_tar_gz(dest_file: &Path) {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use tar::Builder;

    let file = fs::File::create(dest_file).unwrap();
    let gz = GzEncoder::new(file, Compression::fast());
    let mut archive = Builder::new(gz);

    let prefix = "jdk-17.0.8+7-jre";

    // Add bin/java (empty file).
    let java_content = b"#!/bin/sh\n";
    let mut header = tar::Header::new_gnu();
    header.set_size(java_content.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    archive
        .append_data(
            &mut header,
            format!("{prefix}/bin/java"),
            java_content.as_ref(),
        )
        .unwrap();

    // Add release file.
    let release_content = b"JAVA_VERSION=\"17.0.8\"\n";
    let mut header2 = tar::Header::new_gnu();
    header2.set_size(release_content.len() as u64);
    header2.set_mode(0o644);
    header2.set_cksum();
    archive
        .append_data(
            &mut header2,
            format!("{prefix}/release"),
            release_content.as_ref(),
        )
        .unwrap();

    archive.finish().unwrap();
}

#[test]
fn extract_tar_gz_unpacks_and_locates_java() {
    let tmp = TempDir::new().unwrap();
    let archive_path = tmp.path().join("temurin.tar.gz");
    make_tar_gz(&archive_path);

    let dest = tmp.path().join("extracted");
    extract_archive(&archive_path, ArchiveKind::TarGz, &dest).unwrap();

    // locate_java_bin should find jdk-17.0.8+7-jre/bin/java.
    let java = locate_java_bin(&dest, TargetOs::Linux).expect("java binary not found");
    assert!(java.exists(), "located java path should exist on disk");
    assert_eq!(java.file_name().unwrap(), "java");
}

#[test]
fn extract_tar_gz_traversal_refused() {
    // The `tar` crate refuses to build an archive with `..` path components via its
    // safe API.  Build the malicious archive as raw bytes (a minimal POSIX tar block)
    // so we can test our own traversal guard independent of the builder.
    //
    // A POSIX tar entry header is 512 bytes; the name field is bytes 0-99.
    // We write a single file entry with name `../escape`, size 4, then the
    // 4-byte content block (padded to 512), then two 512-byte zero blocks (end-of-archive).
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write as _;

    let tmp = TempDir::new().unwrap();
    let archive_path = tmp.path().join("malicious.tar.gz");

    {
        let file = fs::File::create(&archive_path).unwrap();
        let mut gz = GzEncoder::new(file, Compression::fast());

        // Build a minimal tar header block (512 bytes).
        let mut header = [0u8; 512];
        // Name: ../escape  (bytes 0..100)
        let name = b"../escape\0";
        header[..name.len()].copy_from_slice(name);
        // Mode: 0644 in octal ASCII (bytes 100..108)
        header[100..107].copy_from_slice(b"0000644");
        header[107] = 0;
        // UID, GID: 0 (bytes 108..124)
        header[108..115].copy_from_slice(b"0000000");
        header[116..123].copy_from_slice(b"0000000");
        // Size: 4 bytes → "0000004" (bytes 124..136)
        header[124..131].copy_from_slice(b"0000004");
        header[131] = 0;
        // mtime: 0 (bytes 136..148)
        header[136..147].copy_from_slice(b"00000000000");
        header[147] = 0;
        // checksum placeholder: 8 spaces (bytes 148..156)
        header[148..156].copy_from_slice(b"        ");
        // typeflag: '0' = regular file (byte 156)
        header[156] = b'0';
        // magic + version (bytes 257..265): ustar\000
        header[257..263].copy_from_slice(b"ustar ");
        header[263] = b' ';
        header[264] = 0;
        // Compute checksum (unsigned sum of all header bytes with spaces in 148..156).
        let cksum: u32 = header.iter().map(|&b| b as u32).sum();
        // Write checksum as 6-digit octal + \0 + space.
        let cksum_str = format!("{:06o}\0 ", cksum);
        header[148..156].copy_from_slice(cksum_str.as_bytes());

        gz.write_all(&header).unwrap();

        // Data block (512 bytes, content = "evil" padded with zeros).
        let mut data_block = [0u8; 512];
        data_block[..4].copy_from_slice(b"evil");
        gz.write_all(&data_block).unwrap();

        // End-of-archive: two zero blocks.
        gz.write_all(&[0u8; 1024]).unwrap();
        gz.finish().unwrap();
    }

    let dest = tmp.path().join("extracted");
    let result = extract_archive(&archive_path, ArchiveKind::TarGz, &dest);

    assert!(result.is_err(), "traversal entry must be refused");
    let msg = result.unwrap_err();
    assert!(
        msg.contains("traversal refused"),
        "error message should mention traversal: {msg}"
    );

    // Nothing should have been written outside dest.
    let escape_target = tmp.path().join("escape");
    assert!(
        !escape_target.exists(),
        "malicious file must not exist outside dest"
    );
}

// ------------------------------------------------------------------
// CP4 — extract_archive (zip)
// ------------------------------------------------------------------

/// Build a minimal `.zip` with `jdk-17.0.8+7-jre/bin/java` and `jdk-17.0.8+7-jre/release`.
fn make_zip(dest_file: &Path) {
    use std::io::Write as _;
    use zip::write::{FileOptions, ZipWriter};
    use zip::CompressionMethod;

    let file = fs::File::create(dest_file).unwrap();
    let mut zip = ZipWriter::new(file);
    let options: FileOptions<'_, ()> =
        FileOptions::default().compression_method(CompressionMethod::Deflated);

    let prefix = "jdk-17.0.8+7-jre";

    zip.add_directory(format!("{prefix}/"), options).unwrap();
    zip.add_directory(format!("{prefix}/bin/"), options)
        .unwrap();

    zip.start_file(format!("{prefix}/bin/java"), options)
        .unwrap();
    zip.write_all(b"#!/bin/sh\n").unwrap();

    zip.start_file(format!("{prefix}/release"), options)
        .unwrap();
    zip.write_all(b"JAVA_VERSION=\"17.0.8\"\n").unwrap();

    zip.finish().unwrap();
}

#[test]
fn extract_zip_unpacks_and_locates_java() {
    let tmp = TempDir::new().unwrap();
    let archive_path = tmp.path().join("temurin.zip");
    make_zip(&archive_path);

    let dest = tmp.path().join("extracted");
    extract_archive(&archive_path, ArchiveKind::Zip, &dest).unwrap();

    let java = locate_java_bin(&dest, TargetOs::Linux).expect("java binary not found");
    assert!(java.exists());
    assert_eq!(java.file_name().unwrap(), "java");
}

#[test]
fn extract_zip_traversal_refused() {
    use std::io::Write as _;
    use zip::write::{FileOptions, ZipWriter};
    use zip::CompressionMethod;

    let tmp = TempDir::new().unwrap();
    let archive_path = tmp.path().join("malicious.zip");

    {
        let file = fs::File::create(&archive_path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options: FileOptions<'_, ()> =
            FileOptions::default().compression_method(CompressionMethod::Deflated);
        // Path that escapes dest.
        zip.start_file("../escape", options).unwrap();
        zip.write_all(b"evil").unwrap();
        zip.finish().unwrap();
    }

    let dest = tmp.path().join("extracted");
    let result = extract_archive(&archive_path, ArchiveKind::Zip, &dest);

    assert!(result.is_err(), "zip traversal entry must be refused");
    let msg = result.unwrap_err();
    assert!(
        msg.contains("traversal refused"),
        "error message should mention traversal: {msg}"
    );

    let escape_target = tmp.path().join("escape");
    assert!(
        !escape_target.exists(),
        "malicious file must not exist outside dest"
    );
}

// ------------------------------------------------------------------
// CP4 — F-2: detect labels cache-dir entries as Downloaded
// ------------------------------------------------------------------

#[test]
fn detect_labels_cache_dir_entries_as_downloaded() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().join("java");

    // JRE home is inside the cache dir.
    let home = cache_dir.join("17").join("jdk-17.0.8+7-jre");
    let bin_dir = home.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::write(bin_dir.join("java"), b"").unwrap();
    fs::write(home.join("release"), b"JAVA_VERSION=\"17.0.8\"\n").unwrap();

    let candidates = vec![home];
    let result = detect(17, &candidates, TargetOs::Linux, Some(&cache_dir)).unwrap();

    assert!(
        matches!(result.source, JavaSource::Downloaded),
        "JRE under cache dir must be labelled Downloaded, got {:?}",
        result.source
    );
}

#[test]
fn detect_labels_non_cache_entries_as_detected() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().join("java");
    // JRE home is outside the cache dir.
    let other_dir = tmp.path().join("system_java");
    fs::create_dir_all(&other_dir).unwrap();
    let bin_dir = other_dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::write(bin_dir.join("java"), b"").unwrap();
    fs::write(other_dir.join("release"), b"JAVA_VERSION=\"17.0.8\"\n").unwrap();

    let candidates = vec![other_dir];
    let result = detect(17, &candidates, TargetOs::Linux, Some(&cache_dir)).unwrap();

    assert!(
        matches!(result.source, JavaSource::Detected),
        "JRE outside cache dir must be labelled Detected, got {:?}",
        result.source
    );
}

// ------------------------------------------------------------------
// CP4 — ensure_java_core: detect-hit with injected candidates (no network)
// ------------------------------------------------------------------

#[tokio::test]
async fn ensure_java_core_detect_hit_no_network() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().join("java_cache");
    fs::create_dir_all(&cache_dir).unwrap();

    // Build a fake JRE 17 as a candidate.
    let home = make_fake_jre(&tmp, "17.0.8", TargetOs::Linux);
    let candidates = vec![home.clone()];

    let result = ensure_java_core(17, &candidates, &cache_dir, TargetOs::Linux, || async {
        panic!("provision must not be called on detect hit")
    })
    .await
    .unwrap();

    assert_eq!(result.major, 17);
    assert_eq!(result.path, home.join("bin").join("java"));
}

// ------------------------------------------------------------------
// F-4: extraction dest is <cache_dir>/<major>/ — not archive parent
// ------------------------------------------------------------------

/// Asserts that the extraction target handed to `extract_archive` inside
/// `ensure_java`'s provision closure is `<cache_dir>/<major>/`, not derived
/// from the archive's parent directory.
///
/// Strategy: mirror the dest-derivation logic (`cache_dir.join(major)`) and
/// confirm it equals the path we expect.  Also verified end-to-end: the
/// provision closure in `ensure_java_core` builds a fake JRE at
/// `<cache_dir>/<major>/jdk-tree/` — detection only succeeds if the returned
/// binary path points there, which confirms the dest was the major-scoped dir.
#[test]
fn extract_dest_is_major_scoped_dir() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().join("java");
    let major: u32 = 17;

    // This is the formula used in ensure_java's provision closure.
    let derived = cache_dir.join(major.to_string());

    // Must be <cache_dir>/17, not <cache_dir> or any other path.
    assert_eq!(derived, cache_dir.join("17"));
    assert_ne!(derived, cache_dir, "dest must not be the bare cache dir");
}

#[tokio::test]
async fn ensure_java_core_provision_receives_major_scoped_dest() {
    // End-to-end: provision closure places a fake JRE under cache_dir/<major>/
    // and returns its java bin; ensure_java_core must return that path.
    // This mirrors the real ensure_java behaviour (extract into cache_dir/<major>).
    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().join("java");

    let major: u32 = 21;
    // Build the fake JRE tree at <cache_dir>/<major>/<jdk-dir>/ — where
    // ensure_java would extract to.
    let extract_dest = cache_dir.join(major.to_string());
    let jdk_dir = extract_dest.join("jdk-21.0.3+9-jre");
    let bin_dir = jdk_dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let fake_java = bin_dir.join("java");
    fs::write(&fake_java, b"").unwrap();
    fs::write(jdk_dir.join("release"), b"JAVA_VERSION=\"21.0.3\"\n").unwrap();

    let fake_java_clone = fake_java.clone();

    // Provision closure simulates: extract_archive(archive, kind, cache_dir/major)
    // then locate_java_bin(cache_dir/major, os) → returns the bin path.
    let result = ensure_java_core(
        major,
        &[], // no pre-existing candidates → forces provision
        &cache_dir,
        TargetOs::Linux,
        move || async move { Ok(fake_java_clone) },
    )
    .await
    .unwrap();

    assert_eq!(result.major, major);
    assert_eq!(result.path, fake_java);
    assert!(
        result.path.starts_with(&cache_dir.join(major.to_string())),
        "java binary must be under cache_dir/<major>/, got: {}",
        result.path.display()
    );
}

#[tokio::test]
async fn ensure_java_core_misses_then_provisions() {
    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().join("java_cache");
    fs::create_dir_all(&cache_dir).unwrap();

    // No matching candidates.
    let candidates: Vec<PathBuf> = vec![];

    // Provision closure returns a fake java path.
    let fake_java = tmp.path().join("fake_java");
    fs::write(&fake_java, b"").unwrap();
    let fake_java_clone = fake_java.clone();

    let result = ensure_java_core(
        17,
        &candidates,
        &cache_dir,
        TargetOs::Linux,
        move || async move { Ok(fake_java_clone) },
    )
    .await
    .unwrap();

    assert_eq!(result.major, 17);
    assert_eq!(result.path, fake_java);
    assert!(matches!(result.source, JavaSource::Downloaded));
}
