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

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::core::download::{DownloadItem, ExpectedHash, PlanResult};
use crate::core::instances::ModEntry;
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

// ── CP2: Install executor helpers ────────────────────────────────────────────

/// A single file download that failed during `add_mod`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FailedMod {
    /// The `file_name` of the `PlannedMod` that failed.
    pub file_name: String,
    /// Human-readable error description.
    pub error: String,
}

/// Structured result returned by the `add_mod` Tauri command.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AddModResult {
    /// `ModEntry` records for every successfully downloaded mod.
    pub added: Vec<ModEntry>,
    /// Mods that could not be auto-downloaded (distribution disabled).
    pub manual: Vec<ManualMod>,
    /// Dependencies with no compatible version.
    pub unresolved: Vec<UnresolvedDep>,
    /// Optional dependencies surfaced to the user.
    pub suggestions: Vec<Suggestion>,
    /// Incompatible mods declared by the dependency graph.
    pub warnings: Vec<IncompatibleWarning>,
    /// Mods that had a URL but whose download failed at runtime.
    pub failed: Vec<FailedMod>,
}

/// Map `ProviderKind` to the lowercase string stored in `ModEntry.provider`.
///
/// `ProviderKind` serializes differently (camelCase enum variant) — we need a
/// stable lowercase ASCII string that the frontend and manifest read uniformly.
fn provider_kind_str(kind: ProviderKind) -> String {
    match kind {
        ProviderKind::Modrinth => "modrinth".to_string(),
        ProviderKind::CurseForge => "curseforge".to_string(),
    }
}

/// Build `DownloadItem`s from the downloadable entries in an `InstallPlan`.
///
/// One `DownloadItem` per `PlannedMod` in `plan.downloads`:
/// - `dest` = `mods_dir.join(&planned.file_name)`
/// - `url`  = `planned.url`
/// - `size` = first `VersionFile.size` (stored on `PlannedMod` — if none, `None`)
/// - `expected_hash` = sha512 preferred, sha1 fallback, else `None`
pub fn build_download_items(plan: &InstallPlan, mods_dir: &Path) -> Vec<DownloadItem> {
    plan.downloads
        .iter()
        .map(|p| {
            let expected_hash = if let Some(h) = p.hashes.get("sha512") {
                Some(ExpectedHash::Sha512(h.clone()))
            } else if let Some(h) = p.hashes.get("sha1") {
                Some(ExpectedHash::Sha1(h.clone()))
            } else {
                None
            };
            DownloadItem {
                url: p.url.clone(),
                dest: mods_dir.join(&p.file_name),
                expected_hash,
                size: None, // PlannedMod doesn't carry file size (VersionFile.size not propagated in CP1)
            }
        })
        .collect()
}

/// Convert a successfully-resolved `PlannedMod` into a `ModEntry` for the manifest.
///
/// `enabled` is always `true` (new installs are enabled by default).
pub fn planned_to_mod_entry(p: &PlannedMod) -> ModEntry {
    let mut hashes: BTreeMap<String, String> = BTreeMap::new();
    for (k, v) in &p.hashes {
        hashes.insert(k.clone(), v.clone());
    }
    ModEntry {
        provider: provider_kind_str(p.provider),
        project_id: p.project_id.clone(),
        version_id: p.version_id.clone(),
        file_name: p.file_name.clone(),
        hashes,
        enabled: true,
        side: p.side.clone(),
    }
}

/// Merge `new` entries into `existing`, skipping any whose `project_id` or
/// `file_name` is already present. Idempotent.
pub fn merge_mod_entries(existing: &mut Vec<ModEntry>, new: Vec<ModEntry>) {
    for entry in new {
        let already = existing
            .iter()
            .any(|e| e.project_id == entry.project_id || e.file_name == entry.file_name);
        if !already {
            existing.push(entry);
        }
    }
}

