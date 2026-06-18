//! Per-Minecraft mod-loader version lists, fetched from each loader's own metadata.
//!
//! Availability differs by MC version (see `docs/PROVIDERS.md` / research notes):
//! - **Forge**    — `maven-metadata.xml`, artifacts `{mc}-{forge}` (1.7.10 has a doubled
//!                  `-{mc}` suffix). Covers very old MC, incl. 1.7.10.
//! - **Fabric**   — `meta.fabricmc.net/v2/versions/loader/{mc}` (MC 1.14+).
//! - **Quilt**    — `meta.quiltmc.org/v3/versions/loader/{mc}` (MC 1.14.4+).
//! - **NeoForge** — `maven-metadata.xml`; MC is *derived* from the version (no MC string),
//!                  plus a legacy artifact for 1.20.1 (MC 1.20.2+ otherwise).
//!
//! Each loader is fetched independently; a network/parse failure for one is treated as
//! "not available for this MC" rather than failing the whole request. Vanilla is always
//! offered (empty version list).

use std::time::Duration;

use serde::Serialize;
use tauri::AppHandle;

use crate::core::meta;

const TTL: Duration = Duration::from_secs(6 * 3600);

const FORGE_MAVEN: &str =
    "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml";
const NEOFORGE_MAVEN: &str =
    "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml";
const NEOFORGE_LEGACY_MAVEN: &str =
    "https://maven.neoforged.net/releases/net/neoforged/forge/maven-metadata.xml";

#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LoaderOption {
    /// "vanilla" | "forge" | "fabric" | "quilt" | "neoforge"
    pub kind: String,
    /// Loader build versions, newest first. Empty for vanilla.
    pub versions: Vec<String>,
}

/// All loaders available for `mc`, each with its build list (newest first).
pub async fn for_mc(app: &AppHandle, mc: &str) -> Result<Vec<LoaderOption>, String> {
    let mut out = vec![LoaderOption {
        kind: "vanilla".into(),
        versions: Vec::new(),
    }];

    for (kind, versions) in [
        ("forge", forge_versions(app, mc).await),
        ("fabric", fabric_like_versions(app, mc, FabricFamily::Fabric).await),
        ("quilt", fabric_like_versions(app, mc, FabricFamily::Quilt).await),
        ("neoforge", neoforge_versions(app, mc).await),
    ] {
        if let Ok(v) = versions {
            if !v.is_empty() {
                out.push(LoaderOption {
                    kind: kind.into(),
                    versions: v,
                });
            }
        }
    }
    Ok(out)
}

// --- Forge -----------------------------------------------------------------

async fn forge_versions(app: &AppHandle, mc: &str) -> Result<Vec<String>, String> {
    let body = meta::cached_text(app, FORGE_MAVEN, "forge-maven-metadata.xml", TTL).await?;
    let prefix = format!("{mc}-");
    let suffix = format!("-{mc}"); // 1.7.10 quirk: `1.7.10-10.13.4.1614-1.7.10`
    let mut versions: Vec<String> = extract_xml_versions(&body)
        .into_iter()
        .filter_map(|v| v.strip_prefix(&prefix).map(str::to_string))
        .map(|rest| rest.strip_suffix(&suffix).map(str::to_string).unwrap_or(rest))
        .collect();
    sort_versions_desc(&mut versions);
    Ok(versions)
}

// --- Fabric / Quilt (same JSON shape) --------------------------------------

enum FabricFamily {
    Fabric,
    Quilt,
}

async fn fabric_like_versions(
    app: &AppHandle,
    mc: &str,
    family: FabricFamily,
) -> Result<Vec<String>, String> {
    let (url, key) = match family {
        FabricFamily::Fabric => (
            format!("https://meta.fabricmc.net/v2/versions/loader/{mc}"),
            format!("fabric-loader-{}.json", sanitize(mc)),
        ),
        FabricFamily::Quilt => (
            format!("https://meta.quiltmc.org/v3/versions/loader/{mc}"),
            format!("quilt-loader-{}.json", sanitize(mc)),
        ),
    };
    let body = meta::cached_text(app, &url, &key, TTL).await?;
    let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    // Already newest-first from the API; preserve order.
    Ok(json
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| e["loader"]["version"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default())
}

// --- NeoForge --------------------------------------------------------------

async fn neoforge_versions(app: &AppHandle, mc: &str) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();

    let body = meta::cached_text(app, NEOFORGE_MAVEN, "neoforge-maven-metadata.xml", TTL).await?;
    for v in extract_xml_versions(&body) {
        if neoforge_matches(&v, mc) {
            out.push(v);
        }
    }

    // 1.20.1 lives under the legacy `net/neoforged/forge` artifact (`1.20.1-47.x.y`).
    if mc == "1.20.1" {
        if let Ok(legacy) =
            meta::cached_text(app, NEOFORGE_LEGACY_MAVEN, "neoforge-legacy-maven-metadata.xml", TTL)
                .await
        {
            let prefix = "1.20.1-";
            out.extend(
                extract_xml_versions(&legacy)
                    .into_iter()
                    .filter_map(|v| v.strip_prefix(prefix).map(str::to_string)),
            );
        }
    }

    sort_versions_desc(&mut out);
    Ok(out)
}

/// NeoForge encodes MC in the version, not as a literal string:
/// `20.4.237` → 1.20.4, `21.0.167` → 1.21.0 (≡ MC "1.21"), `26.1.2.73` → 26.1.2.
fn neoforge_matches(version: &str, mc: &str) -> bool {
    let core = version.split('-').next().unwrap_or(version); // drop `-beta` etc.
    let parts: Vec<&str> = core.split('.').collect();
    let derived = match parts.len() {
        3 => format!("1.{}.{}", parts[0], parts[1]), // A.B.C  -> 1.A.B
        4 => format!("{}.{}.{}", parts[0], parts[1], parts[2]), // A.B.C.D -> A.B.C (new scheme)
        _ => return false,
    };
    derived == mc || derived.strip_suffix(".0") == Some(mc)
}

// --- shared helpers --------------------------------------------------------

/// Pull every `<version>…</version>` payload out of a maven-metadata XML doc.
fn extract_xml_versions(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<version>") {
        let after = &rest[start + "<version>".len()..];
        match after.find("</version>") {
            Some(end) => {
                out.push(after[..end].trim().to_string());
                rest = &after[end..];
            }
            None => break,
        }
    }
    out
}

/// Sort dotted/dashed numeric versions newest-first (non-numeric segments → 0).
fn sort_versions_desc(versions: &mut [String]) {
    versions.sort_by(|a, b| version_key(b).cmp(&version_key(a)));
}

fn version_key(v: &str) -> Vec<u64> {
    v.split(['.', '-'])
        .map(|p| p.parse::<u64>().unwrap_or(0))
        .collect()
}

/// Make an MC id safe as a cache filename (`1.20.1` is fine; future-proof anyway).
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' { c } else { '_' })
        .collect()
}
