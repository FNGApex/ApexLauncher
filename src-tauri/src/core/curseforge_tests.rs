//! Unit tests for `curseforge`. Extracted from the source module; wired back
//! via `#[cfg(test)] #[path = "curseforge_tests.rs"] mod tests;` so private items
//! remain accessible through `super::*`.

//! No live HTTP in any test. All HTTP uses a capturing mock client backed by a
//! pre-loaded response queue — same pattern as `modrinth.rs`.

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

    async fn request_count(&self) -> usize {
        self.captured.lock().await.len()
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
}

// ── Fixtures ───────────────────────────────────────────────────────────────

const CF_SEARCH_FIXTURE: &str = include_str!("fixtures/cf_search.json");
const CF_FILES_FIXTURE: &str = include_str!("fixtures/cf_files.json");
const CF_FILE_FIXTURE: &str = include_str!("fixtures/cf_file.json");
const CF_FILE_NO_URL_MD5_ONLY_FIXTURE: &str = include_str!("fixtures/cf_file_no_url_md5_only.json");

// ── split_game_versions unit tests ─────────────────────────────────────────

#[test]
fn split_mc_version_recognized() {
    let (mc, loaders) = split_game_versions(&["1.20.1".to_string()]);
    assert_eq!(mc, vec!["1.20.1"]);
    assert!(loaders.is_empty());
}

#[test]
fn split_forge_recognized_as_loader() {
    let (mc, loaders) = split_game_versions(&["Forge".to_string()]);
    assert!(mc.is_empty());
    assert_eq!(loaders, vec!["forge"]);
}

#[test]
fn split_neoforge_recognized_as_loader() {
    let (mc, loaders) = split_game_versions(&["NeoForge".to_string()]);
    assert!(mc.is_empty());
    assert_eq!(loaders, vec!["neoforge"]);
}

#[test]
fn split_fabric_recognized_as_loader() {
    let (mc, loaders) = split_game_versions(&["Fabric".to_string()]);
    assert!(mc.is_empty());
    assert_eq!(loaders, vec!["fabric"]);
}

#[test]
fn split_quilt_recognized_as_loader() {
    let (mc, loaders) = split_game_versions(&["Quilt".to_string()]);
    assert!(mc.is_empty());
    assert_eq!(loaders, vec!["quilt"]);
}

#[test]
fn split_forge_vs_neoforge_distinct() {
    let entries = vec!["Forge".to_string(), "NeoForge".to_string()];
    let (mc, loaders) = split_game_versions(&entries);
    assert!(mc.is_empty());
    assert!(
        loaders.contains(&"forge".to_string()),
        "forge missing: {:?}",
        loaders
    );
    assert!(
        loaders.contains(&"neoforge".to_string()),
        "neoforge missing: {:?}",
        loaders
    );
}

#[test]
fn split_unknown_entries_discarded() {
    let entries = vec![
        "Java 17".to_string(),
        "Windows".to_string(),
        "Client".to_string(),
    ];
    let (mc, loaders) = split_game_versions(&entries);
    assert!(mc.is_empty(), "mc should be empty: {:?}", mc);
    assert!(loaders.is_empty(), "loaders should be empty: {:?}", loaders);
}

// ── is_mc_version spec-alignment tests (rule: /^\d+\.\d+/) ──────────────

#[test]
fn is_mc_version_two_digit_major_accepted() {
    // "10.2" has a two-digit leading segment — old impl rejected this; spec accepts it.
    let (mc, _) = split_game_versions(&["10.2".to_string()]);
    assert_eq!(mc, vec!["10.2"], "two-digit major version must be accepted");
}

#[test]
fn is_mc_version_non_digit_after_first_digit_rejected() {
    // "1x.2" starts with a digit but has a non-digit before the dot — must be rejected.
    let (mc, _) = split_game_versions(&["1x.2".to_string()]);
    assert!(
        mc.is_empty(),
        "\"1x.2\" must not be classified as MC version"
    );
}

#[test]
fn is_mc_version_multi_digit_segments_accepted() {
    // e.g. "21.1" — a hypothetical future major version
    let (mc, _) = split_game_versions(&["21.1".to_string()]);
    assert_eq!(mc, vec!["21.1"], "21.1 must be accepted");
}