/// Attribute download outcomes back to their `PlannedMod` by URL (order-independent).
///
/// `execute_plan` returns outcomes in completion order (via `FuturesUnordered`) and
/// prepends already-skipped items — index-based matching is wrong. Match by
/// `outcome.url == planned.url` instead.
///
/// Routing:
/// - `Ok` | `Skipped` → file is present on disk → build `ModEntry` via
///   `planned_to_mod_entry` and push to the first return vector.
/// - `Failed { error }` → push `FailedMod` to the second return vector.
/// - No matching outcome (shouldn't happen in practice) → treat as failed with a
///   descriptive error string.
pub fn attribute_outcomes(
    downloads: &[PlannedMod],
    result: &PlanResult,
) -> (Vec<ModEntry>, Vec<FailedMod>) {
    use crate::core::download::ItemStatus;

    let mut added: Vec<ModEntry> = Vec::new();
    let mut failed: Vec<FailedMod> = Vec::new();

    for planned in downloads {
        match result.outcomes.iter().find(|o| o.url == planned.url) {
            Some(outcome) => match &outcome.status {
                ItemStatus::Ok | ItemStatus::Skipped => {
                    added.push(planned_to_mod_entry(planned));
                }
                ItemStatus::Failed { error } => {
                    failed.push(FailedMod {
                        file_name: planned.file_name.clone(),
                        error: error.clone(),
                    });
                }
            },
            None => {
                failed.push(FailedMod {
                    file_name: planned.file_name.clone(),
                    error: format!(
                        "no outcome recorded for url '{}' (internal error)",
                        planned.url
                    ),
                });
            }
        }
    }

    (added, failed)
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

    // ── CP2: pure helper tests ────────────────────────────────────────────────

    use super::{attribute_outcomes, build_download_items, merge_mod_entries, planned_to_mod_entry, AddModResult};
    use crate::core::download::ExpectedHash;
    use crate::core::instances::ModEntry;
    use std::path::Path;

    fn make_planned(file_name: &str, url: &str, sha512: Option<&str>, sha1: Option<&str>) -> super::PlannedMod {
        let mut hashes = HashMap::new();
        if let Some(h) = sha512 {
            hashes.insert("sha512".to_string(), h.to_string());
        }
        if let Some(h) = sha1 {
            hashes.insert("sha1".to_string(), h.to_string());
        }
        super::PlannedMod {
            provider: ProviderKind::Modrinth,
            project_id: "proj-a".to_string(),
            version_id: "v1".to_string(),
            file_name: file_name.to_string(),
            url: url.to_string(),
            hashes,
            primary: true,
            side: "unknown".to_string(),
            page_url: "https://modrinth.com/mod/proj-a".to_string(),
        }
    }

    /// `build_download_items` produces correct dest path = `mods_dir/<file_name>`.
    #[test]
    fn build_download_items_dest_path() {
        let plan = InstallPlan {
            downloads: vec![make_planned("sodium.jar", "https://dl.example.com/sodium.jar", Some("abc"), None)],
            ..Default::default()
        };
        let mods_dir = Path::new("/instances/my-instance/mc/mods");
        let items = build_download_items(&plan, mods_dir);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].dest, mods_dir.join("sodium.jar"));
        assert_eq!(items[0].url, "https://dl.example.com/sodium.jar");
    }

    /// sha512 is preferred over sha1.
    #[test]
    fn build_download_items_prefers_sha512() {
        let plan = InstallPlan {
            downloads: vec![make_planned("mod.jar", "https://dl.example.com/mod.jar", Some("sha512hex"), Some("sha1hex"))],
            ..Default::default()
        };
        let items = build_download_items(&plan, Path::new("/mods"));
        assert_eq!(items[0].expected_hash, Some(ExpectedHash::Sha512("sha512hex".to_string())));
    }

    /// sha1 used when sha512 absent.
    #[test]
    fn build_download_items_falls_back_to_sha1() {
        let plan = InstallPlan {
            downloads: vec![make_planned("mod.jar", "https://dl.example.com/mod.jar", None, Some("sha1hex"))],
            ..Default::default()
        };
        let items = build_download_items(&plan, Path::new("/mods"));
        assert_eq!(items[0].expected_hash, Some(ExpectedHash::Sha1("sha1hex".to_string())));
    }

    /// No hash present → `expected_hash = None`.
    #[test]
    fn build_download_items_no_hash_gives_none() {
        let plan = InstallPlan {
            downloads: vec![make_planned("mod.jar", "https://dl.example.com/mod.jar", None, None)],
            ..Default::default()
        };
        let items = build_download_items(&plan, Path::new("/mods"));
        assert_eq!(items[0].expected_hash, None);
    }

    /// Empty plan → empty items.
    #[test]
    fn build_download_items_empty_plan() {
        let plan = InstallPlan::default();
        let items = build_download_items(&plan, Path::new("/mods"));
        assert!(items.is_empty());
    }

    /// `planned_to_mod_entry` maps fields correctly and sets `enabled = true`.
    #[test]
    fn planned_to_mod_entry_maps_fields() {
        let planned = make_planned("sodium.jar", "https://dl.example.com/sodium.jar", Some("deadbeef"), None);
        let entry = planned_to_mod_entry(&planned);
        assert_eq!(entry.provider, "modrinth");
        assert_eq!(entry.project_id, "proj-a");
        assert_eq!(entry.version_id, "v1");
        assert_eq!(entry.file_name, "sodium.jar");
        assert!(entry.enabled);
        assert_eq!(entry.side, "unknown");
        assert_eq!(entry.hashes.get("sha512"), Some(&"deadbeef".to_string()));
    }

    /// `merge_mod_entries` appends new entries that don't clash.
    #[test]
    fn merge_mod_entries_appends_new() {
        let mut existing: Vec<ModEntry> = Vec::new();
        let entry = planned_to_mod_entry(&make_planned("sodium.jar", "https://example.com/a.jar", None, None));
        merge_mod_entries(&mut existing, vec![entry.clone()]);
        assert_eq!(existing.len(), 1);
        assert_eq!(existing[0].project_id, "proj-a");
    }

    /// `merge_mod_entries` skips entry with duplicate `project_id`.
    #[test]
    fn merge_mod_entries_dedup_by_project_id() {
        let entry = planned_to_mod_entry(&make_planned("sodium.jar", "https://example.com/a.jar", None, None));
        let mut existing = vec![entry.clone()];
        // Try to add same project_id again with a different file_name — should be skipped.
        let mut dup = entry.clone();
        dup.file_name = "sodium-v2.jar".to_string();
        merge_mod_entries(&mut existing, vec![dup]);
        assert_eq!(existing.len(), 1, "duplicate project_id must be skipped");
    }

    /// `merge_mod_entries` skips entry with duplicate `file_name` even if project_id differs.
    #[test]
    fn merge_mod_entries_dedup_by_file_name() {
        let entry = planned_to_mod_entry(&make_planned("sodium.jar", "https://example.com/a.jar", None, None));
        let mut existing = vec![entry];
        let mut dup = planned_to_mod_entry(&make_planned("sodium.jar", "https://example.com/b.jar", None, None));
        dup.project_id = "other-project".to_string(); // different project_id, same file_name
        merge_mod_entries(&mut existing, vec![dup]);
        assert_eq!(existing.len(), 1, "duplicate file_name must be skipped");
    }

    /// `merge_mod_entries` is idempotent: calling twice with same input stays at len 1.
    #[test]
    fn merge_mod_entries_idempotent() {
        let entry = planned_to_mod_entry(&make_planned("sodium.jar", "https://example.com/a.jar", None, None));
        let mut existing: Vec<ModEntry> = Vec::new();
        merge_mod_entries(&mut existing, vec![entry.clone()]);
        merge_mod_entries(&mut existing, vec![entry.clone()]);
        assert_eq!(existing.len(), 1);
    }

    /// `attribute_outcomes` matches by URL (order-independent).
    ///
    /// `PlanResult::outcomes` is in a *different* order than `downloads`:
    /// - sodium (Ok) comes second in outcomes but first in downloads → ModEntry
    /// - fabric-api (Skipped) comes third in outcomes → ModEntry
    /// - broken-mod (Failed) comes first in outcomes → FailedMod
    #[test]
    fn attribute_outcomes_order_independent() {
        use crate::core::download::{ItemOutcome, ItemStatus, PlanResult};

        let sodium = make_planned("sodium.jar", "https://dl.example.com/sodium.jar", Some("abc"), None);
        let mut fabric_api = make_planned("fabric-api.jar", "https://dl.example.com/fa.jar", None, Some("sha1val"));
        fabric_api.project_id = "fabric-api".to_string();
        fabric_api.version_id = "fa-v1".to_string();
        let mut broken = make_planned("broken.jar", "https://dl.example.com/broken.jar", None, None);
        broken.project_id = "broken-mod".to_string();
        broken.version_id = "b-v1".to_string();

        // downloads: [sodium, fabric-api, broken]
        let downloads = vec![sodium.clone(), fabric_api.clone(), broken.clone()];

        // outcomes in DIFFERENT order: broken first, then sodium, then fabric-api
        let plan_result = PlanResult {
            outcomes: vec![
                ItemOutcome {
                    url: broken.url.clone(),
                    status: ItemStatus::Failed { error: "HTTP 404".to_string() },
                },
                ItemOutcome {
                    url: sodium.url.clone(),
                    status: ItemStatus::Ok,
                },
                ItemOutcome {
                    url: fabric_api.url.clone(),
                    status: ItemStatus::Skipped,
                },
            ],
        };

        let (added, failed) = attribute_outcomes(&downloads, &plan_result);

        // sodium (Ok) + fabric-api (Skipped) → both in added
        assert_eq!(added.len(), 2, "Ok and Skipped both produce ModEntry");
        assert!(added.iter().any(|e| e.project_id == "proj-a" && e.file_name == "sodium.jar"), "sodium in added");
        assert!(added.iter().any(|e| e.project_id == "fabric-api"), "fabric-api in added");

        // broken (Failed) → in failed
        assert_eq!(failed.len(), 1, "only broken-mod failed");
        assert_eq!(failed[0].file_name, "broken.jar");
        assert_eq!(failed[0].error, "HTTP 404");
    }

    /// `attribute_outcomes` treats a missing outcome as failed with a descriptive error.
    #[test]
    fn attribute_outcomes_missing_outcome_becomes_failed() {
        use crate::core::download::{ItemOutcome, ItemStatus, PlanResult};

        let sodium = make_planned("sodium.jar", "https://dl.example.com/sodium.jar", Some("abc"), None);
        // PlanResult has no outcome matching sodium's URL.
        let plan_result = PlanResult {
            outcomes: vec![ItemOutcome {
                url: "https://dl.example.com/other.jar".to_string(),
                status: ItemStatus::Ok,
            }],
        };

        let (added, failed) = attribute_outcomes(&[sodium.clone()], &plan_result);
        assert!(added.is_empty());
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].file_name, "sodium.jar");
        assert!(failed[0].error.contains("no outcome recorded"));
    }

    /// `AddModResult` has the expected fields (compile-time shape check).
    #[test]
    fn add_mod_result_default_is_empty() {
        let r = AddModResult::default();
        assert!(r.added.is_empty());
        assert!(r.manual.is_empty());
        assert!(r.unresolved.is_empty());
        assert!(r.suggestions.is_empty());
        assert!(r.warnings.is_empty());
        assert!(r.failed.is_empty());
    }
}
