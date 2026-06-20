//! Unit tests for `modrinth`. Extracted from the source module; wired back
//! via `#[cfg(test)] #[path = "modrinth_tests.rs"] mod tests;` so private items
//! remain accessible through `super::*`.

//! No live HTTP in any test. All HTTP calls use a capturing mock client backed
//! by a pre-loaded response queue.

use super::*;
use crate::core::providers::{ProviderError, ProviderHttpClient, ProjectType, SearchParams};
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

/// Mock HTTP client that records every request URL and header set.
struct CapturingMockClient {
    responses: Arc<Mutex<VecDeque<MockResp>>>,
    /// Captured (url, headers) pairs in call order.
    captured: Arc<Mutex<Vec<(String, Vec<(String, String)>)>>>,
}

impl CapturingMockClient {
    fn new(responses: Vec<MockResp>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses.into_iter().collect())),
            captured: Arc::new(Mutex::new(Vec::new())),
        }
    }

    async fn captured_urls(&self) -> Vec<String> {
        self.captured
            .lock()
            .await
            .iter()
            .map(|(url, _)| url.clone())
            .collect()
    }

    async fn captured_headers(&self) -> Vec<Vec<(String, String)>> {
        self.captured
            .lock()
            .await
            .iter()
            .map(|(_, headers)| headers.clone())
            .collect()
    }
}

