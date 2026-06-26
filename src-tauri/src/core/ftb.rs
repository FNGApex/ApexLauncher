//! FTB (Feed The Beast) modpack provider — `api.modpacks.ch`.
//!
//! ## Design
//! - `FtbProvider` is a **keyless** `ModProvider` (the FTB public API needs no key).
//! - Browse list endpoints return only integer pack ids, so `search` does an N+1
//!   detail fetch (one `/public/modpack/{id}` per visible pack). The catalog is
//!   small (~130 packs) and the server caches aggressively (`max-age=900`).
//! - FTB has no single primary jar per pack — a pack *version* is a JSON manifest
//!   of N files. So `get_versions` returns empty `files`; the install path
//!   (`core::modpack::resolve_and_build_ftb_plan`, CP-2) consumes the manifest.
//! - File kinds are a clean XOR: either an FTB-CDN `url`, or a `curseforge` block
//!   (no url) resolved via the CurseForge API. Manifest types here are `pub` so
//!   the install planner can deserialize and route them.
//!
//! See `docs/design/ftb-integration.md` for the verified API facts.

use serde::Deserialize;

use crate::core::providers::{
    ModBrief, ModProvider, PackInfo, PackSummary, ProjectSummary, ProjectVersion,
    ProviderError, ProviderHttpClient, ProviderKind, SearchParams, SearchResult,
};

const BASE: &str = "https://api.modpacks.ch";

/// Descriptive User-Agent (FTB etiquette, mirrors the Modrinth client).
const USER_AGENT: &str = concat!(
    "modloader/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/; minecraft launcher)"
);

// ── Raw FTB deserialization types ───────────────────────────────────────────────

/// A list endpoint response (`featured`/`popular`/`search`). Carries pack ids only;
/// `search` also echoes a `curseforge` id array which we ignore.
#[derive(Debug, Deserialize)]
struct FtbListResponse {
    #[serde(default)]
    packs: Vec<u64>,
}

/// One `art[]` entry on a pack detail. The icon is the `type == "square"` url.
#[derive(Debug, Deserialize)]
struct FtbArt {
    #[serde(default)]
    url: String,
    #[serde(rename = "type", default)]
    art_type: String,
}

#[derive(Debug, Deserialize)]
struct FtbAuthor {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Deserialize)]
struct FtbTag {
    #[serde(default)]
    name: String,
}

/// A `targets[]` entry — encodes loader / minecraft / java for a version.
#[derive(Debug, Clone, Deserialize)]
pub struct FtbTarget {
    #[serde(default)]
    pub name: String,
    #[serde(rename = "type", default)]
    pub target_type: String,
    #[serde(default)]
    pub version: String,
}

/// `specs` — recommended/minimum RAM in MiB.
#[derive(Debug, Clone, Deserialize)]
pub struct FtbSpecs {
    #[serde(default)]
    pub minimum: Option<u64>,
    #[serde(default)]
    pub recommended: Option<u64>,
}

/// A `versions[]` entry on a pack detail (summary — no `files`).
#[derive(Debug, Deserialize)]
struct FtbVersionEntry {
    id: u64,
    #[serde(default)]
    name: String,
    #[serde(rename = "type", default)]
    version_type: String,
    #[serde(default)]
    targets: Vec<FtbTarget>,
}

/// `GET /public/modpack/{id}` — pack detail.
#[derive(Debug, Deserialize)]
struct FtbModpackDetail {
    id: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    synopsis: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    art: Vec<FtbArt>,
    #[serde(default)]
    authors: Vec<FtbAuthor>,
    #[serde(default)]
    tags: Vec<FtbTag>,
    #[serde(default)]
    versions: Vec<FtbVersionEntry>,
    #[serde(default)]
    installs: u64,
}

/// A CurseForge reference on a manifest file (when `url` is empty).
#[derive(Debug, Clone, Deserialize)]
pub struct FtbCurseforge {
    pub project: u64,
    pub file: u64,
}

/// A `files[]` entry in a version manifest. Either FTB-CDN (`url` set) XOR
/// CurseForge-referenced (`curseforge` set, `url` empty).
#[derive(Debug, Clone, Deserialize)]
pub struct FtbFile {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub sha1: String,
    #[serde(default)]
    pub size: u64,
    #[serde(rename = "type", default)]
    pub file_type: String,
    #[serde(default)]
    pub clientonly: bool,
    #[serde(default)]
    pub serveronly: bool,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub curseforge: Option<FtbCurseforge>,
}

