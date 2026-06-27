//! ATLauncher modpack provider — `api.atlauncher.com`.
//!
//! ## Design
//! - `AtlProvider` is a **keyless** `ModProvider` (the ATLauncher public API needs no key).
//! - Cloudflare blocks requests without a `User-Agent` (HTTP 1020) — every request must
//!   send a non-empty `User-Agent` header.
//! - Browse: one `GET /packs/full/public` call delivers the whole catalog (≤200 packs);
//!   no N+1. Client-side substring filter + name sort + offset/limit window (O-6).
//! - Mod list lives in a CDN `Configs.json` (NOT the API) — deserialized as
//!   `AtlConfigsManifest`. Each mod carries only `md5` for hash verification.
//! - Mods are **self-hosted** (`download ∈ {server, direct}`); keyless install, no CF.
//!   `browser` type maps to the pending-manual UX (rare, defensive — O-2).
//! - Config overrides = one root-relative `Configs.zip` at the CDN.
//! - Icon URL is **deterministic** from `safeName` — no fetch needed.
//!
//! See `docs/design/atlauncher-integration.md` for the verified API facts.

use serde::Deserialize;

use crate::core::providers::{
    ModBrief, ModProvider, PackInfo, PackSummary, ProjectSummary, ProjectVersion, ProviderError,
    ProviderHttpClient, ProviderKind, SearchParams, SearchResult,
};

/// ATL API base URL.
pub const API: &str = "https://api.atlauncher.com/v1";
/// ATL CDN base URL (mod jars, Configs.json/Configs.zip, icons).
pub const CDN: &str = "https://download.nodecdn.net/containers/atl";

/// Descriptive User-Agent — Cloudflare blocks empty/default UAs on ATL endpoints.
const USER_AGENT: &str = concat!(
    "modloader/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/; minecraft launcher)"
);

// ── ATL API envelope ─────────────────────────────────────────────────────────

/// Standard ATL API response wrapper. Every `/v1/` endpoint returns this shape.
/// CDN resources (Configs.json, Configs.zip) are bare — not wrapped.
#[derive(Debug, Deserialize)]
struct AtlEnvelope<T> {
    /// `true` when the API signals an error condition.
    #[serde(default)]
    error: bool,
    /// HTTP-mirrored status code inside the body.
    #[serde(default)]
    code: u16,
    /// Human-readable message (present on errors).
    #[serde(default)]
    message: String,
    /// Payload; `None` on error responses.
    data: Option<T>,
}

/// Unwrap an `AtlEnvelope`, returning `ProviderError::BadResponse` on API-level errors.
fn unwrap_envelope<T>(env: AtlEnvelope<T>) -> Result<T, ProviderError> {
    // `error` is the authoritative signal. `code` is only treated as a failure
    // when it is present and non-200; a 200-body that omits `code` (default 0)
    // must not be misclassified as an error.
    if env.error || (env.code != 0 && env.code != 200) {
        return Err(ProviderError::BadResponse(format!(
            "ATL API error {}: {}",
            env.code, env.message
        )));
    }
    env.data
        .ok_or_else(|| ProviderError::BadResponse("ATL API: missing data field".to_string()))
}

// ── Raw ATL deserialization types ─────────────────────────────────────────────

/// A single pack entry from `GET /packs/full/public` or `GET /pack/{safeName}`.
#[derive(Debug, Deserialize)]
struct AtlPack {
    /// Machine-friendly unique identifier used in all API/CDN paths.
    #[serde(rename = "safeName", default)]
    safe_name: String,
    /// Human-readable display name.
    #[serde(default)]
    name: String,
    /// Short description; may be multi-line (lines separated by `\n`).
    #[serde(default)]
    description: String,
    /// Available versions (summary only — no files, no loader info).
    #[serde(default)]
    versions: Vec<AtlPackVersion>,
}

/// A single version entry inside `AtlPack.versions`.
#[derive(Debug, Deserialize)]
struct AtlPackVersion {
    /// Version string (free-form, e.g. `"1.2.3"`).
    #[serde(default)]
    version: String,
    /// Minecraft version (e.g. `"1.20.1"`).
    #[serde(default)]
    minecraft: String,
}