#[async_trait::async_trait]
impl ProviderHttpClient for CapturingMockClient {
    async fn get(
        &self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<(u16, String), reqwest::Error> {
        // Record the call.
        {
            let mut cap = self.captured.lock().await;
            cap.push((
                url.to_string(),
                headers
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            ));
        }
        // Pop next canned response.
        let mut q = self.responses.lock().await;
        let MockResp(s, b) = q
            .pop_front()
            .expect("CapturingMockClient: no more canned responses");
        Ok((s, b))
    }

    async fn post(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        _body: String,
    ) -> Result<(u16, String), reqwest::Error> {
        // Record the call (same structure as GET).
        {
            let mut cap = self.captured.lock().await;
            cap.push((
                url.to_string(),
                headers
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            ));
        }
        let mut q = self.responses.lock().await;
        let MockResp(s, b) = q
            .pop_front()
            .expect("CapturingMockClient: no more canned responses");
        Ok((s, b))
    }
}

// ── Fixtures ───────────────────────────────────────────────────────────────

const MODRINTH_SEARCH_FIXTURE: &str = include_str!("fixtures/modrinth_search.json");
const MODRINTH_VERSIONS_FIXTURE: &str = include_str!("fixtures/modrinth_versions.json");
const MODRINTH_PROJECTS_BATCH_FIXTURE: &str = include_str!("fixtures/modrinth_projects_batch.json");

// ── URL construction ───────────────────────────────────────────────────────

#[test]
fn search_url_includes_query_facets_offset_limit() {
    let params = SearchParams {
        query: "sodium".to_string(),
        mc_version: Some("1.21".to_string()),
        loader: Some("fabric".to_string()),
        offset: 0,
        limit: 20,
        project_type: ProjectType::Mod,
    };
    let url = ModrinthProvider::build_search_url(&params);
    assert!(url.contains("query=sodium"), "query missing: {url}");
    assert!(url.contains("offset=0"), "offset missing: {url}");
    assert!(url.contains("limit=20"), "limit missing: {url}");
    // facets must encode project_type:mod, versions:1.21, categories:fabric
    let decoded = percent_decode(&url);
    assert!(
        decoded.contains("project_type:mod"),
        "project_type:mod missing: {decoded}"
    );
    assert!(
        decoded.contains("versions:1.21"),
        "versions:1.21 missing: {decoded}"
    );
    assert!(
        decoded.contains("categories:fabric"),
        "categories:fabric missing: {decoded}"
    );
}

#[test]
fn search_url_no_filters_still_includes_project_type_mod() {
    let params = SearchParams {
        query: "".to_string(),
        mc_version: None,
        loader: None,
        offset: 20,
        limit: 10,
        project_type: ProjectType::Mod,
    };
    let url = ModrinthProvider::build_search_url(&params);
    let decoded = percent_decode(&url);
    assert!(
        decoded.contains("project_type:mod"),
        "project_type:mod missing: {decoded}"
    );
    assert!(
        !decoded.contains("versions:"),
        "unexpected versions facet: {decoded}"
    );
    assert!(url.contains("offset=20"), "offset=20 missing: {url}");
    assert!(url.contains("limit=10"), "limit=10 missing: {url}");
}

#[test]
fn versions_url_with_both_filters() {
    let url = ModrinthProvider::build_versions_url("AANobbMI", Some("1.21"), Some("fabric"));
    assert!(
        url.contains("/project/AANobbMI/version"),
        "path missing: {url}"
    );
    assert!(url.contains("1.21"), "mc version missing: {url}");
    assert!(url.contains("fabric"), "loader missing: {url}");
}

#[test]
fn versions_url_no_filters_has_no_query_string() {
    let url = ModrinthProvider::build_versions_url("AANobbMI", None, None);
    assert_eq!(url, "https://api.modrinth.com/v2/project/AANobbMI/version");
}

// ── search: fixture → correct ProjectSummary count + field mapping ─────────

#[tokio::test]
async fn search_returns_correct_project_summary_count_and_fields() {
    let client = CapturingMockClient::new(vec![MockResp::ok(MODRINTH_SEARCH_FIXTURE)]);
    let provider = ModrinthProvider;
    let params = SearchParams {
        query: "sodium".to_string(),
        mc_version: Some("1.21".to_string()),
        loader: Some("fabric".to_string()),
        offset: 0,
        limit: 20,
        project_type: ProjectType::Mod,
    };

    let result = provider.search(&client, &params).await.unwrap();

    assert_eq!(result.hits.len(), 2, "should return 2 hits from fixture");
    assert_eq!(result.total, 2);
    assert_eq!(result.offset, 0);

    let sodium = &result.hits[0];
    assert_eq!(sodium.provider, ProviderKind::Modrinth);
    assert_eq!(sodium.id, "AANobbMI");
    assert_eq!(sodium.slug, "sodium");
    assert_eq!(sodium.name, "Sodium");
    assert_eq!(sodium.downloads, 15_000_000);
    assert!(sodium.icon_url.is_some());
    assert!(sodium.categories.contains(&"optimization".to_string()));
}

#[tokio::test]
async fn search_request_url_contains_facets_query_offset_limit() {
    let client = CapturingMockClient::new(vec![MockResp::ok(MODRINTH_SEARCH_FIXTURE)]);
    let provider = ModrinthProvider;
    let params = SearchParams {
        query: "sodium".to_string(),
        mc_version: Some("1.21".to_string()),
        loader: Some("fabric".to_string()),
        offset: 0,
        limit: 20,
        project_type: ProjectType::Mod,
    };

    provider.search(&client, &params).await.unwrap();

    let urls = client.captured_urls().await;
    assert_eq!(urls.len(), 1);
    let raw_url = &urls[0];
    let decoded = percent_decode(raw_url);

    assert!(
        raw_url.contains("query=sodium"),
        "query param missing: {raw_url}"
    );
    assert!(raw_url.contains("offset=0"), "offset missing: {raw_url}");
    assert!(raw_url.contains("limit=20"), "limit missing: {raw_url}");
    assert!(
        decoded.contains("project_type:mod"),
        "project_type:mod facet missing: {decoded}"
    );
    assert!(
        decoded.contains("versions:1.21"),
        "versions facet missing: {decoded}"
    );
    assert!(
        decoded.contains("categories:fabric"),
        "categories facet missing: {decoded}"
    );
}

#[tokio::test]
async fn search_request_carries_user_agent_header() {
    let client = CapturingMockClient::new(vec![MockResp::ok(MODRINTH_SEARCH_FIXTURE)]);
    let provider = ModrinthProvider;
    let params = SearchParams {
        query: "test".to_string(),
        mc_version: None,
        loader: None,
        offset: 0,
        limit: 10,
        project_type: ProjectType::Mod,
    };

    provider.search(&client, &params).await.unwrap();

    let all_headers = client.captured_headers().await;
    let headers = &all_headers[0];
    let has_ua = headers
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case("User-Agent") && v.contains("modloader/"));
    assert!(has_ua, "User-Agent header missing or wrong: {:?}", headers);
}

