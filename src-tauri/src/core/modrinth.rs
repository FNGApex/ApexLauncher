//! Modrinth `ModProvider` implementation.
//!
//! ## API surface used
//! - `GET /v2/search` — paginated mod search with facets for loader, MC version, project type.
//! - `GET /v2/project/{id}/version` — full version list, filtered client-side by mc_version + loader.
//!
//! ## HTTP
//! All HTTP goes through the injected `ProviderHttpClient` seam so tests need no live network.
//!
//! ## User-Agent
//! Modrinth's etiquette requires a descriptive UA or they may rate-limit. We use the same
//! string as `meta.rs` (`modloader/<version> (https://github.com/; minecraft launcher)`).

use serde::Deserialize;

use crate::core::providers::{
    Dependency, ModProvider, MrSearchResponse, ProjectVersion, ProviderError, ProviderHttpClient,
    ProviderKind, SearchParams, SearchResult, VersionFile,
};

// ── Percent-encoding helper ────────────────────────────────────────────────────

/// Percent-encode a string for use in a URL query parameter value.
///
/// Encodes all characters outside the unreserved set (A-Z a-z 0-9 `-` `_` `.` `~`),
/// which safely handles the facets JSON array string (contains `[`, `]`, `"`, `,`, `:`).
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
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

/// Decode percent-encoded characters in a string (for test assertions only).
#[cfg(test)]
fn percent_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(decoded) = u8::from_str_radix(hex, 16) {
                    out.push(decoded as char);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

// ── Constants ──────────────────────────────────────────────────────────────────

const BASE_URL: &str = "https://api.modrinth.com/v2";

/// User-Agent required by Modrinth etiquette. Matches `meta.rs:16`.
const USER_AGENT: &str = concat!(
    "modloader/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/; minecraft launcher)"
);

// ── Raw Modrinth version deserialization types ─────────────────────────────────

/// Raw Modrinth `/project/{id}/version` response item.
#[derive(Debug, Deserialize)]
struct MrVersion {
    id: String,
    project_id: String,
    name: String,
    version_number: String,
    game_versions: Vec<String>,
    loaders: Vec<String>,
    files: Vec<MrFile>,
    dependencies: Vec<MrDependency>,
}

#[derive(Debug, Deserialize)]
struct MrFile {
    url: String,
    filename: String,
    size: u64,
    hashes: MrHashes,
    primary: bool,
}

#[derive(Debug, Deserialize)]
struct MrHashes {
    sha1: String,
    sha512: String,
}

#[derive(Debug, Deserialize)]
struct MrDependency {
    project_id: Option<String>,
    version_id: Option<String>,
    dependency_type: String,
}

impl MrVersion {
    fn into_project_version(self) -> ProjectVersion {
        let files = self
            .files
            .into_iter()
            .map(|f| {
                let mut hashes = std::collections::HashMap::new();
                hashes.insert("sha1".to_string(), f.hashes.sha1);
                hashes.insert("sha512".to_string(), f.hashes.sha512);
                VersionFile {
                    url: Some(f.url),
                    file_name: f.filename,
                    size: Some(f.size),
                    hashes,
                    primary: f.primary,
                }
            })
            .collect();

        let dependencies = self
            .dependencies
            .into_iter()
            .map(|d| Dependency {
                project_id: d.project_id,
                version_id: d.version_id,
                dependency_type: d.dependency_type,
            })
            .collect();

        ProjectVersion {
            provider: ProviderKind::Modrinth,
            id: self.id,
            name: self.name,
            version_number: self.version_number,
            game_versions: self.game_versions,
            loaders: self.loaders,
            files,
            dependencies,
        }
    }
}

// ── ModrinthProvider ───────────────────────────────────────────────────────────

/// Modrinth implementation of `ModProvider`.
pub struct ModrinthProvider;

impl ModrinthProvider {
    /// Build the Modrinth `/search` URL from `SearchParams`.
    ///
    /// Facets are encoded as a JSON array-of-arrays as required by the Modrinth API.
    /// `project_type:mod` is always included so resource packs and modpacks are excluded.
    fn build_search_url(params: &SearchParams) -> String {
        let mut facets: Vec<String> = vec![r#"["project_type:mod"]"#.to_string()];

        if let Some(mc) = &params.mc_version {
            facets.push(format!(r#"["versions:{}"]"#, mc));
        }
        if let Some(loader) = &params.loader {
            facets.push(format!(r#"["categories:{}"]"#, loader));
        }

        let facets_json = format!("[{}]", facets.join(","));

        format!(
            "{}/search?query={}&facets={}&offset={}&limit={}",
            BASE_URL,
            percent_encode(&params.query),
            percent_encode(&facets_json),
            params.offset,
            params.limit,
        )
    }

    /// Build the Modrinth `/project/{id}/version` URL with optional server-side filters.
    fn build_versions_url(project_id: &str, mc_version: Option<&str>, loader: Option<&str>) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(mc) = mc_version {
            // Modrinth expects game_versions=["1.21"] — the array brackets and quotes need encoding.
            parts.push(format!(
                "game_versions={}",
                percent_encode(&format!(r#"["{}"]"#, mc))
            ));
        }
        if let Some(l) = loader {
            parts.push(format!(
                "loaders={}",
                percent_encode(&format!(r#"["{}"]"#, l))
            ));
        }
        let base = format!("{}/project/{}/version", BASE_URL, project_id);
        if parts.is_empty() {
            base
        } else {
            format!("{}?{}", base, parts.join("&"))
        }
    }
}

#[async_trait::async_trait]
impl ModProvider for ModrinthProvider {
    async fn search(
        &self,
        client: &dyn ProviderHttpClient,
        params: &SearchParams,
    ) -> Result<SearchResult, ProviderError> {
        let url = Self::build_search_url(params);
        let (status, body) = client
            .get(&url, &[("User-Agent", USER_AGENT)])
            .await
            .map_err(ProviderError::from)?;

        if status != 200 {
            return Err(ProviderError::HttpStatus { status, body });
        }

        let raw: MrSearchResponse =
            serde_json::from_str(&body).map_err(|e| ProviderError::BadResponse(e.to_string()))?;

        let hits = raw.hits.into_iter().map(|h| h.into_summary()).collect();
        Ok(SearchResult {
            hits,
            offset: raw.offset,
            total: raw.total_hits,
        })
    }

    async fn get_versions(
        &self,
        client: &dyn ProviderHttpClient,
        project_id: &str,
        mc_version: Option<&str>,
        loader: Option<&str>,
    ) -> Result<Vec<ProjectVersion>, ProviderError> {
        // Pass filters to the API as server-side hints; also filter client-side for safety.
        let url = Self::build_versions_url(project_id, mc_version, loader);
        let (status, body) = client
            .get(&url, &[("User-Agent", USER_AGENT)])
            .await
            .map_err(ProviderError::from)?;

        if status != 200 {
            return Err(ProviderError::HttpStatus { status, body });
        }

        let raw_versions: Vec<MrVersion> =
            serde_json::from_str(&body).map_err(|e| ProviderError::BadResponse(e.to_string()))?;

        let versions: Vec<ProjectVersion> = raw_versions
            .into_iter()
            .map(|v| v.into_project_version())
            .filter(|v| {
                // Client-side compatibility filter (guards against API returning extras).
                let mc_ok = mc_version
                    .map(|mc| v.game_versions.iter().any(|gv| gv == mc))
                    .unwrap_or(true);
                let loader_ok = loader
                    .map(|l| v.loaders.iter().any(|vl| vl == l))
                    .unwrap_or(true);
                mc_ok && loader_ok
            })
            .collect();

        Ok(versions)
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    //! No live HTTP in any test. All HTTP calls use a capturing mock client backed
    //! by a pre-loaded response queue.

    use super::*;
    use crate::core::providers::{ProviderError, ProviderHttpClient, SearchParams};
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
    }

    // ── Fixtures ───────────────────────────────────────────────────────────────

    const MODRINTH_SEARCH_FIXTURE: &str = include_str!("fixtures/modrinth_search.json");
    const MODRINTH_VERSIONS_FIXTURE: &str = include_str!("fixtures/modrinth_versions.json");

    // ── URL construction ───────────────────────────────────────────────────────

    #[test]
    fn search_url_includes_query_facets_offset_limit() {
        let params = SearchParams {
            query: "sodium".to_string(),
            mc_version: Some("1.21".to_string()),
            loader: Some("fabric".to_string()),
            offset: 0,
            limit: 20,
        };
        let url = ModrinthProvider::build_search_url(&params);
        assert!(url.contains("query=sodium"), "query missing: {url}");
        assert!(url.contains("offset=0"), "offset missing: {url}");
        assert!(url.contains("limit=20"), "limit missing: {url}");
        // facets must encode project_type:mod, versions:1.21, categories:fabric
        let decoded = percent_decode(&url);
        assert!(decoded.contains("project_type:mod"), "project_type:mod missing: {decoded}");
        assert!(decoded.contains("versions:1.21"), "versions:1.21 missing: {decoded}");
        assert!(decoded.contains("categories:fabric"), "categories:fabric missing: {decoded}");
    }

    #[test]
    fn search_url_no_filters_still_includes_project_type_mod() {
        let params = SearchParams {
            query: "".to_string(),
            mc_version: None,
            loader: None,
            offset: 20,
            limit: 10,
        };
        let url = ModrinthProvider::build_search_url(&params);
        let decoded = percent_decode(&url);
        assert!(decoded.contains("project_type:mod"), "project_type:mod missing: {decoded}");
        assert!(!decoded.contains("versions:"), "unexpected versions facet: {decoded}");
        assert!(url.contains("offset=20"), "offset=20 missing: {url}");
        assert!(url.contains("limit=10"), "limit=10 missing: {url}");
    }

    #[test]
    fn versions_url_with_both_filters() {
        let url = ModrinthProvider::build_versions_url("AANobbMI", Some("1.21"), Some("fabric"));
        assert!(url.contains("/project/AANobbMI/version"), "path missing: {url}");
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
        };

        provider.search(&client, &params).await.unwrap();

        let urls = client.captured_urls().await;
        assert_eq!(urls.len(), 1);
        let raw_url = &urls[0];
        let decoded = percent_decode(raw_url);

        assert!(raw_url.contains("query=sodium"), "query param missing: {raw_url}");
        assert!(raw_url.contains("offset=0"), "offset missing: {raw_url}");
        assert!(raw_url.contains("limit=20"), "limit missing: {raw_url}");
        assert!(decoded.contains("project_type:mod"), "project_type:mod facet missing: {decoded}");
        assert!(decoded.contains("versions:1.21"), "versions facet missing: {decoded}");
        assert!(decoded.contains("categories:fabric"), "categories facet missing: {decoded}");
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
        let client =
            CapturingMockClient::new(vec![MockResp::ok(MODRINTH_VERSIONS_FIXTURE)]);
        let provider = ModrinthProvider;

        let versions = provider
            .get_versions(&client, "AANobbMI", Some("1.21"), Some("fabric"))
            .await
            .unwrap();

        assert_eq!(versions.len(), 1, "only 1 version should pass filters; got {:?}", versions.iter().map(|v| &v.id).collect::<Vec<_>>());
        let v = &versions[0];
        assert_eq!(v.id, "AABBCC11");
        assert_eq!(v.provider, ProviderKind::Modrinth);
        assert!(v.game_versions.contains(&"1.21".to_string()));
        assert!(v.loaders.contains(&"fabric".to_string()));
    }

    #[tokio::test]
    async fn get_versions_no_filter_returns_all_entries() {
        let client =
            CapturingMockClient::new(vec![MockResp::ok(MODRINTH_VERSIONS_FIXTURE)]);
        let provider = ModrinthProvider;

        let versions = provider
            .get_versions(&client, "AANobbMI", None, None)
            .await
            .unwrap();

        assert_eq!(versions.len(), 3, "no filters → all 3 versions from fixture");
    }

    #[tokio::test]
    async fn get_versions_mc_only_filter() {
        // Filter by mc_version=1.21 only — should match entries 1 (fabric+quilt) and 3 (neoforge).
        let client =
            CapturingMockClient::new(vec![MockResp::ok(MODRINTH_VERSIONS_FIXTURE)]);
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
        let client =
            CapturingMockClient::new(vec![MockResp::ok(MODRINTH_VERSIONS_FIXTURE)]);
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
        let client =
            CapturingMockClient::new(vec![MockResp::ok(MODRINTH_VERSIONS_FIXTURE)]);
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
        let client =
            CapturingMockClient::new(vec![MockResp::ok(MODRINTH_VERSIONS_FIXTURE)]);
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
        let client =
            CapturingMockClient::new(vec![MockResp(404, "not found".to_string())]);
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
}