/// Version summary returned by `GET /pack/{safeName}/latest`.
#[derive(Debug, Deserialize)]
struct AtlVersionSummary {
    #[serde(default)]
    version: String,
}

// ── Configs.json types (pub — consumed by the install planner in CP-3/CP-4) ──

/// The full `Configs.json` manifest from the CDN (not API-enveloped).
///
/// Fetched via `AtlProvider::get_configs_manifest`. The `mods` list drives the
/// download plan; `loader` provides the modloader kind + version; `configs`
/// provides the sha1 for `Configs.zip`; `memory` seeds `Source.recommended`.
#[derive(Debug, Clone, Deserialize)]
pub struct AtlConfigsManifest {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub minecraft: String,
    /// Modloader block (type + metadata). May be absent for vanilla packs.
    pub loader: Option<AtlLoader>,
    /// Mod list.
    #[serde(default)]
    pub mods: Vec<AtlMod>,
    /// Config-overrides archive metadata (sha1 + filesize).
    pub configs: Option<AtlConfigs>,
    /// Recommended memory in MiB. `> 0` → populate `Source.recommended` (O-5).
    pub memory: Option<u64>,
}

/// Loader block inside `Configs.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct AtlLoader {
    /// Loader kind: `"forge"`, `"neoforge"`, `"fabric"`, `"quilt"`, etc.
    #[serde(rename = "type", default)]
    pub loader_type: String,
    /// Per-loader metadata carrying the version string(s).
    pub metadata: AtlLoaderMeta,
}

/// Inner metadata of `AtlLoader`.
///
/// `version` carries the loader version for Forge/NeoForge (e.g. `"47.2.32"`).
/// `loader` carries the Fabric/Quilt loader version (e.g. `"0.15.11"`).
#[derive(Debug, Clone, Deserialize)]
pub struct AtlLoaderMeta {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub loader: String,
}

/// Config-overrides archive descriptor inside `Configs.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct AtlConfigs {
    /// File size in bytes.
    #[serde(default)]
    pub filesize: u64,
    /// SHA-1 hex digest of `Configs.zip`.
    #[serde(default)]
    pub sha1: String,
}

/// One mod entry inside `Configs.json`.
///
/// `download` determines the fetch strategy:
/// - `"server"` → `{CDN}/{url}` (most mods — self-hosted, keyless).
/// - `"direct"` → `url` is an absolute URL.
/// - `"browser"` → manual download (rare; maps to pending-manual UX, O-2).
///
/// Verification uses only `md5` — CP-2 adds `ExpectedHash::Md5` to the engine.
/// `optional` mods are installed by default (O-4); users can disable in the modlist.
#[derive(Debug, Clone, Deserialize)]
pub struct AtlMod {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
    /// URL path (relative for `server`, absolute for `direct`/`browser`).
    #[serde(default)]
    pub url: String,
    /// Filename on disk (including extension).
    #[serde(default)]
    pub file: String,
    /// MD5 hex digest of the jar.
    #[serde(default)]
    pub md5: String,
    /// Content type: `"mods"`, `"resourcepack"`, `"shaderpack"`, `"datapack"`, etc.
    #[serde(rename = "type", default)]
    pub mod_type: String,
    /// Fetch strategy: `"server"` | `"direct"` | `"browser"`.
    #[serde(default)]
    pub download: String,
    /// File size in bytes.
    #[serde(default)]
    pub filesize: u64,
    /// Optional override installation path (relative to the mc dir).
    #[serde(default)]
    pub path: String,
    /// `false` → server-only mod; skip on client install. Defaults to `true`.
    #[serde(default = "default_true")]
    pub client: bool,
    /// Optional mods are included by default (O-4).
    #[serde(default)]
    pub optional: bool,
    /// CurseForge project id, if this mod is CF-mirrored.
    #[serde(rename = "curseId", default)]
    pub curse_id: Option<u64>,
    /// CurseForge file id, if this mod is CF-mirrored.
    #[serde(rename = "curseFileId", default)]
    pub curse_file_id: Option<u64>,
}

fn default_true() -> bool {
    true
}

// ── URL builders ──────────────────────────────────────────────────────────────

fn api_packs_public_url() -> String {
    format!("{API}/packs/full/public")
}

