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
    /// The provider returned an unexpected HTTP status.
    #[error("HTTP {status}: {body}")]
    HttpStatus { status: u16, body: String },
    /// The provider returned a body that could not be parsed.
    #[error("bad response: {0}")]
    BadResponse(String),
}

impl From<reqwest::Error> for ProviderError {
    fn from(e: reqwest::Error) -> Self {
        ProviderError::BadResponse(e.to_string())
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
}

impl MrHit {
    pub fn into_summary(self) -> ProjectSummary {
        ProjectSummary {
            provider: ProviderKind::Modrinth,
            id: self.project_id,
            slug: self.slug,
            name: self.title,
            summary: self.description,
            downloads: self.downloads,
            icon_url: self.icon_url,
            categories: self.categories,
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
}

#[derive(Debug, Deserialize)]
pub struct CfLogo {
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct CfCategory {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct CfPagination {
    pub index: u32,
    #[serde(rename = "totalCount")]
    pub total_count: u32,
}

impl CfMod {
    pub fn into_summary(self) -> ProjectSummary {
        ProjectSummary {
            provider: ProviderKind::CurseForge,
            id: self.id.to_string(),
            slug: self.slug,
            name: self.name,
            summary: self.summary,
            downloads: self.download_count,
            icon_url: self.logo.map(|l| l.url),
            categories: self.categories.into_iter().map(|c| c.name).collect(),
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    //! No live HTTP in any test. All HTTP calls go through the injectable
    //! `ProviderHttpClient` seam. Tests supply a mock client backed by a
    //! pre-loaded response queue.

    use super::*;
    use std::collections::VecDeque;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    // ── Mock HTTP client ──────────────────────────────────────────────────────

    /// Canned response: HTTP status code + body string.
    struct MockResp(u16, String);

    impl MockResp {
        fn ok(body: impl Into<String>) -> Self {
            MockResp(200, body.into())
        }
    }

    /// Mock client backed by a VecDeque of pre-loaded `(status, body)` pairs.
    /// Each `get` call pops the next entry in FIFO order.
    struct MockProviderClient {
        responses: Arc<Mutex<VecDeque<MockResp>>>,
    }

    impl MockProviderClient {
        fn new(responses: Vec<MockResp>) -> Self {
            Self {
                responses: Arc::new(Mutex::new(responses.into_iter().collect())),
            }
        }

        async fn pop(&self) -> (u16, String) {
            let mut q = self.responses.lock().await;
            let MockResp(s, b) = q
                .pop_front()
                .expect("MockProviderClient: no more canned responses");
            (s, b)
        }
    }

    #[async_trait::async_trait]
    impl ProviderHttpClient for MockProviderClient {
        async fn get(
            &self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> Result<(u16, String), reqwest::Error> {
            Ok(self.pop().await)
        }
    }

    // ── Fixture loaders ───────────────────────────────────────────────────────

    const MODRINTH_SEARCH_FIXTURE: &str =
        include_str!("fixtures/modrinth_search.json");
    const CF_SEARCH_FIXTURE: &str =
        include_str!("fixtures/cf_search.json");

    fn modrinth_search_fixture() -> &'static str {
        MODRINTH_SEARCH_FIXTURE
    }

    fn cf_search_fixture() -> &'static str {
        CF_SEARCH_FIXTURE
    }

    // ── CF key resolution tests ───────────────────────────────────────────────

    #[test]
    fn cf_key_from_env_takes_priority_over_settings() {
        let result = cf_api_key_from(
            Some("env-key".to_string()),
            Some("settings-key".to_string()),
        );
        assert_eq!(result, Some("env-key".to_string()));
    }

    #[test]
    fn cf_key_falls_back_to_settings_when_env_blank() {
        let result = cf_api_key_from(Some("  ".to_string()), Some("settings-key".to_string()));
        assert_eq!(result, Some("settings-key".to_string()));
    }

    #[test]
    fn cf_key_falls_back_to_settings_when_env_absent() {
        let result = cf_api_key_from(None, Some("settings-key".to_string()));
        assert_eq!(result, Some("settings-key".to_string()));
    }

    #[test]
    fn cf_key_returns_none_when_both_absent() {
        let result = cf_api_key_from(None, None);
        assert!(result.is_none());
    }

    #[test]
    fn cf_key_returns_none_when_both_blank() {
        let result = cf_api_key_from(Some("".to_string()), Some("  ".to_string()));
        assert!(result.is_none());
    }

    #[test]
    fn cf_key_returns_none_when_only_blank_env() {
        let result = cf_api_key_from(Some("  \t  ".to_string()), None);
        assert!(result.is_none());
    }

    // ── Modrinth fixture deserialization + field mapping ──────────────────────

    #[test]
    fn modrinth_fixture_deserializes_hit_count() {
        let json = modrinth_search_fixture();
        let resp: MrSearchResponse = serde_json::from_str(&json).expect("parse failed");
        assert_eq!(resp.hits.len(), 2);
        assert_eq!(resp.total_hits, 2);
        assert_eq!(resp.offset, 0);
        assert_eq!(resp.limit, 20);
    }

    #[test]
    fn modrinth_fixture_first_hit_fields_map_to_summary() {
        let json = modrinth_search_fixture();
        let resp: MrSearchResponse = serde_json::from_str(&json).expect("parse failed");
        let summary = resp.hits.into_iter().next().unwrap().into_summary();

        assert_eq!(summary.provider, ProviderKind::Modrinth);
        assert_eq!(summary.id, "AANobbMI");
        assert_eq!(summary.slug, "sodium");
        assert_eq!(summary.name, "Sodium");
        assert_eq!(summary.downloads, 15_000_000);
        assert!(summary.icon_url.is_some());
        assert!(summary.categories.contains(&"optimization".to_string()));
    }

    #[test]
    fn modrinth_fixture_second_hit_no_icon_url_is_some() {
        // fabric-api fixture has an icon_url set
        let json = modrinth_search_fixture();
        let resp: MrSearchResponse = serde_json::from_str(&json).expect("parse failed");
        let summary: Vec<_> = resp.hits.into_iter().map(|h| h.into_summary()).collect();
        assert_eq!(summary[1].slug, "fabric-api");
        assert!(summary[1].icon_url.is_some());
    }

    // ── CurseForge fixture deserialization + field mapping ────────────────────

    #[test]
    fn cf_fixture_deserializes_mod_count() {
        let json = cf_search_fixture();
        let resp: CfSearchResponse = serde_json::from_str(&json).expect("parse failed");
        assert_eq!(resp.data.len(), 2);
        assert_eq!(resp.pagination.total_count, 2);
        assert_eq!(resp.pagination.index, 0);
    }

    #[test]
    fn cf_fixture_first_mod_fields_map_to_summary() {
        let json = cf_search_fixture();
        let resp: CfSearchResponse = serde_json::from_str(&json).expect("parse failed");
        let summary = resp.data.into_iter().next().unwrap().into_summary();

        assert_eq!(summary.provider, ProviderKind::CurseForge);
        assert_eq!(summary.id, "238222");
        assert_eq!(summary.slug, "jei");
        assert_eq!(summary.name, "Just Enough Items (JEI)");
        assert_eq!(summary.downloads, 300_000_000);
        assert!(summary.icon_url.is_some());
        assert!(summary.categories.contains(&"Map and Information".to_string()));
    }

    #[test]
    fn cf_fixture_second_mod_no_distribution_has_logo() {
        // optifine has allowModDistribution:false; that field is not part of ProjectSummary
        // but its logo should still be mapped
        let json = cf_search_fixture();
        let resp: CfSearchResponse = serde_json::from_str(&json).expect("parse failed");
        let summary: Vec<_> = resp.data.into_iter().map(|m| m.into_summary()).collect();
        assert_eq!(summary[1].slug, "optifine");
        assert!(summary[1].icon_url.is_some());
    }

    // ── Object safety: Box<dyn ModProvider> must compile ─────────────────────

    // This test is a compile-time assertion: if `ModProvider` is not object-safe,
    // the function below will fail to compile.
    fn _assert_mod_provider_object_safe(_: Box<dyn ModProvider>) {}

    // ── ProviderHttpClient seam: mock impl compiles and delivers responses ─────

    #[tokio::test]
    async fn mock_provider_client_delivers_canned_responses() {
        let client = MockProviderClient::new(vec![
            MockResp::ok(r#"{"hello":"world"}"#),
            MockResp::ok(r#"{"second":"response"}"#),
        ]);

        let (status1, body1) = client.get("https://example.com", &[]).await.unwrap();
        assert_eq!(status1, 200);
        assert_eq!(body1, r#"{"hello":"world"}"#);

        let (status2, body2) = client
            .get("https://example.com", &[("x-api-key", "test")])
            .await
            .unwrap();
        assert_eq!(status2, 200);
        assert_eq!(body2, r#"{"second":"response"}"#);
    }

    #[tokio::test]
    async fn mock_provider_client_returns_correct_status_code() {
        let client = MockProviderClient::new(vec![MockResp(404, "Not Found".to_string())]);
        let (status, body) = client.get("https://example.com", &[]).await.unwrap();
        assert_eq!(status, 404);
        assert_eq!(body, "Not Found");
    }

    // ── serde camelCase round-trip for IPC structs ────────────────────────────

    #[test]
    fn project_summary_serializes_to_camel_case() {
        let summary = ProjectSummary {
            provider: ProviderKind::Modrinth,
            id: "AANobbMI".to_string(),
            slug: "sodium".to_string(),
            name: "Sodium".to_string(),
            summary: "A rendering engine".to_string(),
            downloads: 1_000_000,
            icon_url: Some("https://example.com/icon.png".to_string()),
            categories: vec!["optimization".to_string()],
        };
        let json = serde_json::to_value(&summary).unwrap();
        assert!(json.get("iconUrl").is_some(), "iconUrl key missing");
        assert!(json.get("icon_url").is_none(), "snake_case key must not appear");
        assert!(json.get("provider").is_some());
        assert!(json.get("downloads").is_some());
    }

    #[test]
    fn version_file_null_url_round_trips() {
        let file = VersionFile {
            url: None,
            file_name: "optifine.jar".to_string(),
            size: Some(1_234_567),
            hashes: std::collections::HashMap::new(),
            primary: true,
        };
        let json = serde_json::to_value(&file).unwrap();
        // url field present and is null
        assert_eq!(json.get("url").unwrap(), &serde_json::Value::Null);
        let deserialized: VersionFile = serde_json::from_value(json).unwrap();
        assert!(deserialized.url.is_none());
    }

    #[test]
    fn search_params_serializes_to_camel_case() {
        let params = SearchParams {
            query: "sodium".to_string(),
            mc_version: Some("1.21".to_string()),
            loader: Some("fabric".to_string()),
            offset: 0,
            limit: 20,
        };
        let json = serde_json::to_value(&params).unwrap();
        assert!(json.get("mcVersion").is_some(), "mcVersion key missing");
        assert!(json.get("mc_version").is_none());
    }

    #[test]
    fn search_result_serializes_to_camel_case() {
        let result = SearchResult {
            hits: vec![],
            offset: 0,
            total: 42,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert!(json.get("hits").is_some());
        assert!(json.get("offset").is_some());
        assert!(json.get("total").is_some());
    }

    #[test]
    fn project_version_serializes_to_camel_case() {
        let version = ProjectVersion {
            provider: ProviderKind::CurseForge,
            id: "12345".to_string(),
            name: "JEI 13.0".to_string(),
            version_number: "13.0.0".to_string(),
            game_versions: vec!["1.20.1".to_string()],
            loaders: vec!["forge".to_string()],
            files: vec![],
            dependencies: vec![],
        };
        let json = serde_json::to_value(&version).unwrap();
        assert!(json.get("versionNumber").is_some(), "versionNumber missing");
        assert!(json.get("gameVersions").is_some(), "gameVersions missing");
        assert!(json.get("version_number").is_none());
    }

    #[test]
    fn provider_error_key_missing_display() {
        let err = ProviderError::KeyMissing;
        let msg = err.to_string();
        assert!(msg.contains("CurseForge"));
    }

    #[test]
    fn provider_error_http_status_display() {
        let err = ProviderError::HttpStatus {
            status: 403,
            body: "Forbidden".to_string(),
        };
        assert!(err.to_string().contains("403"));
    }
}
