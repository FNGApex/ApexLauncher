//! Unit tests for `atl`. Wired via `#[cfg(test)] #[path = "atl_tests.rs"] mod tests;`.
//! No live HTTP — a capturing mock client returns canned responses in order,
//! mirroring `ftb_tests.rs` / `curseforge_tests.rs`.

use super::*;
use crate::core::providers::{ProjectType, ProviderHttpClient, ProviderKind, SearchParams};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::Mutex;

// ── Capturing mock HTTP client ─────────────────────────────────────────────────

struct MockResp(u16, String);
impl MockResp {
    fn ok(body: impl Into<String>) -> Self {
        MockResp(200, body.into())
    }
}

struct MockClient {
    responses: Arc<Mutex<VecDeque<MockResp>>>,
    captured: Arc<Mutex<Vec<(String, Vec<(String, String)>)>>>,
}

impl MockClient {
    fn new(responses: Vec<MockResp>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses.into_iter().collect())),
            captured: Arc::new(Mutex::new(Vec::new())),
        }
    }
    async fn urls(&self) -> Vec<String> {
        self.captured.lock().await.iter().map(|(u, _)| u.clone()).collect()
    }
    async fn request_count(&self) -> usize {
        self.captured.lock().await.len()
    }
    async fn all_headers(&self) -> Vec<Vec<(String, String)>> {
        self.captured.lock().await.iter().map(|(_, h)| h.clone()).collect()
    }
}

#[async_trait::async_trait]
impl ProviderHttpClient for MockClient {
    async fn get(
        &self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<(u16, String), reqwest::Error> {
        self.captured.lock().await.push((
            url.to_string(),
            headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        ));
        let MockResp(s, b) = self
            .responses
            .lock()
            .await
            .pop_front()
            .expect("MockClient: no more canned responses");
        Ok((s, b))
    }
    async fn post(
        &self,
        _url: &str,
        _headers: &[(&str, &str)],
        _body: String,
    ) -> Result<(u16, String), reqwest::Error> {
        panic!("AtlProvider never POSTs");
    }
}

// ── Fixtures ──────────────────────────────────────────────────────────────────

const PACKS_PUBLIC: &str = include_str!("fixtures/atl_packs_public.json");
const PACK_DETAIL: &str = include_str!("fixtures/atl_pack_detail.json");
const PACK_LATEST: &str = include_str!("fixtures/atl_pack_latest.json");

fn search_params(query: &str) -> SearchParams {
    SearchParams {
        query: query.to_string(),
        mc_version: None,
        loader: None,
        loaders: vec![],
        categories: vec![],
        offset: 0,
        limit: 10,
        project_type: ProjectType::Modpack,
    }
}

// ── cdn_image_url ─────────────────────────────────────────────────────────────

#[test]
fn cdn_image_url_lowercases_safe_name() {
    let url = cdn_image_url("SkyFactory4");
    assert!(url.ends_with("skyfactory4.png"), "url: {url}");
    assert!(url.starts_with(CDN), "must use CDN base: {url}");
}

#[test]
fn cdn_image_url_already_lowercase_unchanged() {
    let url = cdn_image_url("alltheforge10");
    assert!(url.ends_with("alltheforge10.png"), "url: {url}");
}

// ── search ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn search_makes_exactly_one_http_call() {
    let client = MockClient::new(vec![MockResp::ok(PACKS_PUBLIC)]);
    let provider = AtlProvider::new();

    provider.search(&client, &search_params("")).await.unwrap();

    assert_eq!(client.request_count().await, 1, "must not make N+1 calls");
    let urls = client.urls().await;
    assert!(urls[0].contains("/packs/full/public"), "url: {}", urls[0]);
}

#[tokio::test]
async fn search_empty_query_returns_all_sorted_by_name() {
    let client = MockClient::new(vec![MockResp::ok(PACKS_PUBLIC)]);
    let provider = AtlProvider::new();

    let result = provider.search(&client, &search_params("")).await.unwrap();

    // Fixture order: SkyFactory4, AllTheForge10, Direwolf201_20 (non-alphabetical).
    // After sort by name: "All The Forge 10", "Direwolf20 1.20", "Sky Factory 4".
    assert_eq!(result.total, 3);
    assert_eq!(result.hits.len(), 3);
    assert_eq!(result.hits[0].name, "All The Forge 10");
    assert_eq!(result.hits[1].name, "Direwolf20 1.20");
    assert_eq!(result.hits[2].name, "Sky Factory 4");
}

#[tokio::test]
async fn search_term_filters_case_insensitive() {
    let client = MockClient::new(vec![MockResp::ok(PACKS_PUBLIC)]);
    let provider = AtlProvider::new();

    // "FORGE" matches "All The Forge 10" only (case-insensitive substring).
    let mut params = search_params("FORGE");
    params.limit = 10;
    let result = provider.search(&client, &params).await.unwrap();

    assert_eq!(result.total, 1, "only 'All The Forge 10' matches 'FORGE'");
    assert_eq!(result.hits[0].name, "All The Forge 10");
}

