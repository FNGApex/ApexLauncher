//! Mod install planner — Checkpoint 1 (Phase 5 slice B).
//!
//! `resolve_install` performs a BFS dependency walk from a root `ProjectVersion`,
//! calling the injected `ModProvider` / `ProviderHttpClient` to resolve required
//! transitive deps, and partitions results into:
//! - `downloads` — files with a URL (ready to queue for the download engine).
//! - `manual`    — files with `url == None` (distribution disabled; user must download).
//! - `unresolved` — deps where no compatible version was found.
//! - `suggestions` — optional dependencies surfaced to the user.
//! - `warnings`  — incompatible mods declared by the resolved graph.
//!
//! No filesystem I/O, no manifest mutation, no Tauri commands live here.
//! All network access is via the injected `ProviderHttpClient`.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::core::providers::{ModProvider, ProjectVersion, ProviderHttpClient, ProviderKind};

// ── Output types ──────────────────────────────────────────────────────────────

/// A resolved mod file that can be handed to the download engine.
///
/// Carries everything needed to produce a `DownloadItem` (CP2) and a `ModEntry`
/// (see `instances.rs:51-59`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlannedMod {
    /// Which provider owns this mod.
    pub provider: ProviderKind,
    /// Provider-specific project identifier.
    pub project_id: String,
    /// Provider-specific version identifier.
    pub version_id: String,
    /// Filename as declared by the provider (used for `mc/mods/<file_name>`).
    pub file_name: String,
    /// Direct download URL (always `Some` on a `PlannedMod`; `None` routes to `ManualMod`).
    pub url: String,
    /// Verification hashes (e.g. `"sha512"` → `"<hex>"`).
    pub hashes: std::collections::HashMap<String, String>,
    /// Whether this is the primary file for the version.
    pub primary: bool,
    /// Client/server side declared by the provider (e.g. `"required"`, `"optional"`, `"unsupported"`).
    pub side: String,
    /// Best-effort project page URL for the mod (used as fallback link in UI).
    pub page_url: String,
}

/// A resolved mod file whose URL is `None` (distribution disabled).
///
/// The user must manually download the file from `page_url`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManualMod {
    /// Which provider owns this mod.
    pub provider: ProviderKind,
    /// Provider-specific project identifier.
    pub project_id: String,
    /// Provider-specific version identifier.
    pub version_id: String,
    /// Filename as declared by the provider.
    pub file_name: String,
    /// Project page URL — open in browser so user can download manually.
    pub page_url: String,
}

/// A dependency for which no compatible version could be found.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UnresolvedDep {
    /// Provider-specific project id of the unresolvable dep.
    pub project_id: String,
    /// Human-readable reason (e.g. `"no compatible version found"`).
    pub reason: String,
}

/// An optional dependency surfaced as a suggestion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    /// Provider-specific project id.
    pub project_id: String,
    /// Version id, if it was specified on the dependency (may be `None`).
    pub version_id: Option<String>,
}

/// A declared-incompatible mod.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IncompatibleWarning {
    /// Provider-specific project id of the incompatible mod.
    pub project_id: String,
}

/// Result of `resolve_install`: a fully-partitioned dependency plan.
///
/// Callers (CP2) build `DownloadItem`s from `downloads`, prompt for `manual`,
/// surface `unresolved`/`suggestions`/`warnings` to the UI.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstallPlan {
    /// Mods with a URL — ready for the download engine.
    pub downloads: Vec<PlannedMod>,
    /// Mods without a URL — user must download manually.
    pub manual: Vec<ManualMod>,
    /// Deps where no compatible version could be found.
    pub unresolved: Vec<UnresolvedDep>,
    /// Optional deps surfaced to the user (not installed automatically).
    pub suggestions: Vec<Suggestion>,
    /// Incompatible mods declared by the resolved graph.
    pub warnings: Vec<IncompatibleWarning>,
}

// ── Page URL helpers ──────────────────────────────────────────────────────────

