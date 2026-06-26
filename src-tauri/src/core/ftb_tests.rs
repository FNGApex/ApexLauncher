//! Unit tests for `ftb`. Wired via `#[cfg(test)] #[path = "ftb_tests.rs"] mod tests;`.
//! No live HTTP — a capturing mock client returns canned responses in order,
//! mirroring `curseforge_tests.rs`.

use super::*;
use crate::core::providers::{ProviderHttpClient, SearchParams};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::Mutex;

// ── Capturing mock HTTP client ─────────────────────────────────────────────

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
    async fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<(u16, String), reqwest::Error> {
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
    async fn post(&self, _url: &str, _headers: &[(&str, &str)], _body: String) -> Result<(u16, String), reqwest::Error> {
        panic!("FTB provider never POSTs");
    }
}

// ── Fixtures ───────────────────────────────────────────────────────────────

const FEATURED: &str = include_str!("fixtures/ftb_featured.json");
const POPULAR: &str = include_str!("fixtures/ftb_popular.json");
const SEARCH: &str = include_str!("fixtures/ftb_search.json");
const DETAIL: &str = include_str!("fixtures/ftb_modpack_detail.json");

fn search_params(query: &str) -> SearchParams {
    SearchParams {
        query: query.to_string(),
        mc_version: None,
        loader: None,
        loaders: vec![],
        categories: vec![],
        offset: 0,
        limit: 5,
        project_type: crate::core::providers::ProjectType::Modpack,
    }
}

// ── ProviderKind::Ftb serde ─────────────────────────────────────────────────

#[test]
fn provider_kind_ftb_serializes_lowercase() {
    let s = serde_json::to_string(&ProviderKind::Ftb).unwrap();
    assert_eq!(s, "\"ftb\"");
}

// ── search ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn search_empty_query_merges_featured_and_popular() {
    // featured [100,124] + popular [124,35] → deduped [100,124,35] → 3 details.
    let client = MockClient::new(vec![
        MockResp::ok(FEATURED),
        MockResp::ok(POPULAR),
        MockResp::ok(DETAIL),
        MockResp::ok(DETAIL),
        MockResp::ok(DETAIL),
    ]);
    let provider = FtbProvider::new();

    let result = provider.search(&client, &search_params("")).await.unwrap();

    assert_eq!(result.total, 3, "deduped id count");
    assert_eq!(result.hits.len(), 3);
    assert_eq!(result.hits[0].provider, ProviderKind::Ftb);
    assert_eq!(result.hits[0].name, "FTB Test Pack");
    assert_eq!(result.hits[0].downloads, 1100000);
    // icon = the square art, not the splash.
    assert_eq!(
        result.hits[0].icon_url.as_deref(),
        Some("https://example.invalid/square.png")
    );
    let urls = client.urls().await;
    assert!(urls[0].ends_with("/public/modpack/featured/5"), "url0: {}", urls[0]);
    assert!(urls[1].contains("/popular/installs/5"), "url1: {}", urls[1]);
}

#[tokio::test]
async fn search_term_hits_search_endpoint_and_ignores_cf_echo() {
    // search [105] (+ curseforge echo ignored) → 1 detail.
    let client = MockClient::new(vec![MockResp::ok(SEARCH), MockResp::ok(DETAIL)]);
    let provider = FtbProvider::new();

    let result = provider.search(&client, &search_params("fabric")).await.unwrap();

    assert_eq!(result.total, 1, "only FTB packs[], curseforge[] ignored");
    assert_eq!(result.hits.len(), 1);
    let urls = client.urls().await;
    assert!(urls[0].contains("/public/modpack/search/5?term=fabric"), "url: {}", urls[0]);
}

