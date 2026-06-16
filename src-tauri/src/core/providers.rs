//! Normalized provider types, `ModProvider` trait, and HTTP seam for mod-search backends.
//!
//! ## Design
//! - `ModProvider` is an async_trait object-safe over `ProviderHttpClient`.
//! - `ProviderHttpClient` is the injectable HTTP seam (same pattern as `AuthHttpClient`,
//!   `auth.rs:226`): production uses `reqwest`, tests inject a mock backed by a VecDeque.
//! - All IPC-crossing structs carry `#[serde(rename_all = "camelCase")]` (project
//!   convention).
//! - CF key resolution: `settings.curseforge_api_key` + `MODLOADER_CF_API_KEY` env
//!   override, mirroring `ms_client_id_from` in `auth.rs:28-37`.

use serde::{Deserialize, Serialize};

// ── Env constant ──────────────────────────────────────────────────────────────

/// Env var that overrides the CurseForge API key (for dev/CI).
pub const CF_API_KEY_ENV: &str = "MODLOADER_CF_API_KEY";

/// Resolve the effective CurseForge API key.
///
/// Priority: `MODLOADER_CF_API_KEY` env var (if set and non-blank), then
/// `settings_key` (if `Some` and non-blank), then `None`.
///
/// Separated from env access so tests need no global env mutation.
/// Pure resolution logic used by both the live path and tests.
///
/// `env_val`: value of `MODLOADER_CF_API_KEY` (pre-fetched by the caller).
/// `settings_val`: value of `settings.curseforge_api_key` (pre-fetched by the caller).
pub fn cf_api_key_from(
    env_val: Option<String>,
    settings_val: Option<String>,
) -> Option<String> {
    if let Some(v) = env_val {
        if !v.trim().is_empty() {
            return Some(v);
        }
    }
    if let Some(v) = settings_val {
        if !v.trim().is_empty() {
            return Some(v);
        }
    }
    None
}

// ── Normalized domain types ───────────────────────────────────────────────────

/// Condensed summary of a mod project, suitable for search result cards.
///
/// Maps from both Modrinth `/search` hits and CF `/v1/mods/search` data entries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    /// Which backend provided this result.
    pub provider: ProviderKind,
    /// Provider-specific project identifier (Modrinth: base62 string; CF: numeric id as string).
    pub id: String,
    /// URL-friendly slug (e.g. `"sodium"`).
    pub slug: String,
    /// Display name (e.g. `"Sodium"`).
    pub name: String,
    /// Short human-readable description.
    pub summary: String,
    /// Total all-time download count.
    pub downloads: u64,
    /// Icon URL, if any.
    pub icon_url: Option<String>,
    /// Category tags (normalized to lowercase strings).
    pub categories: Vec<String>,
    /// Provider project page URL, if available.
    ///
    /// Modrinth: `https://modrinth.com/{project_type}/{slug}` (derived from the response hit's
    /// `project_type` field, not the search selector).
    /// CurseForge: `links.websiteUrl` from the search row verbatim; `None` when absent/null.
    pub page_url: Option<String>,
}

/// A specific release of a mod project.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectVersion {
    /// Which backend provided this version.
    pub provider: ProviderKind,
    /// Version identifier (Modrinth: base62 id; CF: numeric file id as string).
    pub id: String,
    /// Human-readable name (e.g. `"Sodium 0.5.11 for MC 1.21"`).
    pub name: String,
    /// Semantic-ish version string (e.g. `"mc1.21-0.5.11"`).
    pub version_number: String,
    /// Compatible Minecraft versions (e.g. `["1.21", "1.20.4"]`).
    pub game_versions: Vec<String>,
    /// Compatible mod loaders (e.g. `["fabric", "quilt"]`).
    pub loaders: Vec<String>,
    /// Downloadable files for this version.
    pub files: Vec<VersionFile>,
    /// Dependencies declared by this version.
    pub dependencies: Vec<Dependency>,
}