#[test]
fn split_mixed_array_correct() {
    // Real CF file entry: ["1.20.1", "Forge", "Java 17"]
    let entries = vec![
        "1.20.1".to_string(),
        "Forge".to_string(),
        "Java 17".to_string(),
    ];
    let (mc, loaders) = split_game_versions(&entries);
    assert_eq!(mc, vec!["1.20.1"]);
    assert_eq!(loaders, vec!["forge"]);
}

#[test]
fn split_fabric_and_quilt_together() {
    let entries = vec![
        "1.20.1".to_string(),
        "Fabric".to_string(),
        "Quilt".to_string(),
    ];
    let (mc, loaders) = split_game_versions(&entries);
    assert_eq!(mc, vec!["1.20.1"]);
    assert!(loaders.contains(&"fabric".to_string()));
    assert!(loaders.contains(&"quilt".to_string()));
}

// ── Key-absent: error returned without HTTP call ───────────────────────────

#[tokio::test]
async fn search_key_absent_returns_key_missing_without_http() {
    let client = CapturingMockClient::new(vec![]); // no responses queued
    let provider = CurseForgeProvider::new(None);
    let params = SearchParams {
        query: "jei".to_string(),
        mc_version: None,
        loader: None,
        offset: 0,
        limit: 20,
        project_type: ProjectType::default(),
    };

    let err = provider.search(&client, &params).await.unwrap_err();

    assert!(
        matches!(err, ProviderError::KeyMissing),
        "expected KeyMissing, got {:?}",
        err
    );
    assert_eq!(
        client.request_count().await,
        0,
        "no HTTP call should have been made"
    );
}

#[tokio::test]
async fn get_versions_key_absent_returns_key_missing_without_http() {
    let client = CapturingMockClient::new(vec![]);
    let provider = CurseForgeProvider::new(None);

    let err = provider
        .get_versions(&client, "238222", None, None)
        .await
        .unwrap_err();

    assert!(
        matches!(err, ProviderError::KeyMissing),
        "expected KeyMissing, got {:?}",
        err
    );
    assert_eq!(client.request_count().await, 0);
}

// ── x-api-key header present on every request ──────────────────────────────

#[tokio::test]
async fn search_carries_api_key_header() {
    let client = CapturingMockClient::new(vec![MockResp::ok(CF_SEARCH_FIXTURE)]);
    let provider = CurseForgeProvider::new(Some("test-cf-key".to_string()));
    let params = SearchParams {
        query: "jei".to_string(),
        mc_version: None,
        loader: None,
        offset: 0,
        limit: 20,
        project_type: ProjectType::default(),
    };

    provider.search(&client, &params).await.unwrap();

    let all_headers = client.captured_headers().await;
    let headers = &all_headers[0];
    let has_key = headers
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case("x-api-key") && v == "test-cf-key");
    assert!(has_key, "x-api-key header missing: {:?}", headers);
}

#[tokio::test]
async fn get_versions_carries_api_key_header() {
    let client = CapturingMockClient::new(vec![MockResp::ok(CF_FILES_FIXTURE)]);
    let provider = CurseForgeProvider::new(Some("test-cf-key".to_string()));

    provider
        .get_versions(&client, "238222", None, None)
        .await
        .unwrap();

    let all_headers = client.captured_headers().await;
    let headers = &all_headers[0];
    let has_key = headers
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case("x-api-key") && v == "test-cf-key");
    assert!(has_key, "x-api-key header missing: {:?}", headers);
}

// ── search: fixture → correct ProjectSummary mapping ──────────────────────

