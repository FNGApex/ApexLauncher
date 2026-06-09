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

#[derive(Serialize, Deserialize, Clone)]
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

impl Default for Settings {
    fn default() -> Self {
        Settings {
            schema: SCHEMA_VERSION,
            default_memory_mb: default_memory(),
            default_java_args: default_java_args(),
            curseforge_api_key: None,
            offline_mode: false,
        }
    }
}

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
