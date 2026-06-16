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

    // 4. Previously downloaded installs under <data>/cache/java/<major>/.
    let java_cache = data_dir.join("cache").join("java");
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
            fs::create_dir_all(&target_canon)
                .map_err(|e| format!("failed to create dir '{entry_name}': {e}"))?;
        } else {
            if let Some(parent) = target_canon.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create parent for '{entry_name}': {e}"))?;
            }
            let mut out = fs::File::create(&target_canon)
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

            // Extraction dest is always the major-scoped dir, explicit — not
            // derived from the archive's parent (which would silently break if
            // provision_java ever placed the archive elsewhere).
            let extract_dest = cache_dir_clone.join(major.to_string());

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
#[path = "java_tests.rs"]
mod tests;
