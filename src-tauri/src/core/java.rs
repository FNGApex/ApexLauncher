//! Java manager — Phase 2, slice C.
//!
//! CP2: system JRE detection.
//!   - `store::java_dir` — `<data>/java/` (wired here, used by detection + provisioning).
//!   - [`JavaInstallation`] — major version + path to `java`/`java.exe` executable.
//!   - [`parse_major_from_release`] — parses `JAVA_VERSION=` from a JRE `release` file.
//!   - [`probe_installation`] — given a JRE home dir, returns a [`JavaInstallation`] or `None`.
//!   - [`detect`] — takes an injectable candidate list + target OS, returns first match.
//!
//! CP3 (Adoptium provisioning) and CP4 (extraction / `ensure_java` / command) are NOT here yet.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// The OS we are probing (injectable for tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetOs {
    Linux,
    MacOs,
    Windows,
}

impl TargetOs {
    /// Returns the current compile-time OS.
    pub fn current() -> Self {
        #[cfg(target_os = "windows")]
        return TargetOs::Windows;
        #[cfg(target_os = "macos")]
        return TargetOs::MacOs;
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        return TargetOs::Linux;
    }

    /// Name of the java binary on this OS.
    pub fn java_bin(&self) -> &'static str {
        match self {
            TargetOs::Windows => "java.exe",
            _ => "java",
        }
    }
}

/// A located JRE.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaInstallation {
    /// Java major version (e.g. 8, 17, 21).
    pub major: u32,
    /// Absolute path to the `java` / `java.exe` executable.
    pub path: PathBuf,
    /// How this installation was discovered.
    pub source: JavaSource,
}

/// How a [`JavaInstallation`] was found.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JavaSource {
    /// Detected from system paths / environment variables.
    Detected,
    /// Previously downloaded by the launcher.
    Downloaded,
}

// ---------------------------------------------------------------------------
// Release-file parsing
// ---------------------------------------------------------------------------

/// Parse the Java major version from the contents of a JRE `release` file.
///
/// Handles:
/// - Modern: `JAVA_VERSION="17.0.8"` → 17
/// - Legacy:  `JAVA_VERSION="1.8.0_392"` → 8
///
/// Returns `None` if the `JAVA_VERSION` line is absent or unparseable.
pub fn parse_major_from_release(contents: &str) -> Option<u32> {
    for line in contents.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("JAVA_VERSION=") {
            // Strip surrounding quotes if present.
            let ver = rest.trim_matches('"');
            return parse_major_from_version_string(ver);
        }
    }
    None
}

/// Parse a major version number from a version string like `"17.0.8"` or `"1.8.0_392"`.
fn parse_major_from_version_string(ver: &str) -> Option<u32> {
    // Split on '.' and take the first two components.
    let mut parts = ver.split('.');
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        // Legacy scheme: 1.8.0_x → major 8.
        let second: u32 = parts.next()?.parse().ok()?;
        Some(second)
    } else {
        // Modern scheme: 17.0.8 → major 17.
        Some(first)
    }
}

// ---------------------------------------------------------------------------
// Candidate probing
// ---------------------------------------------------------------------------

/// Given a JRE home directory, return a [`JavaInstallation`] if:
/// 1. A `release` file exists and contains a parseable `JAVA_VERSION` line, OR
///    (if no `release` file) a `bin/java[.exe]` is present (major treated as unknown — skip).
/// 2. The `bin/java` (or `bin/java.exe`) executable exists.
///
/// `source` is passed in so callers can distinguish detected vs downloaded installs.
pub fn probe_installation(
    home: &Path,
    os: TargetOs,
    source: JavaSource,
) -> Option<JavaInstallation> {
    let bin = home.join("bin").join(os.java_bin());
    if !bin.exists() {
        return None;
    }

    let release_path = home.join("release");
    let major = if release_path.exists() {
        let contents = fs::read_to_string(&release_path).ok()?;
        parse_major_from_release(&contents)?
    } else {
        // No release file — skip; we can't determine the major without shelling out.
        return None;
    };

    Some(JavaInstallation {
        major,
        path: bin,
        source,
    })
}

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

