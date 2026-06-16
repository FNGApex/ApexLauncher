//! On-disk layout under `<OS-appdata-base>/ApexLauncher/`.
//!
//! Resolved via Tauri's path API so it lands in the right place per OS:
//! - macOS:   `~/Library/Application Support/ApexLauncher/`
//! - Windows: `%APPDATA%\ApexLauncher\`
//! - Linux:   `~/.local/share/ApexLauncher/`
//!
//! The root is independent of the bundle identifier (`com.bear.modloader` / `com.apex.apexlauncher`).
//! It uses `app.path().data_dir()` — the OS-level base — joined with `"ApexLauncher"`.

use std::fs;
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

// ---------------------------------------------------------------------------
// Pure helpers (testable without AppHandle)
// ---------------------------------------------------------------------------

/// Joins `base` with `"ApexLauncher"` to produce the data root.
///
/// This is the pure, unit-testable core of [`data_dir`]. Pass `app.path().data_dir()`.
pub fn data_root_from_base(base: &Path) -> PathBuf {
    base.join("ApexLauncher")
}

/// Joins `root` with `"cache"` and `sub` to produce a cache subdirectory path.
///
/// Pure; does not create anything on disk.
pub fn cache_subdir_path(root: &Path, sub: &str) -> PathBuf {
    root.join("cache").join(sub)
}

// ---------------------------------------------------------------------------
// Tauri-backed path helpers
// ---------------------------------------------------------------------------

/// Root app data dir: `<OS-appdata-base>/ApexLauncher/`.
///
/// NOT created by this function; callers that write into it are responsible for
/// creating it (or call a subdir helper which calls `create_dir_all`).
pub fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .data_dir()
        .map(|base| data_root_from_base(&base))
        .map_err(|e| format!("could not resolve app data dir: {e}"))
}

/// `<data>/instances/`, created if missing.
pub fn instances_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = data_dir(app)?.join("instances");
    fs::create_dir_all(&dir).map_err(|e| format!("could not create instances dir: {e}"))?;
    Ok(dir)
}

/// `<data>/cache/`, created if missing.
pub fn cache_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = data_dir(app)?.join("cache");
    fs::create_dir_all(&dir).map_err(|e| format!("could not create cache dir: {e}"))?;
    Ok(dir)
}

/// `<data>/cache/assets/`, created if missing.
pub fn cache_assets_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = cache_subdir_path(&data_dir(app)?, "assets");
    fs::create_dir_all(&dir).map_err(|e| format!("could not create cache/assets dir: {e}"))?;
    Ok(dir)
}

/// `<data>/cache/libraries/`, created if missing.
pub fn cache_libraries_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = cache_subdir_path(&data_dir(app)?, "libraries");
    fs::create_dir_all(&dir).map_err(|e| format!("could not create cache/libraries dir: {e}"))?;
    Ok(dir)
}

/// `<data>/cache/versions/`, created if missing.
pub fn cache_versions_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = cache_subdir_path(&data_dir(app)?, "versions");
    fs::create_dir_all(&dir).map_err(|e| format!("could not create cache/versions dir: {e}"))?;
    Ok(dir)
}

/// `<data>/cache/java/`, created if missing.
///
/// Used as the root for downloaded JRE installations.
/// Downloaded JREs are stored at `<data>/cache/java/<major>/`.
pub fn cache_java_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = cache_subdir_path(&data_dir(app)?, "java");
    fs::create_dir_all(&dir).map_err(|e| format!("could not create cache/java dir: {e}"))?;
    Ok(dir)
}

/// `<data>/cache/meta/`, created if missing.
pub fn cache_meta_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = cache_subdir_path(&data_dir(app)?, "meta");
    fs::create_dir_all(&dir).map_err(|e| format!("could not create cache/meta dir: {e}"))?;
    Ok(dir)
}

/// `<data>/cache/installers/`, created if missing.
pub fn cache_installers_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = cache_subdir_path(&data_dir(app)?, "installers");
    fs::create_dir_all(&dir)
        .map_err(|e| format!("could not create cache/installers dir: {e}"))?;
    Ok(dir)
}

/// `<data>/java/`, created if missing.
///
/// Compatibility alias for [`cache_java_dir`].
pub fn java_dir(app: &AppHandle) -> Result<PathBuf, String> {
    cache_java_dir(app)
}

/// `<data>/account.json` — the single-account metadata store.
///
/// Ensures the parent data dir exists; does NOT create the file itself (it is
/// created lazily on the first write).
pub fn account_file(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = data_dir(app)?;
    fs::create_dir_all(&dir).map_err(|e| format!("could not create app data dir: {e}"))?;
    Ok(dir.join("account.json"))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
