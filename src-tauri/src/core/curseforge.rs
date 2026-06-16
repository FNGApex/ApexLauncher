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

/// Raw CF `/v1/mods/{project_id}/files/{file_id}` response.
#[derive(Debug, Deserialize)]
struct CfFileResponse {
    data: CfFileDetail,
}

/// Single-file detail record. Distinct from `CfFile` (the `/files` list entry) because
/// it additionally carries `hashes[]`, which the list endpoint omits.
#[derive(Debug, Deserialize)]
struct CfFileDetail {
    #[serde(rename = "fileName")]
    file_name: String,
    #[serde(rename = "fileLength")]
    file_length: Option<u64>,
    #[serde(rename = "fileSizeOnDisk")]
    file_size_on_disk: Option<u64>,
    #[serde(rename = "downloadUrl")]
    download_url: Option<String>,
    #[serde(default)]
    hashes: Vec<CfHash>,
}

#[derive(Debug, Deserialize)]
struct CfHash {
    value: String,
    algo: u32,
}

impl CfFileDetail {
    fn into_version_file(self) -> VersionFile {
        // Prefer sha1 (algo 1) over md5 (algo 2) — CF carries no sha256/sha512 here.
        let mut hashes = std::collections::HashMap::new();
        if let Some(h) = self.hashes.iter().find(|h| h.algo == 1) {
            hashes.insert("sha1".to_string(), h.value.clone());
        } else if let Some(h) = self.hashes.iter().find(|h| h.algo == 2) {
            hashes.insert("md5".to_string(), h.value.clone());
        }

        VersionFile {
            url: self.download_url,
            file_name: self.file_name,
            size: self.file_length.or(self.file_size_on_disk),
            hashes,
            primary: true,
        }
    }
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

    /// Build the CF `/v1/mods/{project_id}/files/{file_id}` URL.
    fn build_file_url(project_id: u32, file_id: u32) -> String {
        format!("{}/v1/mods/{}/files/{}", BASE_URL, project_id, file_id)
    }

    /// Resolve a single `(project_id, file_id)` to a normalized `VersionFile`.
    ///
    /// Used by the modpack importer (slice B) to resolve CF manifest entries to
    /// download URLs. `downloadUrl: null` maps to `url: None` exactly like
    /// `get_versions`. Hash preference: sha1 (algo 1) over md5 (algo 2) — CF
    /// `hashes[]` carries no SHA-256/512, so sha1 is the strongest available.
    pub async fn get_file(
        &self,
        client: &dyn ProviderHttpClient,
        project_id: u32,
        file_id: u32,
    ) -> Result<VersionFile, ProviderError> {
        let key = self.require_key()?;
        let url = Self::build_file_url(project_id, file_id);

        let (status, body) = client
            .get(&url, &[("x-api-key", key)])
            .await
            .map_err(ProviderError::from)?;

        if status != 200 {
            return Err(ProviderError::HttpStatus { status, body });
        }

        let raw: CfFileResponse =
            serde_json::from_str(&body).map_err(|e| ProviderError::BadResponse(e.to_string()))?;

        Ok(raw.data.into_version_file())
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
#[path = "curseforge_tests.rs"]
mod tests;