/// Return the first candidate home directory that satisfies `want_major`, or `None`.
///
/// `candidates` is an ordered list of JRE home directories to probe.
/// `os` controls which binary name is expected (`java` vs `java.exe`).
///
/// Fully injectable — no env reads, no filesystem side-effects beyond reading
/// the candidate dirs themselves — so unit tests can pass fixture dirs.
pub fn detect(want_major: u32, candidates: &[PathBuf], os: TargetOs) -> Option<JavaInstallation> {
    for home in candidates {
        // Downloaded installs under <data>/java/<major>/ are recognised here too —
        // they're just candidate dirs like any other.
        if let Some(inst) = probe_installation(home, os, JavaSource::Detected) {
            if inst.major == want_major {
                return Some(inst);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Candidate gathering helpers (thin wrappers — not under test in CP2)
// ---------------------------------------------------------------------------

/// Expand glob-style parent dirs by listing their direct children.
///
/// Used to turn `/usr/lib/jvm` → `[/usr/lib/jvm/java-17-openjdk-amd64, …]`.
fn expand_dir_children(parent: &Path) -> Vec<PathBuf> {
    match fs::read_dir(parent) {
        Ok(entries) => entries
            .filter_map(|e| e.ok().map(|d| d.path()))
            .collect(),
        Err(_) => vec![],
    }
}

/// Build the default candidate list for the given OS and data dir.
///
/// Order: JAVA_HOME → PATH java-adjacent dirs → common per-OS dirs →
///        `<data>/java/<major>/` download cache entries.
///
/// This function reads env vars and the filesystem — not called in unit tests
/// (tests inject their own candidate list via [`detect`] directly).
pub fn default_candidates(os: TargetOs, data_dir: &Path) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    // 1. JAVA_HOME
    if let Ok(jh) = std::env::var("JAVA_HOME") {
        if !jh.is_empty() {
            candidates.push(PathBuf::from(jh));
        }
    }

    // 2. PATH — find dirs containing `java`/`java.exe`, walk up to the JRE home.
    //    e.g. `/usr/bin/java` → parent = `/usr/bin` → grandparent `/usr` (the home).
    if let Ok(path_env) = std::env::var("PATH") {
        let sep = if os == TargetOs::Windows { ';' } else { ':' };
        for dir in path_env.split(sep) {
            let bin = PathBuf::from(dir).join(os.java_bin());
            if bin.exists() {
                // The bin is in <home>/bin/ — so parent of the dir is the home.
                if let Some(parent) = PathBuf::from(dir).parent() {
                    candidates.push(parent.to_path_buf());
                }
            }
        }
    }

    // 3. Per-OS common install dirs.
    match os {
        TargetOs::Linux => {
            // /usr/lib/jvm/* — each child is a JVM home.
            candidates.extend(expand_dir_children(Path::new("/usr/lib/jvm")));
            // /usr/java/* (some distros).
            candidates.extend(expand_dir_children(Path::new("/usr/java")));
        }
        TargetOs::MacOs => {
            // /Library/Java/JavaVirtualMachines/*/Contents/Home
            let base = Path::new("/Library/Java/JavaVirtualMachines");
            for vm_dir in expand_dir_children(base) {
                candidates.push(vm_dir.join("Contents").join("Home"));
            }
        }
        TargetOs::Windows => {
            // Eclipse Adoptium / Temurin.
            let adoptium = PathBuf::from(r"C:\Program Files\Eclipse Adoptium");
            candidates.extend(expand_dir_children(&adoptium));
            // Oracle / other common locations.
            let oracle = PathBuf::from(r"C:\Program Files\Java");
            candidates.extend(expand_dir_children(&oracle));
        }
    }

    // 4. Previously downloaded installs under <data>/java/<major>/.
    let java_cache = data_dir.join("java");
    candidates.extend(expand_dir_children(&java_cache));

    candidates
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
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

        let result = detect(17, &candidates, TargetOs::Linux).unwrap();
        assert_eq!(result.major, 17);
        assert_eq!(result.path, home17.join("bin").join("java"));
    }

    #[test]
    fn detect_skips_non_matching_major() {
        let tmp = TempDir::new().unwrap();
        let home = make_fake_jre(&tmp, "17.0.8", TargetOs::Linux);

        // Looking for Java 21, only have 17.
        let result = detect(21, &[home], TargetOs::Linux);
        assert!(result.is_none());
    }

    #[test]
    fn detect_returns_none_for_empty_candidates() {
        let result = detect(17, &[], TargetOs::Linux);
        assert!(result.is_none());
    }

    #[test]
    fn detect_returns_first_match_when_multiple_same_major() {
        let tmp_a = TempDir::new().unwrap();
        let tmp_b = TempDir::new().unwrap();

        let home_a = make_fake_jre(&tmp_a, "17.0.8", TargetOs::Linux);
        let home_b = make_fake_jre(&tmp_b, "17.0.11", TargetOs::Linux);

        let candidates = vec![home_a.clone(), home_b];
        let result = detect(17, &candidates, TargetOs::Linux).unwrap();
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
        let result = detect(17, &candidates, TargetOs::Linux).unwrap();
        assert_eq!(result.major, 17);
        assert_eq!(result.path, home17.join("bin").join("java"));
    }
}