#[tokio::test]
async fn search_maps_fixture_to_project_summaries() {
    let client = CapturingMockClient::new(vec![MockResp::ok(CF_SEARCH_FIXTURE)]);
    let provider = CurseForgeProvider::new(Some("key".to_string()));
    let params = SearchParams {
        query: "jei".to_string(),
        mc_version: None,
        loader: None,
        offset: 0,
        limit: 20,
        project_type: ProjectType::default(),
    };

    let result = provider.search(&client, &params).await.unwrap();

    assert_eq!(result.hits.len(), 2);
    assert_eq!(result.total, 2);
    assert_eq!(result.offset, 0);

    let jei = &result.hits[0];
    assert_eq!(
        jei.provider,
        crate::core::providers::ProviderKind::CurseForge
    );
    // CF numeric id must be stringified.
    assert_eq!(jei.id, "238222");
    assert_eq!(jei.slug, "jei");
    assert_eq!(jei.name, "Just Enough Items (JEI)");
    assert_eq!(jei.summary, "JEI is an item and recipe viewing mod for Minecraft, built from the ground up for stability and performance.");
    assert_eq!(jei.downloads, 300_000_000);
    assert!(jei.icon_url.is_some(), "logo url should be mapped");
    assert!(
        jei.categories.contains(&"Map and Information".to_string()),
        "categories: {:?}",
        jei.categories
    );
}

#[tokio::test]
async fn search_url_contains_gameid_classid_index_pagesize() {
    let client = CapturingMockClient::new(vec![MockResp::ok(CF_SEARCH_FIXTURE)]);
    let provider = CurseForgeProvider::new(Some("key".to_string()));
    let params = SearchParams {
        query: "jei".to_string(),
        mc_version: Some("1.20.1".to_string()),
        loader: Some("forge".to_string()),
        offset: 20,
        limit: 10,
        project_type: ProjectType::Mod,
    };

    provider.search(&client, &params).await.unwrap();

    let urls = client.captured_urls().await;
    let url = &urls[0];
    assert!(url.contains("gameId=432"), "gameId missing: {url}");
    assert!(url.contains("classId=6"), "classId missing: {url}");
    assert!(url.contains("index=20"), "index/offset missing: {url}");
    assert!(url.contains("pageSize=10"), "pageSize/limit missing: {url}");
    assert!(
        url.contains("gameVersion=1.20.1"),
        "gameVersion missing: {url}"
    );
    assert!(
        url.contains("modLoaderType=1"),
        "modLoaderType=1 (Forge) missing: {url}"
    );
    // Default sort: Popularity desc, so a text query like "jei" surfaces the
    // well-known mod instead of CF's arbitrary unsorted default order.
    assert!(
        url.contains("sortField=2"),
        "sortField=2 (Popularity) missing: {url}"
    );
    assert!(
        url.contains("sortOrder=desc"),
        "sortOrder=desc missing: {url}"
    );
}

// ── get_versions: downloadUrl: null → url: None ────────────────────────────

#[tokio::test]
async fn get_versions_download_url_null_maps_to_none() {
    let client = CapturingMockClient::new(vec![MockResp::ok(CF_FILES_FIXTURE)]);
    let provider = CurseForgeProvider::new(Some("key".to_string()));

    let versions = provider
        .get_versions(&client, "238222", None, None)
        .await
        .unwrap();

    // cf_files.json has 3 entries: id 5034058 (url present), 5034059 (url present),
    // 5034060 (downloadUrl: null).
    let null_url_version = versions
        .iter()
        .find(|v| v.id == "5034060")
        .expect("version 5034060 not found");

    let file = &null_url_version.files[0];
    assert!(
        file.url.is_none(),
        "downloadUrl: null should map to url: None, got {:?}",
        file.url
    );
}

#[tokio::test]
async fn get_versions_present_download_url_maps_to_some() {
    let client = CapturingMockClient::new(vec![MockResp::ok(CF_FILES_FIXTURE)]);
    let provider = CurseForgeProvider::new(Some("key".to_string()));

    let versions = provider
        .get_versions(&client, "238222", None, None)
        .await
        .unwrap();

    let v = versions
        .iter()
        .find(|v| v.id == "5034058")
        .expect("version 5034058 not found");

    assert!(
        v.files[0].url.is_some(),
        "non-null downloadUrl should be Some"
    );
}

// ── get_versions: fixture field mapping ───────────────────────────────────

