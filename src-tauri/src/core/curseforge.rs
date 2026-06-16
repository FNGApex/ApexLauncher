//! CurseForge `ModProvider` implementation.
//!
//! ## API surface used
//! - `GET /v1/mods/search` — paginated mod search; `gameId=432` (Minecraft), `classId=6` (Mods),
//!   `modLoaderType` numeric mapping, `x-api-key` header.
//! - `GET /v1/mods/{id}/files` — version file list; `gameVersions[]` split heuristic to separate
//!   MC versions from loader tags.
//!
//! ## Key handling
//! The provider takes a resolved `Option<String>` key at construction time. Key resolution
//! (env var + settings fallback via `cf_api_key_from`) happens at the command layer (CP4).
//! If the key is `None`, every method returns `ProviderError::KeyMissing` immediately before
//! any HTTP call.
//!
//! ## gameVersions split rule
//! CF files include both MC versions (e.g. `"1.20.1"`) and loader names (e.g. `"Forge"`) in
//! the same `gameVersions` array with no discriminating flag. Parse rule:
//! - Entry starts with a digit followed by `.` → treat as MC version.
//! - Entry is a known loader name (case-insensitive: Forge, NeoForge, Fabric, Quilt) → loader tag.
//! - Otherwise → discard.

use serde::Deserialize;

use crate::core::providers::{
    Dependency, ModProvider, ProjectVersion, ProviderError, ProviderHttpClient,
    ProviderKind, SearchParams, SearchResult, VersionFile,
};

// ── Constants ──────────────────────────────────────────────────────────────────

const BASE_URL: &str = "https://api.curseforge.com";

/// CF game ID for Minecraft.
const MINECRAFT_GAME_ID: u32 = 432;

/// CF class ID for mods.
const MODS_CLASS_ID: u32 = 6;

/// CF `ModsSearchSortField` value for Popularity. Used as the default search
/// sort so a text query surfaces well-known mods (CF returns an arbitrary order
/// when no sort is given, which can push even the obvious match off page one).
const SORT_FIELD_POPULARITY: u32 = 2;

// ── gameVersions split heuristic ──────────────────────────────────────────────

/// Returns `true` if the entry looks like a Minecraft version string.
///
/// Rule: entry matches `/^\d+\.\d+/` — one or more leading digits followed by a dot
/// and at least one more digit.
/// Examples that match: `"1.20.1"`, `"1.21"`, `"1.7.10"`, `"10.2"`, `"21.1"`.
/// Examples that do not match: `"Forge"`, `"Java 17"`, `"1x.2"`, `".21"`.
fn is_mc_version(entry: &str) -> bool {
    let bytes = entry.as_bytes();
    // Skip leading digits (must have at least one).
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    // Must have consumed at least one digit, then a dot, then at least one more digit.
    i > 0 && bytes.get(i) == Some(&b'.') && bytes.get(i + 1).map_or(false, |b| b.is_ascii_digit())
}

/// Returns the normalized loader tag if `entry` is a known loader name,
/// or `None` if it should be discarded.
///
/// Known loaders: Forge, NeoForge, Fabric, Quilt (case-insensitive).
/// Normalized to lowercase to match the project convention.
fn as_loader_tag(entry: &str) -> Option<&'static str> {
    match entry.to_lowercase().as_str() {
        "forge" => Some("forge"),
        "neoforge" => Some("neoforge"),
        "fabric" => Some("fabric"),
        "quilt" => Some("quilt"),
        _ => None,
    }
}

/// Split a `gameVersions` array from a CF file entry into `(mc_versions, loader_tags)`.
///
/// Unknown entries (e.g. `"Java 17"`) are silently discarded.
pub fn split_game_versions(entries: &[String]) -> (Vec<String>, Vec<String>) {
    let mut mc_versions = Vec::new();
    let mut loader_tags = Vec::new();
    for entry in entries {
        if is_mc_version(entry) {
            mc_versions.push(entry.clone());
        } else if let Some(tag) = as_loader_tag(entry) {
            loader_tags.push(tag.to_string());
        }
        // Unknown entries silently discarded.
    }
    (mc_versions, loader_tags)
}

// ── modLoaderType mapping ──────────────────────────────────────────────────────

/// Maps a loader name string to the CF `modLoaderType` integer.
///
/// CF API numeric values: 1 = Forge, 2 = Cauldron, 3 = LiteLoader, 4 = Fabric, 5 = Quilt,
/// 6 = NeoForge. Unrecognized → `None` (omit the parameter).
fn loader_type_id(loader: &str) -> Option<u32> {
    match loader.to_lowercase().as_str() {
        "forge" => Some(1),
        "fabric" => Some(4),
        "quilt" => Some(5),
        "neoforge" => Some(6),
        _ => None,
    }
}