/// A single downloadable file within a `ProjectVersion`.
///
/// `url` is `None` when the author has disabled distribution (CF `allowModDistribution: false`
/// or `downloadUrl: null`). The UI must surface a manual-download prompt in that case.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VersionFile {
    /// Direct download URL; `None` means distribution is disabled.
    pub url: Option<String>,
    /// Filename as declared by the provider.
    pub file_name: String,
    /// File size in bytes, if known.
    pub size: Option<u64>,
    /// Verification hashes keyed by algorithm name (e.g. `"sha1"`, `"sha512"`).
    pub hashes: std::collections::HashMap<String, String>,
    /// Whether this is the primary/recommended file for the version.
    pub primary: bool,
}

/// A declared dependency of a `ProjectVersion`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Dependency {
    /// Provider-specific project id of the dependency, if known.
    pub project_id: Option<String>,
    /// Provider-specific version id of the dependency, if known.
    pub version_id: Option<String>,
    /// Dependency type: `"required"`, `"optional"`, `"incompatible"`, or `"embedded"`.
    pub dependency_type: String,
}

/// Which class of projects to search for.
///
/// Maps to provider-specific selectors:
/// - Modrinth: `project_type` facet (`"mod"` or `"modpack"`).
/// - CurseForge: `classId` query param (`6` for mods, `4471` for modpacks).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum ProjectType {
    /// Standard mods (default).
    #[default]
    #[serde(rename = "mod")]
    Mod,
    /// Modpacks.
    #[serde(rename = "modpack")]
    Modpack,
}

/// Query parameters for searching mods.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchParams {
    /// Free-text search query.
    pub query: String,
    /// Filter to a specific Minecraft version (e.g. `"1.21"`); `None` = any.
    pub mc_version: Option<String>,
    /// Filter to a specific loader (e.g. `"fabric"`); `None` = any.
    pub loader: Option<String>,
    /// Pagination offset (first result index).
    pub offset: u32,
    /// Maximum number of results to return.
    pub limit: u32,
    /// Which project class to search (mod or modpack). Defaults to `Mod`.
    #[serde(default)]
    pub project_type: ProjectType,
}

/// Paginated search response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    /// Matching project summaries for this page.
    pub hits: Vec<ProjectSummary>,
    /// Zero-based offset of the first result in the full result set.
    pub offset: u32,
    /// Total number of results matching the query (across all pages).
    pub total: u32,
}

/// Identifies which provider owns a result.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProviderKind {
    Modrinth,
    CurseForge,
}

// ── HTTP seam ─────────────────────────────────────────────────────────────────

/// Minimal async HTTP abstraction so tests can inject a mock without live TCP.
///
/// Mirrors the `AuthHttpClient` pattern (`auth.rs:226`). Each method returns
/// `(status_code, body_text)` so callers can inspect error bodies. The production
/// implementation uses a shared `reqwest::Client`.
#[async_trait::async_trait]
pub trait ProviderHttpClient: Send + Sync {
    /// GET a URL with optional extra headers. Returns `(status, body)`.
    ///
    /// `headers`: slice of `(name, value)` pairs added verbatim to the request.
    async fn get(
        &self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<(u16, String), reqwest::Error>;
}

/// Production implementation backed by a shared `reqwest::Client`.
pub struct ReqwestProviderClient(pub reqwest::Client);

#[async_trait::async_trait]
impl ProviderHttpClient for ReqwestProviderClient {
    async fn get(
        &self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<(u16, String), reqwest::Error> {
        let mut req = self.0.get(url);
        for (name, value) in headers {
            req = req.header(*name, *value);
        }
        let resp = req.send().await?;
        let status = resp.status().as_u16();
        let body = resp.text().await?;
        Ok((status, body))
    }
}

// ── Provider trait ────────────────────────────────────────────────────────────

/// Error returned by provider operations.
///
/// This is an internal type. The IPC-boundary error (`ProviderCommandError { kind, message }`)
/// is defined in `lib.rs` alongside the Tauri commands (Checkpoint 4).
#[derive(Debug, Clone, thiserror::Error)]
pub enum ProviderError {
    /// The CurseForge API key is missing from both the env var and settings.
    #[error("CurseForge API key is not configured")]
    KeyMissing,
    /// A transport-level failure (connection refused, timeout, TLS error, etc.).
    /// Distinct from `BadResponse` which means the HTTP exchange completed but
    /// the body could not be parsed.
    #[error("network error: {0}")]
    Network(String),
    /// The provider returned an unexpected HTTP status.
    #[error("HTTP {status}: {body}")]
    HttpStatus { status: u16, body: String },
    /// The provider returned a body that could not be parsed.
    #[error("bad response: {0}")]
    BadResponse(String),
}

impl From<reqwest::Error> for ProviderError {
    fn from(e: reqwest::Error) -> Self {
        ProviderError::Network(e.to_string())
    }
}

/// A mod provider capable of searching mods and fetching version lists.
///
/// Object-safe: `Box<dyn ModProvider>` is valid. HTTP is injected via
/// `ProviderHttpClient` so tests can supply a mock without live network.
#[async_trait::async_trait]
pub trait ModProvider: Send + Sync {
    /// Search mods according to `params`. Returns a paginated `SearchResult`.
    async fn search(
        &self,
        client: &dyn ProviderHttpClient,
        params: &SearchParams,
    ) -> Result<SearchResult, ProviderError>;