/// `GET /public/modpack/{id}/{versionId}` — a version's full file manifest.
#[derive(Debug, Clone, Deserialize)]
pub struct FtbVersionManifest {
    #[serde(default)]
    pub name: String,
    #[serde(rename = "type", default)]
    pub manifest_type: String,
    #[serde(default)]
    pub targets: Vec<FtbTarget>,
    #[serde(default)]
    pub specs: Option<FtbSpecs>,
    #[serde(default)]
    pub files: Vec<FtbFile>,
}

impl FtbVersionManifest {
    /// The modloader name (`forge`/`neoforge`/`fabric`/`quilt`), lowercased, if any.
    pub fn loader(&self) -> Option<String> {
        target_value(&self.targets, "modloader").map(|n| n.to_ascii_lowercase())
    }
    /// The Minecraft version, if any.
    pub fn minecraft(&self) -> Option<String> {
        target_version(&self.targets, "game")
    }
}

/// First target `name` whose `type` matches (lowercased compare).
fn target_value(targets: &[FtbTarget], kind: &str) -> Option<String> {
    targets
        .iter()
        .find(|t| t.target_type.eq_ignore_ascii_case(kind))
        .map(|t| t.name.clone())
        .filter(|s| !s.is_empty())
}

/// First target `version` whose `type` matches (lowercased compare).
fn target_version(targets: &[FtbTarget], kind: &str) -> Option<String> {
    targets
        .iter()
        .find(|t| t.target_type.eq_ignore_ascii_case(kind))
        .map(|t| t.version.clone())
        .filter(|s| !s.is_empty())
}

/// The square-art icon url, falling back to the first art entry.
fn square_art(art: &[FtbArt]) -> Option<String> {
    art.iter()
        .find(|a| a.art_type.eq_ignore_ascii_case("square"))
        .or_else(|| art.first())
        .map(|a| a.url.clone())
        .filter(|s| !s.is_empty())
}

// ── Provider ─────────────────────────────────────────────────────────────────────

/// FTB implementation of `ModProvider`. Keyless — no API key state.
pub struct FtbProvider;

impl FtbProvider {
    pub fn new() -> Self {
        FtbProvider
    }

    fn build_featured_url(limit: u32) -> String {
        format!("{BASE}/public/modpack/featured/{limit}")
    }
    fn build_popular_url(limit: u32) -> String {
        format!("{BASE}/public/modpack/popular/installs/{limit}")
    }
    fn build_search_url(limit: u32, term: &str) -> String {
        format!(
            "{BASE}/public/modpack/search/{limit}?term={}",
            percent_encode_query(term)
        )
    }
    fn build_detail_url(id: &str) -> String {
        format!("{BASE}/public/modpack/{id}")
    }
    fn build_version_url(id: u64, version_id: u64) -> String {
        format!("{BASE}/public/modpack/{id}/{version_id}")
    }