#[tokio::test]
async fn search_offset_limit_windows_results() {
    let client = MockClient::new(vec![MockResp::ok(PACKS_PUBLIC)]);
    let provider = AtlProvider::new();

    // Sorted: [AllTheForge10, Direwolf201_20, SkyFactory4].
    // offset=1, limit=1 → just "Direwolf20 1.20".
    let mut params = search_params("");
    params.offset = 1;
    params.limit = 1;
    let result = provider.search(&client, &params).await.unwrap();

    assert_eq!(result.total, 3, "total = full filtered count, not page count");
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].name, "Direwolf20 1.20");
    assert_eq!(result.offset, 1);
}

#[tokio::test]
async fn search_maps_to_project_summary_fields() {
    let client = MockClient::new(vec![MockResp::ok(PACKS_PUBLIC)]);
    let provider = AtlProvider::new();

    let result = provider.search(&client, &search_params("forge")).await.unwrap();
    let hit = &result.hits[0];

    // id and slug are safeName (not the human-readable name).
    assert_eq!(hit.id, "AllTheForge10");
    assert_eq!(hit.slug, "AllTheForge10");
    assert_eq!(hit.provider, ProviderKind::Atlauncher);
    // ATL API does not expose download counts.
    assert_eq!(hit.downloads, 0);
    // Icon URL is deterministic from safeName (lowercased).
    assert_eq!(
        hit.icon_url.as_deref(),
        Some("https://download.nodecdn.net/containers/atl/launcher/images/alltheforge10.png")
    );
    // Page URL uses safeName verbatim (not lowercased).
    assert_eq!(
        hit.page_url.as_deref(),
        Some("https://atlauncher.com/pack/AllTheForge10")
    );
    // Summary is the first line of description only.
    assert_eq!(hit.summary, "A Forge kitchen-sink modpack.");
}

#[tokio::test]
async fn search_envelope_error_returns_provider_error() {
    let error_json = r#"{"error":true,"code":500,"message":"Internal server error","data":null}"#;
    let client = MockClient::new(vec![MockResp::ok(error_json)]);
    let provider = AtlProvider::new();

    let err = provider.search(&client, &search_params("")).await.unwrap_err();

    assert!(
        matches!(err, ProviderError::BadResponse(_)),
        "expected BadResponse for API-level error, got: {err:?}"
    );
}

#[tokio::test]
async fn search_sends_user_agent_on_every_request() {
    let client = MockClient::new(vec![MockResp::ok(PACKS_PUBLIC)]);
    let provider = AtlProvider::new();
    provider.search(&client, &search_params("")).await.unwrap();

    for headers in client.all_headers().await {
        assert!(
            headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("user-agent")),
            "User-Agent missing: {headers:?}"
        );
        // ATL is keyless — must NOT send an x-api-key header.
        assert!(
            !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("x-api-key")),
            "ATL must not send x-api-key: {headers:?}"
        );
    }
}

// ── get_versions ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn get_versions_maps_fields_and_has_empty_loaders_and_files() {
    let client = MockClient::new(vec![MockResp::ok(PACK_DETAIL)]);
    let provider = AtlProvider::new();

    let versions = provider
        .get_versions(&client, "AllTheForge10", None, None)
        .await
        .unwrap();

    assert_eq!(versions.len(), 2, "fixture has 2 versions");
    let v = &versions[0];
    assert_eq!(v.id, "1.2.3");
    assert_eq!(v.name, "1.2.3");
    assert_eq!(v.version_number, "1.2.3");
    assert_eq!(v.game_versions, vec!["1.20.1"]);
    // Loaders and files are empty — only known from Configs.json (fetched at install time).
    assert!(v.loaders.is_empty(), "ATL versions carry no loader; only known from Configs.json");
    assert!(v.files.is_empty(), "ATL versions carry no single jar");
    assert_eq!(v.provider, ProviderKind::Atlauncher);
    // Must hit the right URL.
    let urls = client.urls().await;
    assert!(urls[0].ends_with("/pack/AllTheForge10"), "url: {}", urls[0]);
}

#[tokio::test]
async fn get_versions_empty_minecraft_yields_empty_game_versions() {
    // A version entry whose `minecraft` field is empty must map to an empty
    // `game_versions` vec (not `[""]`).
    let body = r#"{"error":false,"code":200,"message":"","data":{
        "safeName":"NoMc","name":"No MC","description":"x",
        "versions":[{"version":"9.9.9","minecraft":""}]}}"#;
    let client = MockClient::new(vec![MockResp::ok(body)]);
    let provider = AtlProvider::new();

    let versions = provider.get_versions(&client, "NoMc", None, None).await.unwrap();

    assert_eq!(versions.len(), 1);
    assert!(
        versions[0].game_versions.is_empty(),
        "empty minecraft must yield [], got {:?}",
        versions[0].game_versions
    );
}