// ── Raw CurseForge file deserialization types ──────────────────────────────────

/// Raw CF `/v1/mods/{id}/files` response.
#[derive(Debug, Deserialize)]
struct CfFilesResponse {
    data: Vec<CfFile>,
}

#[derive(Debug, Deserialize)]
struct CfFile {
    id: u64,
    #[serde(rename = "displayName")]
    display_name: String,
    #[serde(rename = "fileName")]
    file_name: String,
    #[serde(rename = "fileLength")]
    file_length: Option<u64>,
    #[serde(rename = "downloadUrl")]
    download_url: Option<String>,
    #[serde(rename = "gameVersions")]
    game_versions: Vec<String>,
    #[serde(default)]
    dependencies: Vec<CfDependency>,
}

#[derive(Debug, Deserialize)]
struct CfDependency {
    #[serde(rename = "modId")]
    mod_id: u64,
    #[serde(rename = "relationType")]
    relation_type: u32,
}

impl CfFile {
    fn into_project_version(self) -> ProjectVersion {
        let (mc_versions, loader_tags) = split_game_versions(&self.game_versions);

        // CF files have no hash data in the file list endpoint; leave hashes empty.
        // (Hash lookup via fingerprint API is deferred to slice B install flow.)
        let file = VersionFile {
            url: self.download_url,
            file_name: self.file_name.clone(),
            size: self.file_length,
            hashes: std::collections::HashMap::new(),
            primary: true,
        };

        let dependencies = self
            .dependencies
            .into_iter()
            .map(|d| {
                // CF relationType: 1=EmbeddedLibrary, 2=OptionalDependency, 3=RequiredDependency,
                //                  4=Tool, 5=Incompatible, 6=Include
                let dep_type = match d.relation_type {
                    1 => "embedded",
                    2 => "optional",
                    3 => "required",
                    5 => "incompatible",
                    _ => "optional",
                };
                Dependency {
                    project_id: Some(d.mod_id.to_string()),
                    version_id: None,
                    dependency_type: dep_type.to_string(),
                }
            })
            .collect();

        ProjectVersion {
            provider: ProviderKind::CurseForge,
            id: self.id.to_string(),
            name: self.display_name,
            version_number: self.file_name,
            game_versions: mc_versions,
            loaders: loader_tags,
            files: vec![file],
            dependencies,
        }
    }
}

// ── CurseForgeProvider ─────────────────────────────────────────────────────────

/// CurseForge implementation of `ModProvider`.
///
/// Constructed with a resolved API key. Call `cf_api_key_from` at the command layer
/// (CP4) to obtain the key before constructing this struct.
pub struct CurseForgeProvider {
    /// Resolved CF API key. `None` → every method returns `ProviderError::KeyMissing`.
    api_key: Option<String>,
}

impl CurseForgeProvider {
    /// Create a new `CurseForgeProvider` with the given resolved key.
    ///
    /// Pass `None` to model the "key not configured" state — all methods will return
    /// `ProviderError::KeyMissing` without making any HTTP call.
    pub fn new(api_key: Option<String>) -> Self {
        Self { api_key }
    }

    /// Return the key or `KeyMissing` immediately.
    fn require_key(&self) -> Result<&str, ProviderError> {
        self.api_key
            .as_deref()
            .ok_or(ProviderError::KeyMissing)
    }

    /// Build the CF `/v1/mods/search` URL from `SearchParams`.
    fn build_search_url(params: &SearchParams) -> String {
        let mut url = format!(
            "{}/v1/mods/search?gameId={}&classId={}&index={}&pageSize={}&sortField={}&sortOrder=desc",
            BASE_URL,
            MINECRAFT_GAME_ID,
            MODS_CLASS_ID,
            params.offset,
            params.limit,
            SORT_FIELD_POPULARITY,
        );
        if !params.query.is_empty() {
            url.push_str(&format!("&searchFilter={}", percent_encode(&params.query)));
        }
        if let Some(mc) = &params.mc_version {
            url.push_str(&format!("&gameVersion={}", mc));
        }
        if let Some(loader) = &params.loader {
            if let Some(type_id) = loader_type_id(loader) {
                url.push_str(&format!("&modLoaderType={}", type_id));
            }
        }
        url
    }

    /// Build the CF `/v1/mods/{id}/files` URL.
    fn build_files_url(mod_id: &str) -> String {
        format!("{}/v1/mods/{}/files", BASE_URL, mod_id)
    }
}

/// Percent-encode a string for use in URL query parameter values.
///
/// Encodes all characters outside the unreserved set (A-Z a-z 0-9 `-` `_` `.` `~`).
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

