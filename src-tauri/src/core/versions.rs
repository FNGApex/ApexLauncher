//! Minecraft version list from Mojang's piston-meta manifest.
//!
//! We surface **stable releases only**, from the newest down to the floor `1.7.10`
//! (inclusive). The manifest is newest-first and ordered by release date, so we
//! simply collect releases until we pass the floor.

use std::time::Duration;

use serde::Serialize;
use tauri::AppHandle;

use crate::core::meta;

const MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const FLOOR: &str = "1.7.10";
const TTL: Duration = Duration::from_secs(6 * 3600);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McVersion {
    pub id: String,
    pub release_time: String,
}

/// Stable releases, newest first, down to and including `1.7.10`.
pub async fn list_releases(app: &AppHandle) -> Result<Vec<McVersion>, String> {
    let body = meta::cached_text(app, MANIFEST_URL, "version_manifest_v2.json", TTL).await?;
    let json: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("bad manifest: {e}"))?;
    let arr = json["versions"]
        .as_array()
        .ok_or("manifest missing `versions`")?;

    let mut out = Vec::new();
    for v in arr {
        if v["type"].as_str() != Some("release") {
            continue;
        }
        let id = v["id"].as_str().unwrap_or_default().to_string();
        let at_floor = id == FLOOR;
        out.push(McVersion {
            id,
            release_time: v["releaseTime"].as_str().unwrap_or_default().to_string(),
        });
        if at_floor {
            break; // newest-first → everything past 1.7.10 is older, drop it
        }
    }
    Ok(out)
}