#[tokio::test]
async fn get_versions_maps_game_versions_split() {
    let client = CapturingMockClient::new(vec![MockResp::ok(CF_FILES_FIXTURE)]);
    let provider = CurseForgeProvider::new(Some("key".to_string()));

    let versions = provider
        .get_versions(&client, "238222", None, None)
        .await
        .unwrap();

    // Version 5034058: ["1.20.1", "Forge", "Java 17"] → mc=["1.20.1"], loaders=["forge"]
    let v = versions.iter().find(|v| v.id == "5034058").unwrap();
    assert_eq!(v.game_versions, vec!["1.20.1"]);
    assert_eq!(v.loaders, vec!["forge"]);

    // Version 5034060: ["1.20.1", "Fabric", "Quilt"] → mc=["1.20.1"], loaders=["fabric","quilt"]
    let v2 = versions.iter().find(|v| v.id == "5034060").unwrap();
    assert_eq!(v2.game_versions, vec!["1.20.1"]);
    assert!(v2.loaders.contains(&"fabric".to_string()));
    assert!(v2.loaders.contains(&"quilt".to_string()));
}

#[tokio::test]
async fn get_versions_dependency_relation_types_mapped() {
    let client = CapturingMockClient::new(vec![MockResp::ok(CF_FILES_FIXTURE)]);
    let provider = CurseForgeProvider::new(Some("key".to_string()));

    let versions = provider
        .get_versions(&client, "238222", None, None)
        .await
        .unwrap();

    // Version 5034058 has relationType=3 (required) dep on modId 308702.
    let v = versions.iter().find(|v| v.id == "5034058").unwrap();
    assert_eq!(v.dependencies.len(), 1);
    assert_eq!(v.dependencies[0].dependency_type, "required");
    assert_eq!(v.dependencies[0].project_id, Some("308702".to_string()));

    // Version 5034060 has relationType=2 (optional) and relationType=5 (incompatible).
    let v2 = versions.iter().find(|v| v.id == "5034060").unwrap();
    assert_eq!(v2.dependencies.len(), 2);
    let dep_types: Vec<&str> = v2
        .dependencies
        .iter()
        .map(|d| d.dependency_type.as_str())
        .collect();
    assert!(
        dep_types.contains(&"optional"),
        "optional missing: {:?}",
        dep_types
    );
    assert!(
        dep_types.contains(&"incompatible"),
        "incompatible missing: {:?}",
        dep_types
    );
}

// ── get_versions: mc + loader filter ─────────────────────────────────────

#[tokio::test]
async fn get_versions_filter_mc_and_loader() {
    // Fixture has 3 files:
    //   5034058 — 1.20.1 + Forge
    //   5034059 — 1.21  + NeoForge
    //   5034060 — 1.20.1 + Fabric + Quilt (null url)
    let client = CapturingMockClient::new(vec![MockResp::ok(CF_FILES_FIXTURE)]);
    let provider = CurseForgeProvider::new(Some("key".to_string()));

    let versions = provider
        .get_versions(&client, "238222", Some("1.20.1"), Some("forge"))
        .await
        .unwrap();

    assert_eq!(
        versions.len(),
        1,
        "only forge+1.20.1 should pass: {:?}",
        versions.iter().map(|v| &v.id).collect::<Vec<_>>()
    );
    assert_eq!(versions[0].id, "5034058");
}

#[tokio::test]
async fn get_versions_filter_mc_only() {
    let client = CapturingMockClient::new(vec![MockResp::ok(CF_FILES_FIXTURE)]);
    let provider = CurseForgeProvider::new(Some("key".to_string()));

    let versions = provider
        .get_versions(&client, "238222", Some("1.21"), None)
        .await
        .unwrap();

    assert_eq!(versions.len(), 1, "only 1.21 version should match");
    assert_eq!(versions[0].id, "5034059");
}

#[tokio::test]
async fn get_versions_no_filter_returns_all() {
    let client = CapturingMockClient::new(vec![MockResp::ok(CF_FILES_FIXTURE)]);
    let provider = CurseForgeProvider::new(Some("key".to_string()));

    let versions = provider
        .get_versions(&client, "238222", None, None)
        .await
        .unwrap();

    assert_eq!(
        versions.len(),
        3,
        "all 3 fixture files expected, got {}",
        versions.len()
    );
}

// ── get_file: single-file resolver (CP B2) ─────────────────────────────────

#[tokio::test]
async fn get_file_key_absent_returns_key_missing_without_http() {
    let client = CapturingMockClient::new(vec![]);
    let provider = CurseForgeProvider::new(None);

    let err = provider
        .get_file(&client, 238222, 5034058)
        .await
        .unwrap_err();

    assert!(
        matches!(err, ProviderError::KeyMissing),
        "expected KeyMissing, got {:?}",
        err
    );
    assert_eq!(client.request_count().await, 0);
}

