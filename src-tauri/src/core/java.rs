//! Java manager — Phase 2, slice C.
//!
//! CP2: system JRE detection.
//!   - `store::java_dir` — `<data>/java/` (wired here, used by detection + provisioning).
//!   - [`JavaInstallation`] — major version + path to `java`/`java.exe` executable.
//!   - [`parse_major_from_release`] — parses `JAVA_VERSION=` from a JRE `release` file.
//!   - [`probe_installation`] — given a JRE home dir, returns a [`JavaInstallation`] or `None`.
//!   - [`detect`] — takes an injectable candidate list + target OS, returns first match.
//!
//! CP3: Adoptium provisioning plan.
//!   - [`ArchiveKind`] — TarGz or Zip, derived from package name extension.
//!   - [`adoptium_query_url`] — builds the Adoptium API URL for a given major/os/arch.
//!   - [`parse_adoptium_response`] — parses the JSON array into a `(DownloadItem, ArchiveKind)`.
//!   - [`provision_java`] — async; fetches, parses, and executes the download plan.
//!
//! CP4: extraction + ensure_java + Tauri command.
//!   - [`extract_archive`] — in-process `.tar.gz`/`.zip` extraction with traversal guard.
//!   - [`locate_java_bin`] — walks extracted dir tree for `bin/java[.exe]`.
//!   - [`ensure_java_core`] — injectable detect-or-provision core; testable without network.
//!   - [`ensure_java`] — thin `AppHandle` wrapper over `ensure_java_core`.