    /// GET a URL and parse JSON, mapping transport/status/parse failures.
    async fn get_json<T: serde::de::DeserializeOwned>(
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

    /// Fetch a pack id list (`packs[]`) from a list endpoint.
    async fn fetch_ids(
        client: &dyn ProviderHttpClient,
        url: &str,
    ) -> Result<Vec<u64>, ProviderError> {
        let resp: FtbListResponse = Self::get_json(client, url).await?;
        Ok(resp.packs)
    }

    /// Fetch one pack detail.
    async fn fetch_detail(
        client: &dyn ProviderHttpClient,
        id: u64,
    ) -> Result<FtbModpackDetail, ProviderError> {
        Self::get_json(client, &Self::build_detail_url(&id.to_string())).await
    }

    /// Fetch a version manifest (used by the install planner, CP-2/CP-3).
    pub async fn get_version_manifest(
        &self,
        client: &dyn ProviderHttpClient,
        id: u64,
        version_id: u64,
    ) -> Result<FtbVersionManifest, ProviderError> {
        Self::get_json(client, &Self::build_version_url(id, version_id)).await
    }
}

impl Default for FtbProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// Map a pack detail to a normalized search summary.
fn detail_to_summary(d: FtbModpackDetail) -> ProjectSummary {
    ProjectSummary {
        provider: ProviderKind::Ftb,
        id: d.id.to_string(),
        // FTB packs have no slug; the numeric id stands in.
        slug: d.id.to_string(),
        name: d.name,
        summary: d.synopsis,
        downloads: d.installs,
        icon_url: square_art(&d.art),
        categories: d.tags.into_iter().map(|t| t.name).collect(),
        page_url: Some(format!("https://www.feed-the-beast.com/modpacks/{}", d.id)),
    }
}

#[async_trait::async_trait]
impl ModProvider for FtbProvider {
    async fn search(
        &self,
        client: &dyn ProviderHttpClient,
        params: &SearchParams,
    ) -> Result<SearchResult, ProviderError> {
        let limit = params.limit.max(1);

        // Empty query → the curated default feed (featured + popular, deduped,
        // featured first). Non-empty → the term search (FTB `packs[]` only).
        let ids = if params.query.trim().is_empty() {
            let mut ids = Self::fetch_ids(client, &Self::build_featured_url(limit)).await?;
            // Popular is best-effort — a failure here must not blank the feed.
            if let Ok(popular) = Self::fetch_ids(client, &Self::build_popular_url(limit)).await {
                for id in popular {
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                }
            }
            ids
        } else {
            Self::fetch_ids(client, &Self::build_search_url(limit, params.query.trim())).await?
        };

        let total = ids.len() as u32;
        let offset = params.offset as usize;

        // N+1 detail fetch for the visible window only.
        let mut hits = Vec::new();
        for id in ids.into_iter().skip(offset).take(limit as usize) {
            // A single bad detail shouldn't abort the whole feed.
            if let Ok(detail) = Self::fetch_detail(client, id).await {
                hits.push(detail_to_summary(detail));
            }
        }

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
        let detail: FtbModpackDetail =
            Self::get_json(client, &Self::build_detail_url(project_id)).await?;

        Ok(detail
            .versions
            .into_iter()
            .map(|v| {
                let mc = target_version(&v.targets, "game");
                let loader = target_value(&v.targets, "modloader").map(|n| n.to_ascii_lowercase());
                ProjectVersion {
                    provider: ProviderKind::Ftb,
                    id: v.id.to_string(),
                    name: v.name.clone(),
                    version_number: v.name,
                    game_versions: mc.into_iter().collect(),
                    loaders: loader.into_iter().collect(),
                    // FTB has no single jar — install consumes the manifest itself.
                    files: Vec::new(),
                    dependencies: Vec::new(),
                }
            })
            .collect())
    }

    async fn get_project(
        &self,
        client: &dyn ProviderHttpClient,
        project_id: &str,
    ) -> Result<PackInfo, ProviderError> {
        let d: FtbModpackDetail =
            Self::get_json(client, &Self::build_detail_url(project_id)).await?;
        Ok(PackInfo {
            title: d.name,
            description: d.description,
            icon_url: square_art(&d.art),
            // FTB descriptions are Markdown.
            body_is_html: false,
        })
    }

    async fn get_projects_brief(
        &self,
        _client: &dyn ProviderHttpClient,
        _ids: &[String],
    ) -> Result<Vec<ModBrief>, ProviderError> {
        // FTB has no batch mod-metadata endpoint; CF-referenced mods enrich via the
        // CF path, FTB-hosted jars stay un-enriched. No-op (api-frugality).
        Ok(Vec::new())
    }

    async fn get_pack_summary(
        &self,
        client: &dyn ProviderHttpClient,
        project_id: &str,
    ) -> Result<PackSummary, ProviderError> {
        let d: FtbModpackDetail =
            Self::get_json(client, &Self::build_detail_url(project_id)).await?;
        let author = d.authors.into_iter().map(|a| a.name).find(|n| !n.is_empty());
        Ok(PackSummary {
            name: d.name,
            icon_url: square_art(&d.art),
            author,
            summary: if d.synopsis.is_empty() {
                None
            } else {
                Some(d.synopsis)
            },
            categories: d.tags.into_iter().map(|t| t.name).collect(),
        })
    }
}

/// Minimal percent-encoding for a query-param value (encode chars outside the
/// unreserved set). Mirrors `curseforge::percent_encode`.
fn percent_encode_query(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
#[path = "ftb_tests.rs"]
mod tests;
