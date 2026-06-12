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
mod tests {
    use super::*;
    use std::path::Path;

    // --- data_root_from_base ---

    #[test]
    fn data_root_ends_with_apex_launcher() {
        let base = Path::new("/home/user/.local/share");
        let root = data_root_from_base(base);
        assert_eq!(
            root.file_name().and_then(|n| n.to_str()),
            Some("ApexLauncher"),
            "last component must be ApexLauncher"
        );
    }

    #[test]
    fn data_root_does_not_contain_reverse_dns_identifier() {
        let base = Path::new("/home/user/.local/share");
        let root = data_root_from_base(base);
        let s = root.to_string_lossy();
        assert!(
            !s.contains("com.bear.modloader"),
            "path must not contain old bundle id: {s}"
        );
        assert!(
            !s.contains("com.apex.apexlauncher"),
            "path must not contain any bundle id: {s}"
        );
    }

    #[test]
    fn data_root_on_macos_shape() {
        let base = Path::new("/Users/bear/Library/Application Support");
        let root = data_root_from_base(base);
        assert_eq!(
            root.to_str(),
            Some("/Users/bear/Library/Application Support/ApexLauncher")
        );
    }

    #[test]
    fn data_root_on_windows_shape() {
        let base = Path::new(r"C:\Users\Bear\AppData\Roaming");
        let root = data_root_from_base(base);
        assert!(
            root.to_str().unwrap().ends_with("ApexLauncher"),
            "Windows path should end with ApexLauncher"
        );
    }

    // --- cache_subdir_path ---

    #[test]
    fn cache_subdir_assets_shape() {
        let root = Path::new("/data/ApexLauncher");
        let p = cache_subdir_path(root, "assets");
        assert_eq!(p.to_str(), Some("/data/ApexLauncher/cache/assets"));
    }

    #[test]
    fn cache_subdir_libraries_shape() {
        let root = Path::new("/data/ApexLauncher");
        let p = cache_subdir_path(root, "libraries");
        assert_eq!(p.to_str(), Some("/data/ApexLauncher/cache/libraries"));
    }

    #[test]
    fn cache_subdir_versions_shape() {
        let root = Path::new("/data/ApexLauncher");
        let p = cache_subdir_path(root, "versions");
        assert_eq!(p.to_str(), Some("/data/ApexLauncher/cache/versions"));
    }

    #[test]
    fn cache_subdir_java_shape() {
        let root = Path::new("/data/ApexLauncher");
        let p = cache_subdir_path(root, "java");
        assert_eq!(p.to_str(), Some("/data/ApexLauncher/cache/java"));
    }

    #[test]
    fn cache_subdir_meta_shape() {
        let root = Path::new("/data/ApexLauncher");
        let p = cache_subdir_path(root, "meta");
        assert_eq!(p.to_str(), Some("/data/ApexLauncher/cache/meta"));
    }

    #[test]
    fn cache_subdir_installers_shape() {
        let root = Path::new("/data/ApexLauncher");
        let p = cache_subdir_path(root, "installers");
        assert_eq!(p.to_str(), Some("/data/ApexLauncher/cache/installers"));
    }

    // --- composed path shape (base → root → cache subdir) ---

    // --- cache_dir path shape (pure helper equivalent) ---

    #[test]
    fn cache_dir_path_shape() {
        // cache_dir delegates to data_root_from_base(base).join("cache").
        // Verify: last component is "cache", parent is the data root.
        let base = Path::new("/home/user/.local/share");
        let root = data_root_from_base(base);
        let cache = root.join("cache");
        assert_eq!(
            cache.file_name().and_then(|n| n.to_str()),
            Some("cache"),
            "last component of cache dir must be 'cache'"
        );
        assert_eq!(
            cache.to_str(),
            Some("/home/user/.local/share/ApexLauncher/cache")
        );
    }

    #[test]
    fn full_path_composition_assets() {
        let base = Path::new("/home/user/.local/share");
        let root = data_root_from_base(base);
        let assets = cache_subdir_path(&root, "assets");
        assert_eq!(
            assets.to_str(),
            Some("/home/user/.local/share/ApexLauncher/cache/assets")
        );
    }

    #[test]
    fn full_path_composition_java() {
        let base = Path::new("/home/user/.local/share");
        let root = data_root_from_base(base);
        let java = cache_subdir_path(&root, "java");
        assert_eq!(
            java.to_str(),
            Some("/home/user/.local/share/ApexLauncher/cache/java")
        );
    }
}