#[tokio::test]
async fn search_sends_user_agent_no_api_key() {
    let client = MockClient::new(vec![MockResp::ok(SEARCH), MockResp::ok(DETAIL)]);
    let provider = FtbProvider::new();
    provider.search(&client, &search_params("x")).await.unwrap();

    for headers in client.all_headers().await {
        assert!(
            headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("user-agent")),
            "User-Agent missing: {headers:?}"
        );
        assert!(
            !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("x-api-key")),
            "FTB must not send x-api-key: {headers:?}"
        );
    }
}

// ── get_versions ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn get_versions_maps_targets_and_empty_files() {
    let client = MockClient::new(vec![MockResp::ok(DETAIL)]);
    let provider = FtbProvider::new();

    let versions = provider.get_versions(&client, "100", None, None).await.unwrap();

    assert_eq!(versions.len(), 2);
    let v = &versions[0];
    assert_eq!(v.id, "2275");
    assert_eq!(v.name, "1.3.0");
    assert_eq!(v.game_versions, vec!["1.21.1"]);
    assert_eq!(v.loaders, vec!["neoforge"]);
    assert!(v.files.is_empty(), "FTB versions carry no single jar");
    assert_eq!(v.provider, ProviderKind::Ftb);
}

// ── get_project / get_pack_summary ────────────────────────────────────────────

#[tokio::test]
async fn get_project_returns_markdown_pack_info() {
    let client = MockClient::new(vec![MockResp::ok(DETAIL)]);
    let provider = FtbProvider::new();

    let info = provider.get_project(&client, "100").await.unwrap();

    assert_eq!(info.title, "FTB Test Pack");
    assert!(!info.body_is_html, "FTB descriptions are Markdown");
    assert!(info.description.contains("Markdown body"));
    assert_eq!(info.icon_url.as_deref(), Some("https://example.invalid/square.png"));
}

#[tokio::test]
async fn get_pack_summary_maps_author_and_tags() {
    let client = MockClient::new(vec![MockResp::ok(DETAIL)]);
    let provider = FtbProvider::new();

    let s = provider.get_pack_summary(&client, "100").await.unwrap();

    assert_eq!(s.name, "FTB Test Pack");
    assert_eq!(s.author.as_deref(), Some("FTB Team"));
    assert_eq!(s.summary.as_deref(), Some("A test FTB pack"));
    assert_eq!(s.categories, vec!["FTB Official", "Tech"]);
    assert_eq!(s.icon_url.as_deref(), Some("https://example.invalid/square.png"));
}

// ── get_projects_brief (no-op) ────────────────────────────────────────────────

#[tokio::test]
async fn get_projects_brief_is_noop_without_http() {
    let client = MockClient::new(vec![]); // no responses queued
    let provider = FtbProvider::new();

    let briefs = provider
        .get_projects_brief(&client, &["100".to_string()])
        .await
        .unwrap();

    assert!(briefs.is_empty());
    assert_eq!(client.request_count().await, 0, "no HTTP for the no-op");
}

// ── version manifest fetch + loader/mc helpers ────────────────────────────────

#[tokio::test]
async fn get_version_manifest_exposes_loader_and_mc() {
    let manifest_json = r#"{
        "name": "1.3.0",
        "type": "Release",
        "targets": [
            {"name":"neoforge","type":"modloader","version":"21.1.74"},
            {"name":"minecraft","type":"game","version":"1.21.1"}
        ],
        "specs": {"minimum":4096,"recommended":6144},
        "files": []
    }"#;
    let client = MockClient::new(vec![MockResp::ok(manifest_json)]);
    let provider = FtbProvider::new();

    let m = provider.get_version_manifest(&client, 100, 2275).await.unwrap();

    assert_eq!(m.loader().as_deref(), Some("neoforge"));
    assert_eq!(m.minecraft().as_deref(), Some("1.21.1"));
    assert_eq!(m.specs.unwrap().recommended, Some(6144));
    let urls = client.urls().await;
    assert!(urls[0].ends_with("/public/modpack/100/2275"), "url: {}", urls[0]);
}