#[tokio::test]
async fn search_returns_provider_error_on_non_200() {
    let client = CapturingMockClient::new(vec![MockResp(429, "rate limited".to_string())]);
    let provider = ModrinthProvider;
    let params = SearchParams {
        query: "test".to_string(),
        mc_version: None,
        loader: None,
        offset: 0,
        limit: 10,
        project_type: ProjectType::Mod,
    };

    let err = provider.search(&client, &params).await.unwrap_err();
    assert!(
        matches!(err, ProviderError::HttpStatus { status: 429, .. }),
        "expected HttpStatus(429), got {:?}",
        err
    );
}

// ── get_versions: fixture → filter by mc_version + loader ─────────────────

#[tokio::test]
async fn get_versions_returns_only_compatible_entries() {
    // Fixture has 3 entries:
    //   AABBCC11 — 1.21, [fabric, quilt]   ← should match (fabric + 1.21)
    //   AABBCC22 — 1.20.1, [fabric]         ← filtered out (wrong mc version)
    //   AABBCC33 — 1.21, [neoforge]         ← filtered out (wrong loader)
    let client = CapturingMockClient::new(vec![MockResp::ok(MODRINTH_VERSIONS_FIXTURE)]);
    let provider = ModrinthProvider;

    let versions = provider
        .get_versions(&client, "AANobbMI", Some("1.21"), Some("fabric"))
        .await
        .unwrap();

    assert_eq!(
        versions.len(),
        1,
        "only 1 version should pass filters; got {:?}",
        versions.iter().map(|v| &v.id).collect::<Vec<_>>()
    );
    let v = &versions[0];
    assert_eq!(v.id, "AABBCC11");
    assert_eq!(v.provider, ProviderKind::Modrinth);
    assert!(v.game_versions.contains(&"1.21".to_string()));
    assert!(v.loaders.contains(&"fabric".to_string()));
}

#[tokio::test]
async fn get_versions_no_filter_returns_all_entries() {
    let client = CapturingMockClient::new(vec![MockResp::ok(MODRINTH_VERSIONS_FIXTURE)]);
    let provider = ModrinthProvider;

    let versions = provider
        .get_versions(&client, "AANobbMI", None, None)
        .await
        .unwrap();

    assert_eq!(
        versions.len(),
        3,
        "no filters → all 3 versions from fixture"
    );
}

#[tokio::test]
async fn get_versions_mc_only_filter() {
    // Filter by mc_version=1.21 only — should match entries 1 (fabric+quilt) and 3 (neoforge).
    let client = CapturingMockClient::new(vec![MockResp::ok(MODRINTH_VERSIONS_FIXTURE)]);
    let provider = ModrinthProvider;

    let versions = provider
        .get_versions(&client, "AANobbMI", Some("1.21"), None)
        .await
        .unwrap();

    assert_eq!(versions.len(), 2, "two 1.21-compatible versions expected");
    let ids: Vec<&str> = versions.iter().map(|v| v.id.as_str()).collect();
    assert!(ids.contains(&"AABBCC11"), "AABBCC11 missing: {:?}", ids);
    assert!(ids.contains(&"AABBCC33"), "AABBCC33 missing: {:?}", ids);
}

// ── get_versions: VersionFile.hashes carries sha1 + sha512 ───────────────

#[tokio::test]
async fn version_file_hashes_contain_sha1_and_sha512() {
    let client = CapturingMockClient::new(vec![MockResp::ok(MODRINTH_VERSIONS_FIXTURE)]);
    let provider = ModrinthProvider;

    let versions = provider
        .get_versions(&client, "AANobbMI", None, None)
        .await
        .unwrap();

    for version in &versions {
        for file in &version.files {
            assert!(
                file.hashes.contains_key("sha1"),
                "sha1 missing in version {} file {}",
                version.id,
                file.file_name
            );
            assert!(
                file.hashes.contains_key("sha512"),
                "sha512 missing in version {} file {}",
                version.id,
                file.file_name
            );
        }
    }
}