// ── URL builders: version segment percent-encoding ────────────────────────────

#[test]
fn version_url_builders_percent_encode_the_version_segment() {
    // A version string with reserved characters (space, slash) must be encoded
    // so it cannot break the URL path.
    let ver = "v1 /beta";
    assert!(
        cdn_configs_url("Pack", ver).ends_with("/versions/v1%20%2Fbeta/Configs.json"),
        "got {}",
        cdn_configs_url("Pack", ver)
    );
    assert!(
        cdn_configs_zip_url("Pack", ver).ends_with("/versions/v1%20%2Fbeta/Configs.zip"),
        "got {}",
        cdn_configs_zip_url("Pack", ver)
    );
    assert!(
        api_pack_version_url("Pack", ver).ends_with("/pack/Pack/v1%20%2Fbeta"),
        "got {}",
        api_pack_version_url("Pack", ver)
    );
    // A normal version (dots only — unreserved) is left intact.
    assert!(cdn_configs_url("Pack", "v11.2.1hf").contains("/versions/v11.2.1hf/"));
}

// ── get_project / get_pack_summary ────────────────────────────────────────────

#[tokio::test]
async fn get_project_returns_plain_text_pack_info() {
    let client = MockClient::new(vec![MockResp::ok(PACK_DETAIL)]);
    let provider = AtlProvider::new();

    let info = provider.get_project(&client, "AllTheForge10").await.unwrap();

    assert_eq!(info.title, "All The Forge 10");
    // ATL descriptions are plain text (not Markdown or HTML).
    assert!(!info.body_is_html, "ATL descriptions are not HTML");
    assert!(info.description.contains("A Forge kitchen-sink modpack."));
    assert_eq!(
        info.icon_url.as_deref(),
        Some("https://download.nodecdn.net/containers/atl/launcher/images/alltheforge10.png")
    );
}

#[tokio::test]
async fn get_pack_summary_maps_name_icon_and_first_line_summary() {
    let client = MockClient::new(vec![MockResp::ok(PACK_DETAIL)]);
    let provider = AtlProvider::new();

    let s = provider.get_pack_summary(&client, "AllTheForge10").await.unwrap();

    assert_eq!(s.name, "All The Forge 10");
    assert_eq!(
        s.icon_url.as_deref(),
        Some("https://download.nodecdn.net/containers/atl/launcher/images/alltheforge10.png")
    );
    // ATL API does not expose author data.
    assert!(s.author.is_none(), "ATL has no author field in its API");
    // First line of description only.
    assert_eq!(s.summary.as_deref(), Some("A Forge kitchen-sink modpack."));
    assert!(s.categories.is_empty(), "ATL has no category API");
}

// ── get_projects_brief (no-op) ────────────────────────────────────────────────

#[tokio::test]
async fn get_projects_brief_is_noop_without_http() {
    let client = MockClient::new(vec![]); // no responses queued
    let provider = AtlProvider::new();

    let briefs = provider
        .get_projects_brief(&client, &["AllTheForge10".to_string()])
        .await
        .unwrap();

    assert!(briefs.is_empty());
    assert_eq!(client.request_count().await, 0, "no HTTP for the no-op");
}

// ── newest_version ────────────────────────────────────────────────────────────

#[tokio::test]
async fn newest_version_returns_version_string() {
    let client = MockClient::new(vec![MockResp::ok(PACK_LATEST)]);
    let provider = AtlProvider::new();

    let ver = provider.newest_version(&client, "AllTheForge10").await.unwrap();

    assert_eq!(ver.as_deref(), Some("1.2.3"));
    let urls = client.urls().await;
    assert!(urls[0].ends_with("/pack/AllTheForge10/latest"), "url: {}", urls[0]);
}

// ── AtlMod.client default ─────────────────────────────────────────────────────

#[test]
fn atl_mod_client_defaults_to_true_when_field_absent() {
    // Absence of `client` means it is client-side (the common case).
    let json = r#"{"name":"TestMod","file":"test.jar","download":"server"}"#;
    let m: AtlMod = serde_json::from_str(json).unwrap();
    assert!(
        m.client,
        "client should default to true — server-only mods set it explicitly to false"
    );
}

#[test]
fn atl_mod_client_false_when_explicitly_set() {
    let json = r#"{"name":"ServerMod","file":"server.jar","download":"server","client":false}"#;
    let m: AtlMod = serde_json::from_str(json).unwrap();
    assert!(!m.client);
}