#[async_trait::async_trait]
impl ModProvider for CurseForgeProvider {
    async fn search(
        &self,
        client: &dyn ProviderHttpClient,
        params: &SearchParams,
    ) -> Result<SearchResult, ProviderError> {
        let key = self.require_key()?;
        let url = Self::build_search_url(params);

        let (status, body) = client
            .get(&url, &[("x-api-key", key)])
            .await
            .map_err(ProviderError::from)?;

        if status != 200 {
            return Err(ProviderError::HttpStatus { status, body });
        }

        let raw: crate::core::providers::CfSearchResponse =
            serde_json::from_str(&body).map_err(|e| ProviderError::BadResponse(e.to_string()))?;

        let total = raw.pagination.total_count;
        let offset = raw.pagination.index;
        let hits = raw.data.into_iter().map(|m| m.into_summary()).collect();

        Ok(SearchResult { hits, offset, total })
    }

    async fn get_versions(
        &self,
        client: &dyn ProviderHttpClient,
        project_id: &str,
        mc_version: Option<&str>,
        loader: Option<&str>,
    ) -> Result<Vec<ProjectVersion>, ProviderError> {
        let key = self.require_key()?;
        let url = Self::build_files_url(project_id);

        let (status, body) = client
            .get(&url, &[("x-api-key", key)])
            .await
            .map_err(ProviderError::from)?;

        if status != 200 {
            return Err(ProviderError::HttpStatus { status, body });
        }

        let raw: CfFilesResponse =
            serde_json::from_str(&body).map_err(|e| ProviderError::BadResponse(e.to_string()))?;

        let versions: Vec<ProjectVersion> = raw
            .data
            .into_iter()
            .map(|f| f.into_project_version())
            .filter(|v| {
                // Client-side compatibility filter.
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
    //! No live HTTP in any test. All HTTP uses a capturing mock client backed by a
    //! pre-loaded response queue — same pattern as `modrinth.rs`.

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
        assert!(loaders.contains(&"forge".to_string()), "forge missing: {:?}", loaders);
        assert!(loaders.contains(&"neoforge".to_string()), "neoforge missing: {:?}", loaders);
    }

    #[test]
    fn split_unknown_entries_discarded() {
        let entries = vec!["Java 17".to_string(), "Windows".to_string(), "Client".to_string()];
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
        assert!(mc.is_empty(), "\"1x.2\" must not be classified as MC version");
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
        };

        let result = provider.search(&client, &params).await.unwrap();

        assert_eq!(result.hits.len(), 2);
        assert_eq!(result.total, 2);
        assert_eq!(result.offset, 0);

        let jei = &result.hits[0];
        assert_eq!(jei.provider, crate::core::providers::ProviderKind::CurseForge);
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
        };

        provider.search(&client, &params).await.unwrap();

        let urls = client.captured_urls().await;
        let url = &urls[0];
        assert!(url.contains("gameId=432"), "gameId missing: {url}");
        assert!(url.contains("classId=6"), "classId missing: {url}");
        assert!(url.contains("index=20"), "index/offset missing: {url}");
        assert!(url.contains("pageSize=10"), "pageSize/limit missing: {url}");
        assert!(url.contains("gameVersion=1.20.1"), "gameVersion missing: {url}");
        assert!(url.contains("modLoaderType=1"), "modLoaderType=1 (Forge) missing: {url}");
        // Default sort: Popularity desc, so a text query like "jei" surfaces the
        // well-known mod instead of CF's arbitrary unsorted default order.
        assert!(url.contains("sortField=2"), "sortField=2 (Popularity) missing: {url}");
        assert!(url.contains("sortOrder=desc"), "sortOrder=desc missing: {url}");
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
        assert_eq!(
            v.dependencies[0].project_id,
            Some("308702".to_string())
        );

        // Version 5034060 has relationType=2 (optional) and relationType=5 (incompatible).
        let v2 = versions.iter().find(|v| v.id == "5034060").unwrap();
        assert_eq!(v2.dependencies.len(), 2);
        let dep_types: Vec<&str> = v2.dependencies.iter().map(|d| d.dependency_type.as_str()).collect();
        assert!(dep_types.contains(&"optional"), "optional missing: {:?}", dep_types);
        assert!(dep_types.contains(&"incompatible"), "incompatible missing: {:?}", dep_types);
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

        assert_eq!(versions.len(), 1, "only forge+1.20.1 should pass: {:?}", versions.iter().map(|v| &v.id).collect::<Vec<_>>());
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

        assert_eq!(versions.len(), 3, "all 3 fixture files expected, got {}", versions.len());
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
        };

        let err = provider.search(&client, &params).await.unwrap_err();
        assert!(
            matches!(err, ProviderError::HttpStatus { status: 403, .. }),
            "expected HttpStatus(403), got {:?}",
            err
        );
    }
}