#[tokio::test]
async fn version_file_url_is_some_for_modrinth() {
    let client = CapturingMockClient::new(vec![MockResp::ok(MODRINTH_VERSIONS_FIXTURE)]);
    let provider = ModrinthProvider;

    let versions = provider
        .get_versions(&client, "AANobbMI", None, None)
        .await
        .unwrap();

    for version in &versions {
        for file in &version.files {
            assert!(
                file.url.is_some(),
                "Modrinth version files should always have a URL, but {} is None",
                file.file_name
            );
        }
    }
}

#[tokio::test]
async fn get_versions_request_url_contains_project_id() {
    let client = CapturingMockClient::new(vec![MockResp::ok(MODRINTH_VERSIONS_FIXTURE)]);
    let provider = ModrinthProvider;

    provider
        .get_versions(&client, "AANobbMI", Some("1.21"), Some("fabric"))
        .await
        .unwrap();

    let urls = client.captured_urls().await;
    assert_eq!(urls.len(), 1);
    assert!(
        urls[0].contains("/project/AANobbMI/version"),
        "project path missing: {}",
        urls[0]
    );
}

#[tokio::test]
async fn get_versions_returns_provider_error_on_non_200() {
    let client = CapturingMockClient::new(vec![MockResp(404, "not found".to_string())]);
    let provider = ModrinthProvider;

    let err = provider
        .get_versions(&client, "INVALID", None, None)
        .await
        .unwrap_err();
    assert!(
        matches!(err, ProviderError::HttpStatus { status: 404, .. }),
        "expected HttpStatus(404), got {:?}",
        err
    );
}

// ── get_project: fixture → PackInfo ───────────────────────────────────────

const MODRINTH_PROJECT_FIXTURE: &str = include_str!("fixtures/modrinth_project_sodium.json");

#[tokio::test]
async fn get_project_maps_fixture_to_pack_info() {
    let client = CapturingMockClient::new(vec![MockResp::ok(MODRINTH_PROJECT_FIXTURE)]);
    let provider = ModrinthProvider;

    let info = provider.get_project(&client, "AANobbMI").await.unwrap();

    assert_eq!(info.title, "Sodium");
    assert!(
        info.description.contains("# Sodium"),
        "description should be the Markdown body, got: {:?}",
        &info.description[..50.min(info.description.len())]
    );
    assert_eq!(
        info.icon_url,
        Some("https://cdn.modrinth.com/data/AANobbMI/icon.png".to_string())
    );
    assert!(!info.body_is_html, "Modrinth body is Markdown, not HTML");
}

#[tokio::test]
async fn get_project_url_targets_project_endpoint() {
    let client = CapturingMockClient::new(vec![MockResp::ok(MODRINTH_PROJECT_FIXTURE)]);
    let provider = ModrinthProvider;

    provider.get_project(&client, "AANobbMI").await.unwrap();

    let urls = client.captured_urls().await;
    assert_eq!(urls.len(), 1);
    assert!(
        urls[0].ends_with("/v2/project/AANobbMI"),
        "unexpected url: {}",
        urls[0]
    );
}

#[tokio::test]
async fn get_project_request_carries_user_agent_header() {
    let client = CapturingMockClient::new(vec![MockResp::ok(MODRINTH_PROJECT_FIXTURE)]);
    let provider = ModrinthProvider;

    provider.get_project(&client, "AANobbMI").await.unwrap();

    let all_headers = client.captured_headers().await;
    let headers = &all_headers[0];
    let has_ua = headers
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case("User-Agent") && v.contains("modloader/"));
    assert!(has_ua, "User-Agent header missing or wrong: {:?}", headers);
}

#[tokio::test]
async fn get_project_returns_http_error_on_non_200() {
    let client = CapturingMockClient::new(vec![MockResp(404, "not found".to_string())]);
    let provider = ModrinthProvider;

    let err = provider
        .get_project(&client, "INVALID")
        .await
        .unwrap_err();
    assert!(
        matches!(err, ProviderError::HttpStatus { status: 404, .. }),
        "expected HttpStatus(404), got {:?}",
        err
    );
}

// ── project_type selector: facet switching ────────────────────────────────

#[test]
fn search_url_with_project_type_mod_includes_mod_facet() {
    let params = SearchParams {
        query: "sodium".to_string(),
        mc_version: None,
        loader: None,
        offset: 0,
        limit: 20,
        project_type: ProjectType::Mod,
    };
    let url = ModrinthProvider::build_search_url(&params);
    let decoded = percent_decode(&url);
    assert!(
        decoded.contains("project_type:mod"),
        "project_type:mod facet missing: {decoded}"
    );
    assert!(
        !decoded.contains("project_type:modpack"),
        "project_type:modpack should not appear: {decoded}"
    );
}

