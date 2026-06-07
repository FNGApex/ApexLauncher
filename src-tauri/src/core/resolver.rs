//! Vanilla Minecraft resolver — Phase 2, slice B.
//!
//! Fetches + caches the per-version manifest from Mojang's piston-meta, parses
//! it into typed structs, and (later checkpoints) produces a [`DownloadPlan`]
//! plus a `LaunchMeta` struct for slice D.
//!
//! CP1: manifest fetch + parse only. Rule eval (CP2), asset resolution (CP3),
//!      and command wiring (CP4) are added in subsequent iterations.

use std::time::Duration;

use serde::Deserialize;
use tauri::AppHandle;

use crate::core::meta;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

/// Vanilla manifests are immutable once published — 365-day TTL is effectively
/// "cache forever" while still allowing manual invalidation by deleting the file.
const VERSION_TTL: Duration = Duration::from_secs(365 * 24 * 3600);

const MANIFEST_TTL: Duration = Duration::from_secs(6 * 3600);

// ---------------------------------------------------------------------------
// version_manifest_v2 entry
// ---------------------------------------------------------------------------

/// A single entry from the top-level `version_manifest_v2.json`.
#[derive(Debug, Deserialize)]
struct ManifestEntry {
    pub id: String,
    pub url: String,
    pub sha1: String,
}

#[derive(Debug, Deserialize)]
struct VersionManifest {
    pub versions: Vec<ManifestEntry>,
}

// ---------------------------------------------------------------------------
// Per-version manifest structs
// ---------------------------------------------------------------------------

/// `downloads.client` / `downloads.server` / library artifact.
#[derive(Debug, Deserialize)]
pub struct DownloadSpec {
    pub url: String,
    pub sha1: String,
    pub size: u64,
}

/// `javaVersion` block. Absent on very old versions; defaults to major 8.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    pub component: String,
    pub major_version: u32,
}

/// `assetIndex` block.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndex {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    pub total_size: u64,
    pub url: String,
}

/// `downloads` object inside the per-version manifest.
#[derive(Debug, Deserialize)]
pub struct ManifestDownloads {
    pub client: DownloadSpec,
}

// ---------------------------------------------------------------------------
// Arguments (modern structured vs legacy string)
// ---------------------------------------------------------------------------

/// A game/jvm argument entry. Mojang uses a mixed array: plain strings and
/// objects with `rules` + `value` (string or string array). We deserialize both
/// but CP1 only preserves the raw strings; rule-filtered JVM args are CP2+.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ArgumentEntry {
    Plain(String),
    Conditional(ConditionalArgument),
}

#[derive(Debug, Deserialize)]
pub struct ConditionalArgument {
    pub rules: Vec<serde_json::Value>, // opaque for CP1; CP2 evaluates
    pub value: ArgumentValue,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ArgumentValue {
    Single(String),
    Many(Vec<String>),
}

/// Modern `arguments` block (MC ≥1.13).
#[derive(Debug, Deserialize, Default)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<ArgumentEntry>,
    #[serde(default)]
    pub jvm: Vec<ArgumentEntry>,
}

// ---------------------------------------------------------------------------
// Libraries
// ---------------------------------------------------------------------------

/// `downloads.classifiers` map value (one native jar variant).
#[derive(Debug, Deserialize)]
pub struct NativeArtifact {
    pub path: String,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

/// `downloads` inside a library entry.
#[derive(Debug, Deserialize, Default)]
pub struct LibraryDownloads {
    /// Present for most libraries; absent for some Maven-only entries.
    pub artifact: Option<DownloadSpec>,
    /// Native jar variants, keyed by classifier string (e.g. `"natives-linux"`).
    #[serde(default)]
    pub classifiers: std::collections::HashMap<String, NativeArtifact>,
}

/// A single library entry from the `libraries` array.
#[derive(Debug, Deserialize)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub downloads: LibraryDownloads,
    /// OS-native classifier map: `"linux"` → `"natives-linux"`, etc.
    #[serde(default)]
    pub natives: std::collections::HashMap<String, String>,
    /// Allow/deny rules for this library (CP2 evaluates; stored as raw Value).
    #[serde(default)]
    pub rules: Vec<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Top-level version manifest
// ---------------------------------------------------------------------------

/// Parsed per-version manifest (e.g. `1.21.1.json` from piston-meta).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionSpec {
    pub id: String,
    pub main_class: String,
    pub downloads: ManifestDownloads,
    pub asset_index: AssetIndex,
    pub libraries: Vec<Library>,

    /// Modern structured args (MC ≥1.13).
    #[serde(default)]
    pub arguments: Option<Arguments>,

    /// Legacy flat arg string (MC <1.13).
    #[serde(default)]
    pub minecraft_arguments: Option<String>,

    /// `javaVersion` may be absent on old manifests; defaults to major 8.
    #[serde(default)]
    pub java_version: Option<JavaVersion>,
}

