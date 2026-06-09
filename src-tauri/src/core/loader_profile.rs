//! Fabric / Quilt loader profile fetcher and parser.
//!
//! The loader profile is a Mojang launcher-profile JSON that describes:
//! - `mainClass` — overrides the vanilla entry point (KnotClient for Fabric/Quilt).
//! - `libraries` — additional JARs needed on the classpath (Maven coordinates + base URL).
//! - `arguments` — optional extra JVM/game args in the same modern format as the vanilla manifest.
//!
//! `fetch_profile` caches the response via `meta::cached_text` with a 6-hour TTL, matching the
//! TTL used by `loaders.rs` for the version list.

use std::time::Duration;

use serde::Deserialize;
use tauri::AppHandle;

use crate::core::meta;
use crate::core::resolver::Arguments;

const TTL: Duration = Duration::from_secs(6 * 3600);

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// A library entry in the loader profile. Each entry has a Maven coordinate
/// (`name`) and a base Maven repository URL (`url`).
#[derive(Debug, Deserialize, PartialEq)]
pub struct LoaderLibrary {
    pub name: String,
    pub url: String,
}

/// Parsed representation of a Fabric / Quilt loader profile JSON.
///
/// Serde-tolerant: unknown fields are ignored, all optional blocks default to
/// their zero values so the struct parses even if the API ever omits a section.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderProfile {
    pub main_class: String,
    #[serde(default)]
    pub libraries: Vec<LoaderLibrary>,
    #[serde(default)]
    pub arguments: Arguments,
}

// ---------------------------------------------------------------------------
// URL builder (pure — unit-testable without network)
// ---------------------------------------------------------------------------

/// Build the profile JSON URL for the given loader kind, MC version, and loader version.
///
/// Returns `Err` for unknown loader kinds.
pub fn profile_url(kind: &str, mc: &str, loader: &str) -> Result<String, String> {
    match kind {
        "fabric" => Ok(format!(
            "https://meta.fabricmc.net/v2/versions/loader/{mc}/{loader}/profile/json"
        )),
        "quilt" => Ok(format!(
            "https://meta.quiltmc.org/v3/versions/loader/{mc}/{loader}/profile/json"
        )),
        other => Err(format!("unknown loader kind: {other}")),
    }
}

// ---------------------------------------------------------------------------
// Fetch + cache
// ---------------------------------------------------------------------------

/// Fetch and cache a loader profile for `kind` (fabric|quilt), `mc`, and `loader_version`.
///
/// The cache key is `<kind>-profile-<sanitized-mc>-<sanitized-loader>.json`, stored under
/// the app's `meta-cache/` directory.
pub async fn fetch_profile(
    app: &AppHandle,
    kind: &str,
    mc: &str,
    loader_version: &str,
) -> Result<LoaderProfile, String> {
    let url = profile_url(kind, mc, loader_version)?;
    let key = format!(
        "{}-profile-{}-{}.json",
        kind,
        sanitize(mc),
        sanitize(loader_version),
    );
    let body = meta::cached_text(app, &url, &key, TTL).await?;
    serde_json::from_str(&body).map_err(|e| format!("loader profile parse error: {e}"))
}

// ---------------------------------------------------------------------------
// Maven coordinate → relative path
// ---------------------------------------------------------------------------