    /// Fetch all versions of a project compatible with `mc_version` and `loader`.
    ///
    /// `mc_version` and `loader` are advisory filters; implementations may apply
    /// them server-side or client-side depending on what the API supports.
    async fn get_versions(
        &self,
        client: &dyn ProviderHttpClient,
        project_id: &str,
        mc_version: Option<&str>,
        loader: Option<&str>,
    ) -> Result<Vec<ProjectVersion>, ProviderError>;
}

// ── Raw Modrinth deserialization types ────────────────────────────────────────

/// Raw Modrinth `/search` response shape (subset of fields needed for normalization).
#[derive(Debug, Deserialize)]
pub struct MrSearchResponse {
    pub hits: Vec<MrHit>,
    pub total_hits: u32,
    pub offset: u32,
    pub limit: u32,
}

#[derive(Debug, Deserialize)]
pub struct MrHit {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub categories: Vec<String>,
    pub downloads: u64,
    pub icon_url: Option<String>,
    /// Modrinth project type string (e.g. `"mod"`, `"modpack"`). Used to build `page_url`.
    pub project_type: String,
}

impl MrHit {
    pub fn into_summary(self) -> ProjectSummary {
        let page_url = Some(format!(
            "https://modrinth.com/{}/{}",
            self.project_type, self.slug
        ));
        ProjectSummary {
            provider: ProviderKind::Modrinth,
            id: self.project_id,
            slug: self.slug,
            name: self.title,
            summary: self.description,
            downloads: self.downloads,
            icon_url: self.icon_url,
            categories: self.categories,
            page_url,
        }
    }
}

// ── Raw CurseForge deserialization types ──────────────────────────────────────

/// Raw CF `/v1/mods/search` response shape.
#[derive(Debug, Deserialize)]
pub struct CfSearchResponse {
    pub data: Vec<CfMod>,
    pub pagination: CfPagination,
}

#[derive(Debug, Deserialize)]
pub struct CfMod {
    pub id: u64,
    pub name: String,
    pub slug: String,
    pub summary: String,
    #[serde(rename = "downloadCount")]
    pub download_count: u64,
    pub logo: Option<CfLogo>,
    pub categories: Vec<CfCategory>,
    /// CF search row links block — `websiteUrl` is used for `page_url`.
    pub links: Option<CfLinks>,
}

#[derive(Debug, Deserialize)]
pub struct CfLogo {
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct CfCategory {
    pub name: String,
}

/// Links block from a CF search row.
#[derive(Debug, Deserialize)]
pub struct CfLinks {
    #[serde(rename = "websiteUrl")]
    pub website_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CfPagination {
    pub index: u32,
    #[serde(rename = "totalCount")]
    pub total_count: u32,
}

impl CfMod {
    pub fn into_summary(self) -> ProjectSummary {
        let page_url = self.links.and_then(|l| l.website_url);
        ProjectSummary {
            provider: ProviderKind::CurseForge,
            id: self.id.to_string(),
            slug: self.slug,
            name: self.name,
            summary: self.summary,
            downloads: self.download_count,
            icon_url: self.logo.map(|l| l.url),
            categories: self.categories.into_iter().map(|c| c.name).collect(),
            page_url,
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "providers_tests.rs"]
mod tests;