#[test]
fn search_url_with_project_type_modpack_includes_modpack_facet() {
    let params = SearchParams {
        query: "all the mods".to_string(),
        mc_version: None,
        loader: None,
        offset: 0,
        limit: 20,
        project_type: ProjectType::Modpack,
    };
    let url = ModrinthProvider::build_search_url(&params);
    let decoded = percent_decode(&url);
    assert!(
        decoded.contains("project_type:modpack"),
        "project_type:modpack facet missing: {decoded}"
    );
    // The trailing `"` is intentional: `project_type:modpack` contains the
    // substring `project_type:mod`, so we must match the closing quote to
    // distinguish the two facet values.
    assert!(
        !decoded.contains("project_type:mod\""),
        "project_type:mod should not appear when modpack selected: {decoded}"
    );
}

// ── page_url: populated from hit's project_type + slug ────────────────────

#[tokio::test]
async fn search_populates_page_url_for_mod_hits() {
    let client = CapturingMockClient::new(vec![MockResp::ok(MODRINTH_SEARCH_FIXTURE)]);
    let provider = ModrinthProvider;
    let params = SearchParams {
        query: "sodium".to_string(),
        mc_version: None,
        loader: None,
        offset: 0,
        limit: 20,
        project_type: ProjectType::Mod,
    };

    let result = provider.search(&client, &params).await.unwrap();
    let sodium = &result.hits[0];

    // Fixture hit has project_type: "mod" and slug: "sodium"
    assert_eq!(
        sodium.page_url,
        Some("https://modrinth.com/mod/sodium".to_string()),
        "page_url mismatch: {:?}",
        sodium.page_url
    );
}

#[tokio::test]
async fn search_populates_page_url_using_response_project_type_not_selector() {
    // The fixture hits have project_type: "mod" in the response.
    // Even if the selector param says "modpack", the page_url is derived from
    // the actual hit's project_type field (response-driven, not selector-driven).
    let client = CapturingMockClient::new(vec![MockResp::ok(MODRINTH_SEARCH_FIXTURE)]);
    let provider = ModrinthProvider;
    let params = SearchParams {
        query: "sodium".to_string(),
        mc_version: None,
        loader: None,
        offset: 0,
        limit: 20,
        project_type: ProjectType::Modpack,
    };

    let result = provider.search(&client, &params).await.unwrap();
    let sodium = &result.hits[0];

    // Response project_type is "mod" → page_url must use "mod" segment
    assert_eq!(
        sodium.page_url,
        Some("https://modrinth.com/mod/sodium".to_string()),
        "page_url must derive from response project_type, not selector: {:?}",
        sodium.page_url
    );
}

// ── get_projects_brief: batch metadata fetch (MM-B2) ──────────────────────

#[tokio::test]
async fn get_projects_brief_returns_briefs_for_all_ids() {
    let client = CapturingMockClient::new(vec![MockResp::ok(MODRINTH_PROJECTS_BATCH_FIXTURE)]);
    let provider = ModrinthProvider;

    let ids = vec!["AANobbMI".to_string(), "gvQqBUqZ".to_string()];
    let briefs = provider.get_projects_brief(&client, &ids).await.unwrap();

    assert_eq!(briefs.len(), 2, "should return 2 briefs from fixture");
    let sodium = briefs.iter().find(|b| b.project_id == "AANobbMI").unwrap();
    assert_eq!(sodium.name, "Sodium");
    assert_eq!(
        sodium.icon_url,
        Some("https://cdn.modrinth.com/data/AANobbMI/icon.png".to_string())
    );
    assert_eq!(
        sodium.summary,
        "A modern rendering engine and client-side optimization mod for Minecraft."
    );

    let lithium = briefs.iter().find(|b| b.project_id == "gvQqBUqZ").unwrap();
    assert_eq!(lithium.name, "Lithium");
    assert!(lithium.icon_url.is_some());
}