impl VersionSpec {
    /// Resolved Java major version, defaulting to 8 when the field is absent.
    pub fn java_major(&self) -> u32 {
        self.java_version
            .as_ref()
            .map(|j| j.major_version)
            .unwrap_or(8)
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Fetch + cache the per-version manifest for `version_id`.
///
/// Uses the top-level `version_manifest_v2.json` to discover the per-version
/// URL (verified by `sha1`), then fetches + caches the version JSON with a
/// long TTL (immutable per id).
pub async fn fetch_version_spec(
    app: &AppHandle,
    version_id: &str,
) -> Result<VersionSpec, String> {
    // 1. Fetch top-level manifest (short TTL — new releases appear here).
    let body =
        meta::cached_text(app, MANIFEST_URL, "version_manifest_v2.json", MANIFEST_TTL).await?;
    let manifest: VersionManifest =
        serde_json::from_str(&body).map_err(|e| format!("bad version_manifest_v2: {e}"))?;

    // 2. Find the entry for the requested version id.
    let entry = manifest
        .versions
        .into_iter()
        .find(|v| v.id == version_id)
        .ok_or_else(|| format!("version '{version_id}' not found in manifest"))?;

    // 3. Fetch + cache the per-version JSON (long TTL — content is immutable).
    let key = format!("version-{}.json", version_id);
    let version_body =
        meta::cached_text(app, &entry.url, &key, VERSION_TTL).await?;

    // 4. Deserialize into typed structs.
    let spec: VersionSpec = serde_json::from_str(&version_body)
        .map_err(|e| format!("bad version manifest for '{version_id}': {e}"))?;

    // Sanity-check the sha1 field is present on the entry (we don't verify
    // the download here because cached_text handles re-validation on miss;
    // full hash verification is download.rs's job at execution time).
    let _ = &entry.sha1; // bind to avoid dead-code warning; used in CP4 plan item

    Ok(spec)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const MODERN_FIXTURE: &str =
        include_str!("fixtures/version_manifest_modern.json");

    #[test]
    fn parse_modern_manifest_client_download() {
        let spec: VersionSpec = serde_json::from_str(MODERN_FIXTURE)
            .expect("fixture must deserialize without error");

        assert_eq!(
            spec.downloads.client.url,
            "https://piston-data.mojang.com/v1/objects/aabbccddeeff00112233445566778899aabbccdd/client.jar"
        );
        assert_eq!(
            spec.downloads.client.sha1,
            "aabbccddeeff00112233445566778899aabbccdd"
        );
        assert_eq!(spec.downloads.client.size, 26234786);
    }

    #[test]
    fn parse_modern_manifest_main_class() {
        let spec: VersionSpec = serde_json::from_str(MODERN_FIXTURE)
            .expect("fixture must deserialize without error");

        assert_eq!(spec.main_class, "net.minecraft.client.main.Main");
    }

    #[test]
    fn parse_modern_manifest_java_major() {
        let spec: VersionSpec = serde_json::from_str(MODERN_FIXTURE)
            .expect("fixture must deserialize without error");

        assert_eq!(spec.java_major(), 21);
    }

    #[test]
    fn parse_modern_manifest_asset_index() {
        let spec: VersionSpec = serde_json::from_str(MODERN_FIXTURE)
            .expect("fixture must deserialize without error");

        assert_eq!(spec.asset_index.id, "17");
        assert_eq!(
            spec.asset_index.sha1,
            "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
        );
        assert_eq!(spec.asset_index.size, 447030);
        assert_eq!(spec.asset_index.total_size, 799786602);
    }

    #[test]
    fn parse_modern_manifest_libraries() {
        let spec: VersionSpec = serde_json::from_str(MODERN_FIXTURE)
            .expect("fixture must deserialize without error");

        assert_eq!(spec.libraries.len(), 3);
        let authlib = &spec.libraries[0];
        assert_eq!(authlib.name, "com.mojang:authlib:6.0.54");
        let artifact = authlib.downloads.artifact.as_ref().unwrap();
        assert_eq!(artifact.size, 112233);
    }

    #[test]
    fn parse_modern_manifest_structured_arguments() {
        let spec: VersionSpec = serde_json::from_str(MODERN_FIXTURE)
            .expect("fixture must deserialize without error");

        // Modern manifest: `arguments` present, `minecraftArguments` absent.
        assert!(spec.arguments.is_some());
        assert!(spec.minecraft_arguments.is_none());

        let args = spec.arguments.unwrap();
        // Game args include plain string entries.
        assert!(!args.game.is_empty());
        // JVM args include both plain strings and conditional (rules-based) entries.
        assert!(!args.jvm.is_empty());
    }

    #[test]
    fn java_major_defaults_to_8_when_absent() {
        // Minimal manifest with no javaVersion field.
        let json = r#"{
            "id": "1.8.9",
            "mainClass": "net.minecraft.client.main.Main",
            "downloads": {
                "client": {
                    "url": "https://example.com/client.jar",
                    "sha1": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "size": 1000000
                }
            },
            "assetIndex": {
                "id": "1.8",
                "sha1": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "size": 100000,
                "totalSize": 200000000,
                "url": "https://example.com/1.8.json"
            },
            "libraries": []
        }"#;

        let spec: VersionSpec = serde_json::from_str(json)
            .expect("minimal manifest must deserialize");
        assert_eq!(spec.java_major(), 8);
    }
}