use std::fs;
use std::io;
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
/// `cache_prefix` — if a candidate's path starts with this prefix it is labelled
/// [`JavaSource::Downloaded`]; all others are [`JavaSource::Detected`].
///
/// Fully injectable — no env reads, no filesystem side-effects beyond reading
/// the candidate dirs themselves — so unit tests can pass fixture dirs.
pub fn detect(
    want_major: u32,
    candidates: &[PathBuf],
    os: TargetOs,
    cache_prefix: Option<&Path>,
) -> Option<JavaInstallation> {
    for home in candidates {
        // F-2 fix: JREs under the launcher's own cache dir are labelled Downloaded,
        // not Detected — they were put there by the launcher, not found on the system.
        let source = match cache_prefix {
            Some(prefix) if home.starts_with(prefix) => JavaSource::Downloaded,
            _ => JavaSource::Detected,
        };
        if let Some(inst) = probe_installation(home, os, source) {
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
    //    Assumption: the binary sits in `<home>/bin/java[.exe]`, so ascending one level
    //    from the PATH entry gives the JRE home.  This covers the common case of
    //    `/usr/bin/java` (symlink aside) and `<home>/bin` entries added by sdkman/jabba.
    //    It does NOT cover wrappers that live directly in a PATH dir without a `bin/`
    //    subdirectory (e.g. some `/usr/bin/java` OS wrapper symlinks point into
    //    `/etc/alternatives` rather than a JRE home).  Those cases fall through to the
    //    per-OS dir scan below.
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
// CP3 — Adoptium provisioning plan
// ---------------------------------------------------------------------------

/// The archive format of a downloaded Temurin package.
///
/// Derived from the package filename extension: `.tar.gz` → [`TarGz`], `.zip` → [`Zip`].
/// CP4 extraction consumes this to pick the right unpacker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    TarGz,
    Zip,
}

// ---------------------------------------------------------------------------
// OS / arch mapping
// ---------------------------------------------------------------------------

impl TargetOs {
    /// Maps our [`TargetOs`] to the Adoptium API `os` parameter.
    ///
    /// Adoptium uses `linux`, `mac` (NOT `osx`), `windows`.
    pub fn adoptium_os(&self) -> &'static str {
        match self {
            TargetOs::Linux => "linux",
            // Adoptium uses "mac", NOT "osx" (gotcha — matches Mojang's naming).
            TargetOs::MacOs => "mac",
            TargetOs::Windows => "windows",
        }
    }
}

/// Maps `std::env::consts::ARCH` values to the Adoptium `architecture` parameter.
///
/// Returns `None` for unsupported architectures.
pub fn adoptium_arch(arch: &str) -> Option<&'static str> {
    match arch {
        "x86_64" => Some("x64"),
        "aarch64" => Some("aarch64"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Query URL builder
// ---------------------------------------------------------------------------

/// Build the Adoptium `/v3/assets/latest/<major>/hotspot` query URL.
///
/// Pure function — no I/O. Testable without network.
///
/// `os` should be the Adoptium os string (e.g. `"linux"`, `"mac"`, `"windows"`).
/// `arch` should be the Adoptium arch string (e.g. `"x64"`, `"aarch64"`).
pub fn adoptium_query_url(major: u32, os: &str, arch: &str) -> String {
    format!(
        "https://api.adoptium.net/v3/assets/latest/{major}/hotspot\
         ?architecture={arch}&image_type=jre&os={os}&vendor=eclipse"
    )
}

// ---------------------------------------------------------------------------
// Adoptium JSON response shape
// ---------------------------------------------------------------------------

/// Top-level asset entry from `/v3/assets/latest`.
#[derive(Debug, Deserialize)]
struct AdoptiumAsset {
    binary: AdoptiumBinary,
}

#[derive(Debug, Deserialize)]
struct AdoptiumBinary {
    // F-3: `os` and `architecture` were deserialized but never read; dropped to
    // eliminate dead-code noise. serde ignores unknown JSON fields by default.
    image_type: String,
    package: AdoptiumPackage,
}

#[derive(Debug, Deserialize)]
struct AdoptiumPackage {
    /// Direct download URL.
    link: String,
    /// SHA-256 hex digest.
    checksum: String,
    /// Archive filename, e.g. `OpenJDK17U-jre_x64_linux_hotspot_17.0.19_10.tar.gz`.
    name: String,
    /// Uncompressed size in bytes.
    size: u64,
}

// ---------------------------------------------------------------------------
// Response parser
// ---------------------------------------------------------------------------

/// Parse the Adoptium `/v3/assets/latest` JSON array into a `(DownloadItem, ArchiveKind)`.
///
/// `data_dir_java` — the `<data>/java/` base path; the file lands at
/// `<data>/java/<major>/<package.name>`.
///
/// Selects the first asset whose `image_type` is `"jre"`. Returns an error if the
/// array is empty or contains no `jre` entry.
pub fn parse_adoptium_response(
    json: &str,
    major: u32,
    data_dir_java: &Path,
) -> Result<(crate::core::download::DownloadItem, ArchiveKind), String> {
    use crate::core::download::{DownloadItem, ExpectedHash};

    let assets: Vec<AdoptiumAsset> =
        serde_json::from_str(json).map_err(|e| format!("adoptium JSON parse error: {e}"))?;

    // Prefer an asset whose image_type is "jre".
    let asset = assets
        .into_iter()
        .find(|a| a.binary.image_type == "jre")
        .ok_or_else(|| {
            format!("no jre asset found in Adoptium response for major {major}")
        })?;

    let pkg = asset.binary.package;

    // Derive archive kind from filename extension.
    let kind = if pkg.name.ends_with(".tar.gz") {
        ArchiveKind::TarGz
    } else if pkg.name.ends_with(".zip") {
        ArchiveKind::Zip
    } else {
        return Err(format!(
            "unrecognised archive extension for package '{}'",
            pkg.name
        ));
    };

    let dest = data_dir_java.join(major.to_string()).join(&pkg.name);

    let item = DownloadItem {
        url: pkg.link,
        dest,
        expected_hash: Some(ExpectedHash::Sha256(pkg.checksum)),
        size: Some(pkg.size),
    };

    Ok((item, kind))
}

// ---------------------------------------------------------------------------
// Async provision orchestration
// ---------------------------------------------------------------------------

/// Fetch the Adoptium metadata, build a single-item [`DownloadPlan`], and execute it
/// through the download engine.
///
/// This function issues a real HTTP request and is not called in unit tests.
/// The pure sub-functions (`adoptium_query_url`, `parse_adoptium_response`) are
/// tested via fixture instead.
///
/// `data_dir_java` — `<data>/java/` base path (see `store::java_dir`).
/// `os` / `arch` — Adoptium-style strings (use `TargetOs::adoptium_os()` +
/// `adoptium_arch(std::env::consts::ARCH)`).
pub async fn provision_java(
    major: u32,
    os: &str,
    arch: &str,
    data_dir_java: &Path,
    sink: &(impl crate::core::download::ProgressSink + Sync),
) -> Result<(PathBuf, ArchiveKind), String> {
    use crate::core::download::{execute_plan, DownloadPlan};

    let url = adoptium_query_url(major, os, arch);

    let client = reqwest::Client::builder()
        .user_agent(concat!(
            "modloader/",
            env!("CARGO_PKG_VERSION"),
            " (https://github.com/; minecraft launcher)"
        ))
        .build()
        .map_err(|e| format!("failed to build HTTP client: {e}"))?;

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("adoptium request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("adoptium returned {}: {url}", resp.status()));
    }

    let json = resp
        .text()
        .await
        .map_err(|e| format!("failed to read adoptium response: {e}"))?;

    let (item, kind) = parse_adoptium_response(&json, major, data_dir_java)?;
    let dest = item.dest.clone();

    let plan = DownloadPlan::new(vec![item]);
    let result = execute_plan(&client, &plan, sink, 1).await;

    // execute_plan runs all items and collects outcomes; check for failures.
    for outcome in &result.outcomes {
        if let crate::core::download::ItemStatus::Failed { error } = &outcome.status {
            return Err(format!("download failed for {}: {error}", outcome.url));
        }
    }

    Ok((dest, kind))
}

// ---------------------------------------------------------------------------
// CP4 — Archive extraction (traversal-safe)
// ---------------------------------------------------------------------------

/// Extract a `.tar.gz` or `.zip` archive into `dest`.
///
/// Every entry is checked: if its resolved path would escape `dest` (zip-slip /
/// `../` attack), the entry is refused and an error is returned — nothing is
/// written to disk beyond that point.
pub fn extract_archive(archive_path: &Path, kind: ArchiveKind, dest: &Path) -> Result<(), String> {
    // Canonicalize dest so we get a consistent prefix for the escape check.
    // dest may not exist yet — create it first.
    fs::create_dir_all(dest)
        .map_err(|e| format!("failed to create dest dir {}: {e}", dest.display()))?;
    let dest_canon = dest
        .canonicalize()
        .map_err(|e| format!("failed to canonicalize dest {}: {e}", dest.display()))?;

    match kind {
        ArchiveKind::TarGz => extract_tar_gz(archive_path, &dest_canon),
        ArchiveKind::Zip => extract_zip(archive_path, &dest_canon),
    }
}

fn extract_tar_gz(archive_path: &Path, dest_canon: &Path) -> Result<(), String> {
    use flate2::read::GzDecoder;
    use tar::Archive;

    let file = fs::File::open(archive_path)
        .map_err(|e| format!("failed to open archive {}: {e}", archive_path.display()))?;
    let gz = GzDecoder::new(io::BufReader::new(file));
    let mut archive = Archive::new(gz);
    // Do not preserve ownership on platforms where we can't (Windows) or don't
    // need to — avoids permission errors.
    archive.set_preserve_permissions(false);
    archive.set_preserve_mtime(false);

    for entry in archive
        .entries()
        .map_err(|e| format!("failed to read tar entries: {e}"))?
    {
        let mut entry =
            entry.map_err(|e| format!("failed to read tar entry: {e}"))?;

        // Read the path — the tar crate may reject `..` components itself; if
        // so, treat it as a traversal attempt just like our own check would.
        let entry_path = match entry.path() {
            Ok(p) => p.into_owned(),
            Err(_) => {
                return Err(
                    "traversal refused: tar entry has an unsafe path component".to_string(),
                );
            }
        };

        // Explicit traversal guard: normalize and prefix-check before any write.
        let target = dest_canon.join(&entry_path);
        let target_canon = normalize_path(&target);
        if !target_canon.starts_with(dest_canon) {
            return Err(format!(
                "traversal refused: entry '{}' would escape dest",
                entry_path.display()
            ));
        }

        // `unpack_in` unpacks the entry relative to `dest_canon`, handling
        // platform path differences (including Windows extended-length paths).
        entry
            .unpack_in(dest_canon)
            .map_err(|e| format!("failed to unpack '{}': {e}", entry_path.display()))?;
    }
    Ok(())
}

fn extract_zip(archive_path: &Path, dest_canon: &Path) -> Result<(), String> {
    use zip::ZipArchive;

    let file = fs::File::open(archive_path)
        .map_err(|e| format!("failed to open archive {}: {e}", archive_path.display()))?;
    let mut zip =
        ZipArchive::new(io::BufReader::new(file))
            .map_err(|e| format!("failed to read zip archive: {e}"))?;

    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| format!("failed to read zip entry {i}: {e}"))?;

        let entry_name = entry.name().to_owned();

        let target = dest_canon.join(&entry_name);
        let target_canon = normalize_path(&target);
        if !target_canon.starts_with(dest_canon) {
            return Err(format!(
                "traversal refused: entry '{entry_name}' would escape dest"
            ));
        }

        if entry_name.ends_with('/') {
            // Directory entry.
            fs::create_dir_all(&target)
                .map_err(|e| format!("failed to create dir '{entry_name}': {e}"))?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create parent for '{entry_name}': {e}"))?;
            }
            let mut out = fs::File::create(&target)
                .map_err(|e| format!("failed to create file '{entry_name}': {e}"))?;
            io::copy(&mut entry, &mut out)
                .map_err(|e| format!("failed to write '{entry_name}': {e}"))?;
        }
    }
    Ok(())
}

