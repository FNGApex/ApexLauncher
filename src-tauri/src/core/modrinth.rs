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
#[path = "modrinth_tests.rs"]
mod tests;
