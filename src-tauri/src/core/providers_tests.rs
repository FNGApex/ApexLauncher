//! Unit tests for `providers`. Extracted from the source module; wired back
//! via `#[cfg(test)] #[path = "providers_tests.rs"] mod tests;` so private items
//! remain accessible through `super::*`.

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

const MODRINTH_SEARCH_FIXTURE: &str = include_str!("fixtures/modrinth_search.json");
const CF_SEARCH_FIXTURE: &str = include_str!("fixtures/cf_search.json");

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
fn modrinth_fixture_second_hit_has_icon_url() {
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
    assert!(summary
        .categories
        .contains(&"Map and Information".to_string()));
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
    assert!(
        json.get("icon_url").is_none(),
        "snake_case key must not appear"
    );
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
