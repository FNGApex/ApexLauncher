//! Small HTTP helper with a TTL'd disk cache under `<data>/meta-cache/`.
//!
//! Used by the version/loader metadata fetchers so we don't hammer Mojang/Forge/
//! Fabric/etc. on every dropdown change. Responses are cached verbatim by key.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use tauri::AppHandle;

use crate::core::store;

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!(
            "modloader/",
            env!("CARGO_PKG_VERSION"),
            " (https://github.com/; minecraft launcher)"
        ))
        .build()
        .expect("failed to build HTTP client")
}

fn cache_path(app: &AppHandle, key: &str) -> Result<PathBuf, String> {
    let dir = store::data_dir(app)?.join("meta-cache");
    fs::create_dir_all(&dir).map_err(|e| format!("could not create meta-cache dir: {e}"))?;
    Ok(dir.join(key))
}

/// GET `url` as text, served from `meta-cache/<key>` if that file is younger than `ttl`.
/// On a successful fetch the body is written back to the cache.
pub async fn cached_text(
    app: &AppHandle,
    url: &str,
    key: &str,
    ttl: Duration,
) -> Result<String, String> {
    let path = cache_path(app, key)?;

    if let Ok(meta) = fs::metadata(&path) {
        let fresh = meta
            .modified()
            .ok()
            .and_then(|m| SystemTime::now().duration_since(m).ok())
            .is_some_and(|age| age < ttl);
        if fresh {
            if let Ok(body) = fs::read_to_string(&path) {
                return Ok(body);
            }
        }
    }

    let resp = client()
        .get(url)
        .send()
        .await
        .map_err(|e| format!("request to {url} failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("{url} returned {}", resp.status()));
    }
    let body = resp.text().await.map_err(|e| e.to_string())?;
    let _ = fs::write(&path, &body); // cache is best-effort
    Ok(body)
}