/// Normalize a path without requiring the path to exist on disk.
///
/// Resolves `..` and `.` components lexically.  Used for the traversal check
/// before writing entries (we can't `canonicalize()` a path that doesn't exist yet).
fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// CP4 — Locate java binary after extraction
// ---------------------------------------------------------------------------

/// Walk `search_root` recursively for `bin/java` (Unix) or `bin/java.exe` (Windows).
///
/// Temurin archives nest under a versioned top dir (e.g. `jdk-17.0.8+7-jre/bin/java`);
/// we do not assume a fixed depth — we find the first match anywhere under the tree.
pub fn locate_java_bin(search_root: &Path, os: TargetOs) -> Option<PathBuf> {
    locate_java_recursive(search_root, os.java_bin())
}

fn locate_java_recursive(dir: &Path, bin_name: &str) -> Option<PathBuf> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return None,
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            // If this directory is named "bin", look for the binary directly inside.
            if path.file_name().and_then(|n| n.to_str()) == Some("bin") {
                let candidate = path.join(bin_name);
                if candidate.exists() {
                    return Some(candidate);
                }
            }
            // Recurse.
            if let Some(found) = locate_java_recursive(&path, bin_name) {
                return Some(found);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// CP4 — ensure_java (injectable core + thin AppHandle wrapper)
// ---------------------------------------------------------------------------

/// Pure/injectable core: detect-or-provision a JRE matching `want_major`.
///
/// Arguments:
/// - `want_major` — the Java major required.
/// - `candidates` — ordered list of JRE home dirs to probe (injected; avoids env reads in tests).
/// - `cache_dir`  — the `<data>/java/` directory; entries under it are labelled `Downloaded`.
/// - `os`         — target OS (injected for tests).
/// - `provision`  — async closure called on cache miss; should download + extract and return
///                  the path to `bin/java[.exe]`.  Injected so tests can skip network.
///
/// On detect-hit returns immediately (no network). On miss, calls `provision` then
/// attempts to locate the binary in `cache_dir/<major>/`.
pub async fn ensure_java_core<F, Fut>(
    want_major: u32,
    candidates: &[PathBuf],
    cache_dir: &Path,
    os: TargetOs,
    provision: F,
) -> Result<JavaInstallation, String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<PathBuf, String>>,
{
    // Detect phase — check injected candidates.
    if let Some(inst) = detect(want_major, candidates, os, Some(cache_dir)) {
        return Ok(inst);
    }

    // Provision phase — download + extract via injected closure.
    let java_bin = provision().await?;

    Ok(JavaInstallation {
        major: want_major,
        path: java_bin,
        source: JavaSource::Downloaded,
    })
}

/// Thin `AppHandle` wrapper: gather real candidates + cache dir, then call [`ensure_java_core`].
///
/// This function issues real network requests (via `provision_java`) and is not called in
/// unit tests — tests use `ensure_java_core` with an injected provision closure instead.
pub async fn ensure_java(
    app: &tauri::AppHandle,
    major: u32,
) -> Result<JavaInstallation, String> {
    use crate::core::download::NoOpSink;

    let data_dir = crate::core::store::data_dir(app)?;
    let cache_dir = crate::core::store::java_dir(app)?;
    let os = TargetOs::current();

    let arch = adoptium_arch(std::env::consts::ARCH)
        .ok_or_else(|| format!("unsupported architecture: {}", std::env::consts::ARCH))?;
    let os_str = os.adoptium_os();

    let candidates = default_candidates(os, &data_dir);
    let cache_dir_clone = cache_dir.clone();
    let os_str_owned = os_str.to_owned();
    let arch_owned = arch.to_owned();

    ensure_java_core(
        major,
        &candidates,
        &cache_dir,
        os,
        || async move {
            let (archive_path, kind) =
                provision_java(major, &os_str_owned, &arch_owned, &cache_dir_clone, &NoOpSink)
                    .await?;

            let extract_dest = archive_path
                .parent()
                .ok_or_else(|| "archive has no parent dir".to_string())?
                .to_path_buf();

            extract_archive(&archive_path, kind, &extract_dest)?;

            locate_java_bin(&extract_dest, os)
                .ok_or_else(|| "java binary not found after extraction".to_string())
        },
    )
    .await
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

    const ADOPTIUM_FIXTURE: &str =
        include_str!("fixtures/adoptium_latest.json");

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
        assert_eq!(item.expected_hash, Some(ExpectedHash::Sha256("bbbb".to_string())));
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
        zip.add_directory(format!("{prefix}/bin/"), options).unwrap();

        zip.start_file(format!("{prefix}/bin/java"), options).unwrap();
        zip.write_all(b"#!/bin/sh\n").unwrap();

        zip.start_file(format!("{prefix}/release"), options).unwrap();
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

        let result = ensure_java_core(
            17,
            &candidates,
            &cache_dir,
            TargetOs::Linux,
            || async { panic!("provision must not be called on detect hit") },
        )
        .await
        .unwrap();

        assert_eq!(result.major, 17);
        assert_eq!(result.path, home.join("bin").join("java"));
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
}