/// Convert a Maven coordinate (`group:artifact:version` or
/// `group:artifact:version:classifier`) to the relative Maven repository path.
///
/// `"net.fabricmc:fabric-loader:0.16.10"` →
/// `"net/fabricmc/fabric-loader/0.16.10/fabric-loader-0.16.10.jar"`
///
/// `"a.b:c:1.0:natives"` → `"a/b/c/1.0/c-1.0-natives.jar"`
pub fn maven_coord_to_path(coord: &str) -> String {
    let parts: Vec<&str> = coord.splitn(4, ':').collect();
    // parts: [group, artifact, version, (optional classifier)]
    let group = parts.first().copied().unwrap_or("");
    let artifact = parts.get(1).copied().unwrap_or("");
    let version = parts.get(2).copied().unwrap_or("");
    let classifier = parts.get(3).copied();

    let group_path = group.replace('.', "/");

    let filename = match classifier {
        Some(cls) => format!("{artifact}-{version}-{cls}.jar"),
        None => format!("{artifact}-{version}.jar"),
    };

    format!("{group_path}/{artifact}/{version}/{filename}")
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Make a string safe to embed in a cache filename.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::resolver::ArgumentEntry;

    // --- Fixture parse -------------------------------------------------------

    #[test]
    fn fixture_parses_main_class() {
        let json = include_str!("fixtures/fabric_profile.json");
        let profile: LoaderProfile = serde_json::from_str(json).expect("fixture should parse");
        assert_eq!(
            profile.main_class,
            "net.fabricmc.loader.impl.launch.knot.KnotClient"
        );
    }

    #[test]
    fn fixture_parses_libraries() {
        let json = include_str!("fixtures/fabric_profile.json");
        let profile: LoaderProfile = serde_json::from_str(json).expect("fixture should parse");
        assert!(!profile.libraries.is_empty(), "libraries should not be empty");
        // First library: fabric-loader itself
        assert_eq!(profile.libraries[0].name, "net.fabricmc:fabric-loader:0.16.10");
        assert_eq!(profile.libraries[0].url, "https://maven.fabricmc.net/");
    }

    #[test]
    fn fixture_parses_arguments() {
        let json = include_str!("fixtures/fabric_profile.json");
        let profile: LoaderProfile = serde_json::from_str(json).expect("fixture should parse");
        // JVM args: one entry "-DFabricMcEmu=net.minecraft.client.main.Main"
        assert_eq!(profile.arguments.jvm.len(), 1);
        match &profile.arguments.jvm[0] {
            ArgumentEntry::Plain(s) => assert_eq!(s, "-DFabricMcEmu=net.minecraft.client.main.Main"),
            other => panic!("expected Plain arg, got: {other:?}"),
        }
        // Game args: empty array
        assert_eq!(profile.arguments.game.len(), 0);
    }

    #[test]
    fn fixture_has_five_libraries() {
        let json = include_str!("fixtures/fabric_profile.json");
        let profile: LoaderProfile = serde_json::from_str(json).expect("fixture should parse");
        assert_eq!(profile.libraries.len(), 5);
    }

    // --- profile_url ---------------------------------------------------------

    #[test]
    fn profile_url_fabric() {
        let url = profile_url("fabric", "1.21.1", "0.16.10").expect("fabric url should be ok");
        assert_eq!(
            url,
            "https://meta.fabricmc.net/v2/versions/loader/1.21.1/0.16.10/profile/json"
        );
    }

    #[test]
    fn profile_url_quilt() {
        let url = profile_url("quilt", "1.21.1", "0.26.4").expect("quilt url should be ok");
        assert_eq!(
            url,
            "https://meta.quiltmc.org/v3/versions/loader/1.21.1/0.26.4/profile/json"
        );
    }

    #[test]
    fn profile_url_unknown_kind_errors() {
        let result = profile_url("forge", "1.21.1", "54.0.0");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown loader kind"));
    }

    #[test]
    fn profile_url_fabric_differs_from_quilt() {
        let fabric = profile_url("fabric", "1.20.4", "0.15.11").unwrap();
        let quilt = profile_url("quilt", "1.20.4", "0.26.4").unwrap();
        assert_ne!(fabric, quilt, "fabric and quilt URLs must differ");
        assert!(fabric.contains("fabricmc.net"));
        assert!(quilt.contains("quiltmc.org"));
    }

    // --- maven_coord_to_path -------------------------------------------------

    #[test]
    fn maven_coord_three_segment() {
        assert_eq!(
            maven_coord_to_path("net.fabricmc:fabric-loader:0.16.10"),
            "net/fabricmc/fabric-loader/0.16.10/fabric-loader-0.16.10.jar"
        );
    }

    #[test]
    fn maven_coord_four_segment_classifier() {
        assert_eq!(
            maven_coord_to_path("a.b:c:1.0:natives"),
            "a/b/c/1.0/c-1.0-natives.jar"
        );
    }

    #[test]
    fn maven_coord_asm() {
        assert_eq!(
            maven_coord_to_path("org.ow2.asm:asm:9.7"),
            "org/ow2/asm/asm/9.7/asm-9.7.jar"
        );
    }

    #[test]
    fn maven_coord_complex_version() {
        assert_eq!(
            maven_coord_to_path("net.fabricmc:sponge-mixin:0.15.4+mixin.0.8.7"),
            "net/fabricmc/sponge-mixin/0.15.4+mixin.0.8.7/sponge-mixin-0.15.4+mixin.0.8.7.jar"
        );
    }
}