#[tokio::test]
async fn get_file_carries_api_key_header() {
    let client = CapturingMockClient::new(vec![MockResp::ok(CF_FILE_FIXTURE)]);
    let provider = CurseForgeProvider::new(Some("test-cf-key".to_string()));

    provider.get_file(&client, 238222, 5034058).await.unwrap();

    let all_headers = client.captured_headers().await;
    let headers = &all_headers[0];
    let has_key = headers
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case("x-api-key") && v == "test-cf-key");
    assert!(has_key, "x-api-key header missing: {:?}", headers);
}

#[tokio::test]
async fn get_file_url_targets_project_and_file_id() {
    let client = CapturingMockClient::new(vec![MockResp::ok(CF_FILE_FIXTURE)]);
    let provider = CurseForgeProvider::new(Some("key".to_string()));

    provider.get_file(&client, 238222, 5034058).await.unwrap();

    let urls = client.captured_urls().await;
    assert!(
        urls[0].ends_with("/v1/mods/238222/files/5034058"),
        "unexpected url: {}",
        urls[0]
    );
}

#[tokio::test]
async fn get_file_maps_fixture_fields() {
    let client = CapturingMockClient::new(vec![MockResp::ok(CF_FILE_FIXTURE)]);
    let provider = CurseForgeProvider::new(Some("key".to_string()));

    let file = provider.get_file(&client, 238222, 5034058).await.unwrap();

    assert_eq!(file.file_name, "jei-1.20.1-forge-15.3.0.4.jar");
    assert_eq!(file.size, Some(1234567));
    assert_eq!(
        file.url,
        Some("https://edge.forgecdn.net/files/5034/058/jei-1.20.1-forge-15.3.0.4.jar".to_string())
    );
    assert_eq!(file.hashes.get("sha1"), Some(&"abc123sha1hash".to_string()));
    assert!(file.primary);
}

#[tokio::test]
async fn get_file_download_url_null_maps_to_none() {
    let client = CapturingMockClient::new(vec![MockResp::ok(CF_FILE_NO_URL_MD5_ONLY_FIXTURE)]);
    let provider = CurseForgeProvider::new(Some("key".to_string()));

    let file = provider.get_file(&client, 238222, 5034060).await.unwrap();

    assert!(
        file.url.is_none(),
        "downloadUrl: null should map to url: None"
    );
}

#[tokio::test]
async fn get_file_prefers_sha1_over_md5() {
    let client = CapturingMockClient::new(vec![MockResp::ok(CF_FILE_FIXTURE)]);
    let provider = CurseForgeProvider::new(Some("key".to_string()));

    let file = provider.get_file(&client, 238222, 5034058).await.unwrap();

    assert!(
        file.hashes.contains_key("sha1"),
        "sha1 should be picked when present"
    );
    assert!(
        !file.hashes.contains_key("md5"),
        "md5 should not be picked when sha1 present"
    );
}

#[tokio::test]
async fn get_file_falls_back_to_md5_when_no_sha1() {
    let client = CapturingMockClient::new(vec![MockResp::ok(CF_FILE_NO_URL_MD5_ONLY_FIXTURE)]);
    let provider = CurseForgeProvider::new(Some("key".to_string()));

    let file = provider.get_file(&client, 238222, 5034060).await.unwrap();

    assert_eq!(file.hashes.get("md5"), Some(&"def456md5hash".to_string()));
}

#[tokio::test]
async fn get_file_returns_http_error_on_404() {
    let client = CapturingMockClient::new(vec![MockResp(404, "Not Found".to_string())]);
    let provider = CurseForgeProvider::new(Some("key".to_string()));

    let err = provider
        .get_file(&client, 238222, 999999)
        .await
        .unwrap_err();

    assert!(
        matches!(err, ProviderError::HttpStatus { status: 404, .. }),
        "expected HttpStatus(404), got {:?}",
        err
    );
}

// ── search: HTTP error propagation ────────────────────────────────────────

