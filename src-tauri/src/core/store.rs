//! On-disk layout under the cross-platform app data dir (see `docs/ARCHITECTURE.md` §2).
//!
//! Resolved via Tauri's path API so it lands in the right place per OS:
//! - macOS:   `~/Library/Application Support/<identifier>/`
//! - Windows: `%APPDATA%\<identifier>\`
//! - Linux:   `~/.local/share/<identifier>/`

use std::fs;
use std::path::PathBuf;

use tauri::{AppHandle, Manager};

/// Root app data dir. Created on demand by the callers that write into it.
pub fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|e| format!("could not resolve app data dir: {e}"))
}

/// `<data>/instances/`, created if missing.
pub fn instances_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = data_dir(app)?.join("instances");
    fs::create_dir_all(&dir).map_err(|e| format!("could not create instances dir: {e}"))?;
    Ok(dir)
}

/// `<data>/java/`, created if missing.
///
/// Used as the root for downloaded JRE installations and as a detection candidate.
/// Downloaded JREs are stored at `<data>/java/<major>/`.
pub fn java_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = data_dir(app)?.join("java");
    fs::create_dir_all(&dir).map_err(|e| format!("could not create java dir: {e}"))?;
    Ok(dir)
}