fn api_pack_url(safe: &str) -> String {
    format!("{API}/pack/{safe}")
}

fn api_pack_version_url(safe: &str, ver: &str) -> String {
    format!("{API}/pack/{safe}/{}", percent_encode(ver))
}

/// CDN path for `Configs.json` — the bare pack manifest (not API-enveloped).
pub fn cdn_configs_url(safe: &str, ver: &str) -> String {
    format!(
        "{CDN}/packs/{safe}/versions/{}/Configs.json",
        percent_encode(ver)
    )
}

/// CDN path for `Configs.zip` — the config-overrides archive.
pub fn cdn_configs_zip_url(safe: &str, ver: &str) -> String {
    format!(
        "{CDN}/packs/{safe}/versions/{}/Configs.zip",
        percent_encode(ver)
    )
}

/// Percent-encode a free-form path segment (e.g. a version string). Encodes all
/// characters outside the unreserved set so a version containing `/`, spaces, or
/// other reserved characters cannot break the URL. Mirrors `modrinth::percent_encode`.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            other => {
                out.push('%');
                out.push_str(&format!("{:02X}", other));
            }
        }
    }
    out
}

/// CDN path for a pack-relative file (e.g. from `AtlMod.url` with `download == "server"`).
pub fn cdn_file_url(rel: &str) -> String {
    format!("{CDN}/{rel}")
}

/// Deterministic CDN icon URL for a pack. Lowercases `safe` (ATL CDN naming convention).
pub fn cdn_image_url(safe: &str) -> String {
    format!("{CDN}/launcher/images/{}.png", safe.to_ascii_lowercase())
}

// ── Provider ──────────────────────────────────────────────────────────────────

/// ATLauncher implementation of `ModProvider`. Keyless — no API key required.
///
/// Every request sends a `User-Agent` header; Cloudflare returns HTTP 1020 without one.
pub struct AtlProvider;

impl AtlProvider {
    pub fn new() -> Self {
        AtlProvider
    }

    /// GET a URL, deser the ATL API envelope, and return the inner `data`. Maps all errors.
    async fn get_api_json<T: serde::de::DeserializeOwned>(
        client: &dyn ProviderHttpClient,
        url: &str,
    ) -> Result<T, ProviderError> {
        let (status, body) = client
            .get(url, &[("User-Agent", USER_AGENT)])
            .await
            .map_err(ProviderError::from)?;
        if status != 200 {
            return Err(ProviderError::HttpStatus { status, body });
        }
        let env: AtlEnvelope<T> = serde_json::from_str(&body)
            .map_err(|e| ProviderError::BadResponse(e.to_string()))?;
        unwrap_envelope(env)
    }

    /// GET a CDN URL and deserialize the raw (non-enveloped) JSON body.
    async fn get_cdn_json<T: serde::de::DeserializeOwned>(
        client: &dyn ProviderHttpClient,
        url: &str,
    ) -> Result<T, ProviderError> {
        let (status, body) = client
            .get(url, &[("User-Agent", USER_AGENT)])
            .await
            .map_err(ProviderError::from)?;
        if status != 200 {
            return Err(ProviderError::HttpStatus { status, body });
        }
        serde_json::from_str(&body).map_err(|e| ProviderError::BadResponse(e.to_string()))
    }

    /// Fetch the latest version string for a pack.
    ///
    /// Returns `None` when the API returns an empty version string.
    /// Used by the install path (CP-4) and update-check (CP-6).
    pub async fn newest_version(
        &self,
        client: &dyn ProviderHttpClient,
        safe: &str,
    ) -> Result<Option<String>, ProviderError> {
        let summary: AtlVersionSummary =
            Self::get_api_json(client, &api_pack_version_url(safe, "latest")).await?;
        Ok(if summary.version.is_empty() {
            None
        } else {
            Some(summary.version)
        })
    }

    /// Fetch the `Configs.json` manifest for a specific pack version (CP-3/CP-4).
    ///
    /// This is a **bare CDN resource** — not wrapped in the ATL API envelope.
    pub async fn get_configs_manifest(
        &self,
        client: &dyn ProviderHttpClient,
        safe: &str,
        ver: &str,
    ) -> Result<AtlConfigsManifest, ProviderError> {
        Self::get_cdn_json(client, &cdn_configs_url(safe, ver)).await
    }
}