#[tokio::test]
async fn search_returns_http_error_on_403() {
    let client = CapturingMockClient::new(vec![MockResp(403, "Forbidden".to_string())]);
    let provider = CurseForgeProvider::new(Some("bad-key".to_string()));
    let params = SearchParams {
        query: "test".to_string(),
        mc_version: None,
        loader: None,
        offset: 0,
        limit: 10,
        project_type: ProjectType::default(),
    };

    let err = provider.search(&client, &params).await.unwrap_err();
    assert!(
        matches!(err, ProviderError::HttpStatus { status: 403, .. }),
        "expected HttpStatus(403), got {:?}",
        err
    );
}

// ── project_type selector: classId switching ──────────────────────────────

#[tokio::test]
async fn search_url_with_project_type_mod_uses_class_id_6() {
    let client = CapturingMockClient::new(vec![MockResp::ok(CF_SEARCH_FIXTURE)]);
    let provider = CurseForgeProvider::new(Some("key".to_string()));
    let params = SearchParams {
        query: "jei".to_string(),
        mc_version: None,
        loader: None,
        offset: 0,
        limit: 20,
        project_type: ProjectType::Mod,
    };

    provider.search(&client, &params).await.unwrap();

    let urls = client.captured_urls().await;
    assert!(
        urls[0].contains("classId=6"),
        "classId=6 (mods) expected for ProjectType::Mod: {}",
        urls[0]
    );
}

#[tokio::test]
async fn search_url_with_project_type_modpack_uses_class_id_4471() {
    let client = CapturingMockClient::new(vec![MockResp::ok(CF_SEARCH_FIXTURE)]);
    let provider = CurseForgeProvider::new(Some("key".to_string()));
    let params = SearchParams {
        query: "all the mods".to_string(),
        mc_version: None,
        loader: None,
        offset: 0,
        limit: 20,
        project_type: ProjectType::Modpack,
    };

    provider.search(&client, &params).await.unwrap();

    let urls = client.captured_urls().await;
    assert!(
        urls[0].contains("classId=4471"),
        "classId=4471 (modpacks) expected for ProjectType::Modpack: {}",
        urls[0]
    );
    assert!(
        !urls[0].contains("classId=6"),
        "classId=6 must not appear for modpack: {}",
        urls[0]
    );
}

// ── page_url: populated from links.websiteUrl ─────────────────────────────

#[tokio::test]
async fn search_populates_page_url_from_links_website_url() {
    let client = CapturingMockClient::new(vec![MockResp::ok(CF_SEARCH_FIXTURE)]);
    let provider = CurseForgeProvider::new(Some("key".to_string()));
    let params = SearchParams {
        query: "jei".to_string(),
        mc_version: None,
        loader: None,
        offset: 0,
        limit: 20,
        project_type: ProjectType::Mod,
    };

    let result = provider.search(&client, &params).await.unwrap();
    let jei = &result.hits[0];

    assert_eq!(
        jei.page_url,
        Some("https://www.curseforge.com/minecraft/mc-mods/jei".to_string()),
        "page_url should come from links.websiteUrl: {:?}",
        jei.page_url
    );
}

#[tokio::test]
async fn search_page_url_none_when_website_url_absent() {
    // Build a fixture without links.websiteUrl (or with null websiteUrl).
    let fixture = r#"{
        "data": [{
            "id": 99999,
            "name": "No Links Mod",
            "slug": "no-links-mod",
            "summary": "A mod with no links.",
            "downloadCount": 100,
            "logo": null,
            "categories": [],
            "links": { "websiteUrl": null, "wikiUrl": null, "issuesUrl": null, "sourceUrl": null }
        }],
        "pagination": { "index": 0, "pageSize": 20, "resultCount": 1, "totalCount": 1 }
    }"#;
    let client = CapturingMockClient::new(vec![MockResp::ok(fixture)]);
    let provider = CurseForgeProvider::new(Some("key".to_string()));
    let params = SearchParams {
        query: "".to_string(),
        mc_version: None,
        loader: None,
        offset: 0,
        limit: 20,
        project_type: ProjectType::Mod,
    };

    let result = provider.search(&client, &params).await.unwrap();
    let hit = &result.hits[0];

    assert!(
        hit.page_url.is_none(),
        "page_url should be None when websiteUrl is null: {:?}",
        hit.page_url
    );
}
