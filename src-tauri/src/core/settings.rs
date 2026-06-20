//! Global launcher settings, persisted to `<data>/settings.json` (Phase 1).
//!
//! Holds cross-instance defaults (memory, JVM args) and the CurseForge API key
//! (used from Phase 5). Missing/blank fields fall back to defaults so an older or
//! partial file still loads.

use std::fs;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::core::store;

/// Bumped when the on-disk settings shape changes (for future migrations).
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Clone, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default = "default_schema")]
    pub schema: u32,
    /// RAM applied to newly created instances (MB).
    #[serde(default = "default_memory")]
    pub default_memory_mb: u32,
    /// Extra JVM args applied to instances with no per-instance override.
    #[serde(default = "default_java_args")]
    pub default_java_args: String,
    /// CurseForge API key (`x-api-key`). Backend-only — never sent to the webview
    /// except to display/edit here. `None` until the user sets one.
    #[serde(default)]
    pub curseforge_api_key: Option<String>,
    /// When true, always use the offline identity at launch regardless of whether
    /// an active account is set. Default: false (use real account when available).
    /// Preserves existing offline behavior for users with no account configured.
    #[serde(default = "default_offline_mode")]
    pub offline_mode: bool,
    /// When true, the sidebar starts collapsed on first launch (before the user has
    /// manually toggled it). The live collapsed state is persisted in localStorage
    /// (`apex-ui`); this setting is the initial seed applied on first run only.
    #[serde(default = "default_sidebar_collapsed")]
    pub sidebar_start_collapsed: bool,
    /// When true, `ensure_java` is called at launch to auto-download a JRE if none
    /// is detected. When false, only detection is attempted; if no JRE is found the
    /// launch is aborted with a message directing the user to configure Java manually.
    /// Default: true (preserves existing behavior).
    #[serde(default = "default_auto_download_java")]
    pub auto_download_java: bool,
    /// When true, the instance console panel starts expanded. When false, it starts
    /// collapsed and the user must click to reveal it. Default: false.
    #[serde(default = "default_show_console_default")]
    pub show_console_default: bool,
    /// When true (default), the launcher window stays open after launching an
    /// instance. When false, the window is minimized immediately after a successful
    /// launch (not closed — closing would kill run monitoring).
    #[serde(default = "default_keep_launcher_open")]
    pub keep_launcher_open: bool,
    /// When true (default), the launcher window is maximized on startup. Replaces
    /// the static `"maximized": true` in `tauri.conf.json` with a dynamic check.
    #[serde(default = "default_maximize_on_start")]
    pub maximize_on_start: bool,
}

fn default_schema() -> u32 {
    SCHEMA_VERSION
}
fn default_memory() -> u32 {
    4096
}
fn default_java_args() -> String {
    "-XX:+UseG1GC".to_string()
}
fn default_offline_mode() -> bool {
    false
}
fn default_sidebar_collapsed() -> bool {
    false
}
fn default_auto_download_java() -> bool {
    true
}
fn default_show_console_default() -> bool {
    false
}
fn default_keep_launcher_open() -> bool {
    true
}
fn default_maximize_on_start() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            schema: SCHEMA_VERSION,
            default_memory_mb: default_memory(),
            default_java_args: default_java_args(),
            curseforge_api_key: None,
            offline_mode: false,
            sidebar_start_collapsed: false,
            auto_download_java: true,
            show_console_default: false,
            keep_launcher_open: true,
            maximize_on_start: true,
        }
    }
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;

/// Load settings, returning defaults if the file doesn't exist yet.
pub fn load(app: &AppHandle) -> Result<Settings, String> {
    let path = store::data_dir(app)?.join("settings.json");
    if !path.is_file() {
        return Ok(Settings::default());
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| format!("malformed settings.json: {e}"))
}

/// Persist settings (normalizing a blank API key to `None`), returning the saved value.
pub fn save(app: &AppHandle, mut settings: Settings) -> Result<Settings, String> {
    settings.schema = SCHEMA_VERSION;
    if settings
        .curseforge_api_key
        .as_ref()
        .is_some_and(|k| k.trim().is_empty())
    {
        settings.curseforge_api_key = None;
    }

    let dir = store::data_dir(app)?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    fs::write(dir.join("settings.json"), raw)
        .map_err(|e| format!("could not write settings.json: {e}"))?;
    Ok(settings)
}