#[tokio::test]
async fn get_projects_brief_issues_one_get_call() {
    let client = CapturingMockClient::new(vec![MockResp::ok(MODRINTH_PROJECTS_BATCH_FIXTURE)]);
    let provider = ModrinthProvider;

    let ids = vec!["AANobbMI".to_string(), "gvQqBUqZ".to_string()];
    provider.get_projects_brief(&client, &ids).await.unwrap();

    let urls = client.captured_urls().await;
    assert_eq!(urls.len(), 1, "must issue exactly ONE HTTP call; got {:?}", urls);
    assert!(
        urls[0].contains("/v2/projects"),
        "URL should target /v2/projects: {}",
        urls[0]
    );
}

#[tokio::test]
async fn get_projects_brief_url_contains_encoded_ids() {
    let client = CapturingMockClient::new(vec![MockResp::ok(MODRINTH_PROJECTS_BATCH_FIXTURE)]);
    let provider = ModrinthProvider;

    let ids = vec!["AANobbMI".to_string(), "gvQqBUqZ".to_string()];
    provider.get_projects_brief(&client, &ids).await.unwrap();

    let urls = client.captured_urls().await;
    // The URL must contain the ids JSON-encoded and percent-encoded.
    // Decoded it should contain both ids.
    let decoded_url = urls[0]
        .replace("%22", "\"")
        .replace("%5B", "[")
        .replace("%5D", "]")
        .replace("%2C", ",");
    assert!(
        decoded_url.contains("AANobbMI"),
        "decoded URL should contain AANobbMI: {}",
        decoded_url
    );
    assert!(
        decoded_url.contains("gvQqBUqZ"),
        "decoded URL should contain gvQqBUqZ: {}",
        decoded_url
    );
}

#[tokio::test]
async fn get_projects_brief_carries_user_agent() {
    let client = CapturingMockClient::new(vec![MockResp::ok(MODRINTH_PROJECTS_BATCH_FIXTURE)]);
    let provider = ModrinthProvider;

    let ids = vec!["AANobbMI".to_string()];
    provider.get_projects_brief(&client, &ids).await.unwrap();

    let all_headers = client.captured_headers().await;
    let headers = &all_headers[0];
    let has_ua = headers
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case("User-Agent") && v.contains("modloader/"));
    assert!(has_ua, "User-Agent header missing: {:?}", headers);
}

#[tokio::test]
async fn get_projects_brief_empty_ids_returns_empty_no_http() {
    let client = CapturingMockClient::new(vec![]); // no responses queued
    let provider = ModrinthProvider;

    let briefs = provider.get_projects_brief(&client, &[]).await.unwrap();

    assert!(briefs.is_empty(), "empty ids → empty result");
    let urls = client.captured_urls().await;
    assert!(urls.is_empty(), "empty ids → zero HTTP calls");
}

#[tokio::test]
async fn get_projects_brief_returns_http_error_on_non_200() {
    let client = CapturingMockClient::new(vec![MockResp(429, "rate limited".to_string())]);
    let provider = ModrinthProvider;

    let ids = vec!["AANobbMI".to_string()];
    let err = provider
        .get_projects_brief(&client, &ids)
        .await
        .unwrap_err();
    assert!(
        matches!(err, ProviderError::HttpStatus { status: 429, .. }),
        "expected HttpStatus(429), got {:?}",
        err
    );
}

#[tokio::test]
async fn get_projects_brief_summary_uses_description_not_body() {
    // Fixture has both `description` (short) and `body` (long Markdown).
    // The brief must carry `description`, not `body`.
    let client = CapturingMockClient::new(vec![MockResp::ok(MODRINTH_PROJECTS_BATCH_FIXTURE)]);
    let provider = ModrinthProvider;

    let ids = vec!["AANobbMI".to_string()];
    let briefs = provider.get_projects_brief(&client, &ids).await.unwrap();
    let sodium = briefs.iter().find(|b| b.project_id == "AANobbMI").unwrap();

    // Short description from fixture; body starts with "# Sodium"
    assert!(
        !sodium.summary.starts_with("# Sodium"),
        "summary must be the short description, not the body; got: {}",
        sodium.summary
    );
    assert_eq!(
        sodium.summary,
        "A modern rendering engine and client-side optimization mod for Minecraft."
    );
}