/// Build a best-effort project page URL for a mod.
///
/// `slug_or_id` is used as the path component — callers pass the slug when
/// available (root mod) or fall back to the project_id for deps.
pub fn page_url_for(provider: ProviderKind, slug_or_id: &str) -> String {
    match provider {
        ProviderKind::Modrinth => format!("https://modrinth.com/mod/{slug_or_id}"),
        ProviderKind::CurseForge => {
            format!("https://www.curseforge.com/minecraft/mc-mods/{slug_or_id}")
        }
    }
}

// ── Planner ───────────────────────────────────────────────────────────────────

/// Resolve a full install plan for a mod, walking required transitive dependencies.
///
/// # Parameters
/// - `provider`          — the `ModProvider` used to fetch dep version lists.
/// - `client`            — injected HTTP client (mock in tests, `ReqwestProviderClient` in prod).
/// - `root_version`      — the already-fetched `ProjectVersion` for the mod being installed.
/// - `root_project_id`   — provider project_id for the root mod (e.g. Modrinth base62 string).
/// - `root_slug`         — URL slug of the root project (used for page URL; deps fall back to project_id).
/// - `mc_version`        — Minecraft version string to filter dep versions (e.g. `"1.21"`).
/// - `loader`            — mod loader string (e.g. `"fabric"`).
/// - `already_installed` — set of `project_id` strings already present in the instance manifest.
///
/// # Behaviour
/// BFS over `dependencies[]`:
/// - `required`     → resolve newest compatible version, recurse; `url==None` → `manual`.
/// - `optional`     → add to `suggestions`, stop.
/// - `incompatible` → add to `warnings`, stop.
/// - `embedded`     → ignore.
/// - Already in `already_installed` OR already `visited` → skip (dedup + cycle guard).
/// - No compatible version → `unresolved` entry; does not abort.
///
/// The root version itself is always the first entry in `downloads` (or `manual`).
pub async fn resolve_install(
    provider: &dyn ModProvider,
    client: &dyn ProviderHttpClient,
    root_version: ProjectVersion,
    root_project_id: &str,
    root_slug: &str,
    mc_version: &str,
    loader: &str,
    already_installed: &HashSet<String>,
) -> InstallPlan {
    let mut plan = InstallPlan::default();
    // `visited` guards against cycles and duplicate dep resolutions.
    // Keyed by the provider-specific project_id string (same namespace as dep.project_id).
    let mut visited: HashSet<String> = HashSet::new();

    // Record the root project as visited immediately so any dep pointing back
    // to the root project is skipped.
    visited.insert(root_project_id.to_string());

    // Queue entries: (version, project_id, slug_or_id_for_page_url).
    let mut queue: Vec<(ProjectVersion, String, String)> =
        vec![(root_version, root_project_id.to_string(), root_slug.to_string())];

    while !queue.is_empty() {
        let (version, project_id, slug) = queue.remove(0);
        let provider_kind = version.provider;
        let version_id = version.id.clone();

        // Find the primary file (or first file).
        let primary_file = version
            .files
            .iter()
            .find(|f| f.primary)
            .or_else(|| version.files.first());

        if let Some(file) = primary_file {
            let page = page_url_for(provider_kind, &slug);
            if let Some(url) = &file.url {
                plan.downloads.push(PlannedMod {
                    provider: provider_kind,
                    project_id: project_id.clone(),
                    version_id: version_id.clone(),
                    file_name: file.file_name.clone(),
                    url: url.clone(),
                    hashes: file.hashes.clone(),
                    primary: file.primary,
                    side: "unknown".to_string(), // providers don't surface per-file side yet
                    page_url: page,
                });
            } else {
                plan.manual.push(ManualMod {
                    provider: provider_kind,
                    project_id: project_id.clone(),
                    version_id: version_id.clone(),
                    file_name: file.file_name.clone(),
                    page_url: page_url_for(provider_kind, &slug),
                });
            }
        }
        // (If there are no files at all we still process deps — unusual but not fatal.)

        // Walk dependencies.
        for dep in &version.dependencies {
            match dep.dependency_type.as_str() {
                "optional" => {
                    if let Some(pid) = &dep.project_id {
                        plan.suggestions.push(Suggestion {
                            project_id: pid.clone(),
                            version_id: dep.version_id.clone(),
                        });
                    }
                }
                "incompatible" => {
                    if let Some(pid) = &dep.project_id {
                        plan.warnings.push(IncompatibleWarning {
                            project_id: pid.clone(),
                        });
                    }
                }
                "embedded" => {
                    // Ignore embedded — bundled inside the parent jar.
                }
                "required" => {
                    let Some(pid) = &dep.project_id else {
                        // No project_id on a required dep — nothing we can do.
                        continue;
                    };

                    // Dedup: already in manifest or already visited in this BFS.
                    if already_installed.contains(pid) || visited.contains(pid) {
                        continue;
                    }
                    visited.insert(pid.clone());

                    // If the dep specifies an exact version_id, use it.
                    let resolved = if let Some(vid) = &dep.version_id {
                        // Fetch the specific version by getting all versions and
                        // filtering — providers don't expose a single-version endpoint
                        // in the current trait.  Fall back to newest-compatible.
                        fetch_newest_compatible(provider, client, pid, mc_version, loader, Some(vid))
                            .await
                    } else {
                        fetch_newest_compatible(provider, client, pid, mc_version, loader, None)
                            .await
                    };

                    match resolved {
                        Some(dep_version) => {
                            // page URL for a dep uses its project_id as the slug
                            // (best effort; slug not available from Dependency struct).
                            queue.push((dep_version, pid.clone(), pid.clone()));
                        }
                        None => {
                            plan.unresolved.push(UnresolvedDep {
                                project_id: pid.clone(),
                                reason: "no compatible version found".to_string(),
                            });
                        }
                    }
                }
                _ => {
                    // Unknown dep type — ignore defensively.
                }
            }
        }
    }

    plan
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Fetch the newest compatible version of `project_id`.
///
/// If `preferred_version_id` is `Some`, try to return that specific version first;
/// fall back to first returned version if not found (providers may not support
/// single-version lookup in `get_versions`).
async fn fetch_newest_compatible(
    provider: &dyn ModProvider,
    client: &dyn ProviderHttpClient,
    project_id: &str,
    mc_version: &str,
    loader: &str,
    preferred_version_id: Option<&str>,
) -> Option<ProjectVersion> {
    let versions = provider
        .get_versions(client, project_id, Some(mc_version), Some(loader))
        .await
        .ok()?;

    if versions.is_empty() {
        return None;
    }

    // If a specific version was requested, prefer it.
    if let Some(vid) = preferred_version_id {
        if let Some(v) = versions.iter().find(|v| v.id == vid) {
            return Some(v.clone());
        }
    }

    // Providers return newest first; take the first one with a file.
    versions.into_iter().find(|v| !v.files.is_empty())
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::providers::{
        Dependency, ProjectVersion, ProviderError, ProviderHttpClient, ProviderKind, SearchParams,
        SearchResult, VersionFile,
    };
    use std::collections::{HashMap, HashSet, VecDeque};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    // ── Mock HTTP client ──────────────────────────────────────────────────────

    struct MockResp(u16, String);

    struct MockProviderClient {
        responses: Arc<Mutex<VecDeque<MockResp>>>,
    }

    impl MockProviderClient {
        fn new(responses: Vec<MockResp>) -> Self {
            Self {
                responses: Arc::new(Mutex::new(responses.into_iter().collect())),
            }
        }
    }

    #[async_trait::async_trait]
    impl ProviderHttpClient for MockProviderClient {
        async fn get(
            &self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> Result<(u16, String), reqwest::Error> {
            let mut q = self.responses.lock().await;
            let MockResp(s, b) = q
                .pop_front()
                .expect("MockProviderClient: no more canned responses");
            Ok((s, b))
        }
    }

    // ── Mock provider ─────────────────────────────────────────────────────────

    /// A mock `ModProvider` whose `get_versions` returns pre-loaded responses.
    struct MockProvider {
        /// Each call to `get_versions` pops the next list.
        version_lists: Arc<Mutex<VecDeque<Result<Vec<ProjectVersion>, ProviderError>>>>,
    }

    impl MockProvider {
        fn new(version_lists: Vec<Result<Vec<ProjectVersion>, ProviderError>>) -> Self {
            Self {
                version_lists: Arc::new(Mutex::new(version_lists.into_iter().collect())),
            }
        }
    }

    #[async_trait::async_trait]
    impl crate::core::providers::ModProvider for MockProvider {
        async fn search(
            &self,
            _client: &dyn ProviderHttpClient,
            _params: &SearchParams,
        ) -> Result<SearchResult, ProviderError> {
            unimplemented!("search not used by planner")
        }

        async fn get_versions(
            &self,
            _client: &dyn ProviderHttpClient,
            _project_id: &str,
            _mc_version: Option<&str>,
            _loader: Option<&str>,
        ) -> Result<Vec<ProjectVersion>, ProviderError> {
            let mut q = self.version_lists.lock().await;
            q.pop_front()
                .expect("MockProvider: no more canned version lists")
        }
    }

    // ── Fixture helpers ───────────────────────────────────────────────────────

    fn make_file(url: Option<&str>, primary: bool) -> VersionFile {
        let mut hashes = HashMap::new();
        hashes.insert("sha512".to_string(), "abc123".to_string());
        VersionFile {
            url: url.map(|s| s.to_string()),
            file_name: "mod.jar".to_string(),
            size: Some(1024),
            hashes,
            primary,
        }
    }

    fn make_version(
        _project_id_in_id: &str, // not used — version_id is the unique key in ProjectVersion
        version_id: &str,
        files: Vec<VersionFile>,
        deps: Vec<Dependency>,
    ) -> ProjectVersion {
        ProjectVersion {
            provider: ProviderKind::Modrinth,
            id: version_id.to_string(),
            name: format!("Mod {version_id}"),
            version_number: "1.0.0".to_string(),
            game_versions: vec!["1.21".to_string()],
            loaders: vec!["fabric".to_string()],
            files,
            dependencies: deps,
        }
    }

    fn req_dep(project_id: &str) -> Dependency {
        Dependency {
            project_id: Some(project_id.to_string()),
            version_id: None,
            dependency_type: "required".to_string(),
        }
    }

    fn req_dep_vid(project_id: &str, version_id: &str) -> Dependency {
        Dependency {
            project_id: Some(project_id.to_string()),
            version_id: Some(version_id.to_string()),
            dependency_type: "required".to_string(),
        }
    }

    fn opt_dep(project_id: &str) -> Dependency {
        Dependency {
            project_id: Some(project_id.to_string()),
            version_id: None,
            dependency_type: "optional".to_string(),
        }
    }

    fn incompat_dep(project_id: &str) -> Dependency {
        Dependency {
            project_id: Some(project_id.to_string()),
            version_id: None,
            dependency_type: "incompatible".to_string(),
        }
    }

    fn embedded_dep(project_id: &str) -> Dependency {
        Dependency {
            project_id: Some(project_id.to_string()),
            version_id: None,
            dependency_type: "embedded".to_string(),
        }
    }

    // ── Tests ─────────────────────────────────────────────────────────────────

    /// Root-only: no deps → single download entry.
    #[tokio::test]
    async fn root_only_no_deps() {
        let root = make_version("sodium", "v1", vec![make_file(Some("https://dl.example.com/sodium.jar"), true)], vec![]);
        let provider = MockProvider::new(vec![]);
        let client = MockProviderClient::new(vec![]);
        let plan = resolve_install(
            &provider, &client, root, "sodium", "sodium", "1.21", "fabric", &HashSet::new(),
        )
        .await;

        assert_eq!(plan.downloads.len(), 1);
        assert_eq!(plan.downloads[0].url, "https://dl.example.com/sodium.jar");
        assert!(plan.manual.is_empty());
        assert!(plan.unresolved.is_empty());
        assert!(plan.suggestions.is_empty());
        assert!(plan.warnings.is_empty());
    }

    /// Required dep is recursed and both root + dep appear in downloads.
    #[tokio::test]
    async fn required_dep_recursed() {
        let dep_version = make_version(
            "fabric-api",
            "dep-v1",
            vec![make_file(Some("https://dl.example.com/fabric-api.jar"), true)],
            vec![],
        );
        let root = make_version(
            "sodium",
            "v1",
            vec![make_file(Some("https://dl.example.com/sodium.jar"), true)],
            vec![req_dep("fabric-api")],
        );
        // Provider will be called once for dep "fabric-api".
        let provider = MockProvider::new(vec![Ok(vec![dep_version])]);
        let client = MockProviderClient::new(vec![]);
        let plan = resolve_install(
            &provider, &client, root, "sodium", "sodium", "1.21", "fabric", &HashSet::new(),
        )
        .await;

        assert_eq!(plan.downloads.len(), 2, "root + dep both downloaded");
        assert!(plan.manual.is_empty());
        assert!(plan.unresolved.is_empty());
    }

    /// Transitive dep (dep of dep) is also recursed.
    #[tokio::test]
    async fn transitive_dep_recursed() {
        // dep-b depends on dep-c
        let dep_c = make_version(
            "dep-c",
            "dep-c-v1",
            vec![make_file(Some("https://dl.example.com/c.jar"), true)],
            vec![],
        );
        let dep_b = make_version(
            "dep-b",
            "dep-b-v1",
            vec![make_file(Some("https://dl.example.com/b.jar"), true)],
            vec![req_dep("dep-c")],
        );
        let root = make_version(
            "root-mod",
            "root-v1",
            vec![make_file(Some("https://dl.example.com/root.jar"), true)],
            vec![req_dep("dep-b")],
        );
        let provider = MockProvider::new(vec![Ok(vec![dep_b]), Ok(vec![dep_c])]);
        let client = MockProviderClient::new(vec![]);
        let plan = resolve_install(
            &provider, &client, root, "root-mod", "root-mod", "1.21", "fabric", &HashSet::new(),
        )
        .await;

        assert_eq!(plan.downloads.len(), 3, "root + dep-b + dep-c");
    }

    /// Cycle guard: dep-a depends on dep-b which depends on dep-a again.
    #[tokio::test]
    async fn cycle_guard_prevents_infinite_loop() {
        // dep-a and dep-b point at each other; each project will be visited at most once.
        let dep_b = make_version(
            "dep-b",
            "dep-b-v1",
            vec![make_file(Some("https://dl.example.com/b.jar"), true)],
            vec![req_dep("dep-a")], // points back at dep-a
        );
        let root = make_version(
            "dep-a",
            "root-v1",
            vec![make_file(Some("https://dl.example.com/a.jar"), true)],
            vec![req_dep("dep-b")],
        );
        // Provider called once: for dep-b (dep-a is already visited as root).
        let provider = MockProvider::new(vec![Ok(vec![dep_b])]);
        let client = MockProviderClient::new(vec![]);
        let plan = resolve_install(
            &provider, &client, root, "dep-a", "dep-a", "1.21", "fabric", &HashSet::new(),
        )
        .await;

        assert_eq!(plan.downloads.len(), 2, "root + dep-b; dep-a not re-added");
        assert!(plan.unresolved.is_empty());
    }

    /// Already-installed project is skipped.
    #[tokio::test]
    async fn already_installed_dep_skipped() {
        let root = make_version(
            "sodium",
            "v1",
            vec![make_file(Some("https://dl.example.com/sodium.jar"), true)],
            vec![req_dep("fabric-api")],
        );
        let mut already: HashSet<String> = HashSet::new();
        already.insert("fabric-api".to_string());

        // Provider should NOT be called — dep is already installed.
        let provider = MockProvider::new(vec![]);
        let client = MockProviderClient::new(vec![]);
        let plan = resolve_install(
            &provider, &client, root, "sodium", "sodium", "1.21", "fabric", &already,
        )
        .await;

        assert_eq!(plan.downloads.len(), 1, "only root downloaded");
        assert!(plan.unresolved.is_empty());
    }

    /// `url == None` on root → manual entry, not download.
    #[tokio::test]
    async fn url_none_produces_manual_entry() {
        let root = make_version(
            "cf-mod",
            "v1",
            vec![make_file(None, true)], // no URL
            vec![],
        );
        let provider = MockProvider::new(vec![]);
        let client = MockProviderClient::new(vec![]);
        let plan = resolve_install(
            &provider, &client, root, "cf-mod", "cf-mod", "1.21", "fabric", &HashSet::new(),
        )
        .await;

        assert!(plan.downloads.is_empty(), "no URL → not in downloads");
        assert_eq!(plan.manual.len(), 1);
        assert_eq!(plan.manual[0].project_id, "cf-mod");
    }

    /// No compatible version for a required dep → `unresolved` entry; does not abort.
    #[tokio::test]
    async fn no_compatible_version_yields_unresolved() {
        let root = make_version(
            "sodium",
            "v1",
            vec![make_file(Some("https://dl.example.com/sodium.jar"), true)],
            vec![req_dep("missing-dep")],
        );
        // Provider returns empty list for "missing-dep".
        let provider = MockProvider::new(vec![Ok(vec![])]);
        let client = MockProviderClient::new(vec![]);
        let plan = resolve_install(
            &provider, &client, root, "sodium", "sodium", "1.21", "fabric", &HashSet::new(),
        )
        .await;

        assert_eq!(plan.downloads.len(), 1, "root still downloaded");
        assert_eq!(plan.unresolved.len(), 1);
        assert_eq!(plan.unresolved[0].project_id, "missing-dep");
    }

    /// Provider error for a dep → treated as unresolved (does not abort).
    #[tokio::test]
    async fn provider_error_yields_unresolved() {
        let root = make_version(
            "sodium",
            "v1",
            vec![make_file(Some("https://dl.example.com/sodium.jar"), true)],
            vec![req_dep("broken-dep")],
        );
        let provider = MockProvider::new(vec![Err(ProviderError::BadResponse(
            "parse error".to_string(),
        ))]);
        let client = MockProviderClient::new(vec![]);
        let plan = resolve_install(
            &provider, &client, root, "sodium", "sodium", "1.21", "fabric", &HashSet::new(),
        )
        .await;

        assert_eq!(plan.downloads.len(), 1);
        assert_eq!(plan.unresolved.len(), 1);
        assert_eq!(plan.unresolved[0].project_id, "broken-dep");
    }

    /// Optional dep → suggestion, not downloaded.
    #[tokio::test]
    async fn optional_dep_becomes_suggestion() {
        let root = make_version(
            "sodium",
            "v1",
            vec![make_file(Some("https://dl.example.com/sodium.jar"), true)],
            vec![opt_dep("indium")],
        );
        let provider = MockProvider::new(vec![]); // should NOT be called
        let client = MockProviderClient::new(vec![]);
        let plan = resolve_install(
            &provider, &client, root, "sodium", "sodium", "1.21", "fabric", &HashSet::new(),
        )
        .await;

        assert_eq!(plan.downloads.len(), 1);
        assert_eq!(plan.suggestions.len(), 1);
        assert_eq!(plan.suggestions[0].project_id, "indium");
        assert!(plan.unresolved.is_empty());
    }

    /// Incompatible dep → warning, not downloaded.
    #[tokio::test]
    async fn incompatible_dep_becomes_warning() {
        let root = make_version(
            "lithium",
            "v1",
            vec![make_file(Some("https://dl.example.com/lithium.jar"), true)],
            vec![incompat_dep("optifine")],
        );
        let provider = MockProvider::new(vec![]); // should NOT be called
        let client = MockProviderClient::new(vec![]);
        let plan = resolve_install(
            &provider, &client, root, "lithium", "lithium", "1.21", "fabric", &HashSet::new(),
        )
        .await;

        assert_eq!(plan.downloads.len(), 1);
        assert_eq!(plan.warnings.len(), 1);
        assert_eq!(plan.warnings[0].project_id, "optifine");
    }

    /// Embedded dep → ignored entirely.
    #[tokio::test]
    async fn embedded_dep_ignored() {
        let root = make_version(
            "sodium",
            "v1",
            vec![make_file(Some("https://dl.example.com/sodium.jar"), true)],
            vec![embedded_dep("bundled-lib")],
        );
        let provider = MockProvider::new(vec![]);
        let client = MockProviderClient::new(vec![]);
        let plan = resolve_install(
            &provider, &client, root, "sodium", "sodium", "1.21", "fabric", &HashSet::new(),
        )
        .await;

        assert_eq!(plan.downloads.len(), 1);
        assert!(plan.suggestions.is_empty());
        assert!(plan.warnings.is_empty());
        assert!(plan.unresolved.is_empty());
    }

    /// Dep with explicit `version_id` → that specific version is preferred.
    #[tokio::test]
    async fn dep_with_explicit_version_id_preferred() {
        let specific = make_version(
            "fabric-api",
            "fa-v2",
            vec![make_file(Some("https://dl.example.com/fa-v2.jar"), true)],
            vec![],
        );
        let other = make_version(
            "fabric-api",
            "fa-v3",
            vec![make_file(Some("https://dl.example.com/fa-v3.jar"), true)],
            vec![],
        );
        let root = make_version(
            "sodium",
            "v1",
            vec![make_file(Some("https://dl.example.com/sodium.jar"), true)],
            vec![req_dep_vid("fabric-api", "fa-v2")],
        );
        // Provider returns v3 first (newest), then v2; we expect v2 to be picked.
        let provider = MockProvider::new(vec![Ok(vec![other, specific])]);
        let client = MockProviderClient::new(vec![]);
        let plan = resolve_install(
            &provider, &client, root, "sodium", "sodium", "1.21", "fabric", &HashSet::new(),
        )
        .await;

        assert_eq!(plan.downloads.len(), 2);
        // The dep entry should be fa-v2, not fa-v3.
        let dep_entry = plan
            .downloads
            .iter()
            .find(|d| d.version_id == "fa-v2")
            .expect("fa-v2 should be in plan");
        assert_eq!(dep_entry.url, "https://dl.example.com/fa-v2.jar");
    }

    /// Page URL is built correctly for Modrinth.
    #[tokio::test]
    async fn page_url_modrinth_uses_slug() {
        let root = make_version(
            "sodium",
            "v1",
            vec![make_file(Some("https://dl.example.com/sodium.jar"), true)],
            vec![],
        );
        let provider = MockProvider::new(vec![]);
        let client = MockProviderClient::new(vec![]);
        let plan = resolve_install(
            &provider, &client, root, "sodium", "sodium", "1.21", "fabric", &HashSet::new(),
        )
        .await;

        assert_eq!(
            plan.downloads[0].page_url,
            "https://modrinth.com/mod/sodium"
        );
    }

    /// Duplicate required dep from two different parents → resolved only once.
    #[tokio::test]
    async fn duplicate_dep_from_two_parents_resolved_once() {
        // Both dep-a and dep-b require "shared-dep".
        let shared = make_version(
            "shared-dep",
            "shared-v1",
            vec![make_file(Some("https://dl.example.com/shared.jar"), true)],
            vec![],
        );
        let dep_b = make_version(
            "dep-b",
            "dep-b-v1",
            vec![make_file(Some("https://dl.example.com/b.jar"), true)],
            vec![req_dep("shared-dep")], // also requires shared-dep
        );
        let root = make_version(
            "root",
            "root-v1",
            vec![make_file(Some("https://dl.example.com/root.jar"), true)],
            // root requires both dep-b and shared-dep directly
            vec![req_dep("dep-b"), req_dep("shared-dep")],
        );
        // Provider called: once for dep-b, once for shared-dep (from root).
        // dep-b's inner dep on shared-dep should be skipped via visited set.
        let provider = MockProvider::new(vec![Ok(vec![dep_b]), Ok(vec![shared])]);
        let client = MockProviderClient::new(vec![]);
        let plan = resolve_install(
            &provider, &client, root, "root", "root", "1.21", "fabric", &HashSet::new(),
        )
        .await;

        // root + dep-b + shared-dep = 3, not 4
        assert_eq!(plan.downloads.len(), 3);
    }
}