impl Default for AtlProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// Map an `AtlPack` to a normalized `ProjectSummary`.
fn pack_to_summary(pack: AtlPack) -> ProjectSummary {
    let summary = pack.description.lines().next().unwrap_or("").to_string();
    let icon_url = Some(cdn_image_url(&pack.safe_name));
    let page_url = Some(format!("https://atlauncher.com/pack/{}", pack.safe_name));
    ProjectSummary {
        provider: ProviderKind::Atlauncher,
        id: pack.safe_name.clone(),
        slug: pack.safe_name,
        name: pack.name,
        summary,
        downloads: 0,
        icon_url,
        categories: vec![],
        page_url,
    }
}

#[async_trait::async_trait]
impl ModProvider for AtlProvider {
    async fn search(
        &self,
        client: &dyn ProviderHttpClient,
        params: &SearchParams,
    ) -> Result<SearchResult, ProviderError> {
        // One call delivers the whole catalog — no N+1 (ATL is ~150 packs).
        let mut packs: Vec<AtlPack> =
            Self::get_api_json(client, &api_packs_public_url()).await?;

        // Client-side filter: case-insensitive substring match on name.
        let query = params.query.trim().to_ascii_lowercase();
        if !query.is_empty() {
            packs.retain(|p| p.name.to_ascii_lowercase().contains(&query));
        }

        // O-6: stable alphabetical sort by name (lowercased for consistent ordering).
        packs.sort_by(|a, b| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
        });

        let total = packs.len() as u32;
        let offset = params.offset as usize;
        let limit = params.limit.max(1) as usize;

        let hits = packs
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(pack_to_summary)
            .collect();

        Ok(SearchResult {
            hits,
            offset: params.offset,
            total,
        })
    }

    async fn get_versions(
        &self,
        client: &dyn ProviderHttpClient,
        project_id: &str,
        _mc_version: Option<&str>,
        _loader: Option<&str>,
    ) -> Result<Vec<ProjectVersion>, ProviderError> {
        let pack: AtlPack = Self::get_api_json(client, &api_pack_url(project_id)).await?;
        Ok(pack
            .versions
            .into_iter()
            .map(|v| ProjectVersion {
                provider: ProviderKind::Atlauncher,
                id: v.version.clone(),
                name: v.version.clone(),
                version_number: v.version,
                game_versions: if v.minecraft.is_empty() {
                    vec![]
                } else {
                    vec![v.minecraft]
                },
                // Loaders are only known from Configs.json (fetched at install time).
                loaders: vec![],
                // ATL packs have no single jar — install consumes Configs.json.
                files: vec![],
                dependencies: vec![],
            })
            .collect())
    }

    async fn get_project(
        &self,
        client: &dyn ProviderHttpClient,
        project_id: &str,
    ) -> Result<PackInfo, ProviderError> {
        let pack: AtlPack = Self::get_api_json(client, &api_pack_url(project_id)).await?;
        Ok(PackInfo {
            title: pack.name,
            description: pack.description,
            icon_url: Some(cdn_image_url(&pack.safe_name)),
            // ATL descriptions are plain text (neither Markdown nor HTML).
            body_is_html: false,
        })
    }

    async fn get_projects_brief(
        &self,
        _client: &dyn ProviderHttpClient,
        _ids: &[String],
    ) -> Result<Vec<ModBrief>, ProviderError> {
        // ATL has no batch metadata endpoint; all ATL mods are self-hosted and
        // metadata is captured at install-time from Configs.json. No-op.
        Ok(Vec::new())
    }

    async fn get_pack_summary(
        &self,
        client: &dyn ProviderHttpClient,
        project_id: &str,
    ) -> Result<PackSummary, ProviderError> {
        let pack: AtlPack = Self::get_api_json(client, &api_pack_url(project_id)).await?;
        let summary = pack
            .description
            .lines()
            .next()
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty());
        Ok(PackSummary {
            name: pack.name,
            icon_url: Some(cdn_image_url(&pack.safe_name)),
            // ATL API does not expose author information.
            author: None,
            summary,
            categories: vec![],
        })
    }
}

#[cfg(test)]
#[path = "atl_tests.rs"]
mod tests;
