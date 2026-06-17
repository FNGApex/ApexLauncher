//! Unit tests for `mod_install`. Extracted from the source module; wired back
//! via `#[cfg(test)] #[path = "mod_install_tests.rs"] mod tests;` so private items
//! remain accessible through `super::*`.

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
    let root = make_version(
        "sodium",
        "v1",
        vec![make_file(Some("https://dl.example.com/sodium.jar"), true)],
        vec![],
    );
    let provider = MockProvider::new(vec![]);
    let client = MockProviderClient::new(vec![]);
    let plan = resolve_install(
        &provider,
        &client,
        root,
        "sodium",
        "sodium",
        "1.21",
        "fabric",
        &HashSet::new(),
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
        vec![make_file(
            Some("https://dl.example.com/fabric-api.jar"),
            true,
        )],
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
        &provider,
        &client,
        root,
        "sodium",
        "sodium",
        "1.21",
        "fabric",
        &HashSet::new(),
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
        &provider,
        &client,
        root,
        "root-mod",
        "root-mod",
        "1.21",
        "fabric",
        &HashSet::new(),
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
        &provider,
        &client,
        root,
        "dep-a",
        "dep-a",
        "1.21",
        "fabric",
        &HashSet::new(),
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
        &provider,
        &client,
        root,
        "cf-mod",
        "cf-mod",
        "1.21",
        "fabric",
        &HashSet::new(),
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
        &provider,
        &client,
        root,
        "sodium",
        "sodium",
        "1.21",
        "fabric",
        &HashSet::new(),
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
        &provider,
        &client,
        root,
        "sodium",
        "sodium",
        "1.21",
        "fabric",
        &HashSet::new(),
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
        &provider,
        &client,
        root,
        "sodium",
        "sodium",
        "1.21",
        "fabric",
        &HashSet::new(),
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
        &provider,
        &client,
        root,
        "lithium",
        "lithium",
        "1.21",
        "fabric",
        &HashSet::new(),
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
        &provider,
        &client,
        root,
        "sodium",
        "sodium",
        "1.21",
        "fabric",
        &HashSet::new(),
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
        &provider,
        &client,
        root,
        "sodium",
        "sodium",
        "1.21",
        "fabric",
        &HashSet::new(),
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
        &provider,
        &client,
        root,
        "sodium",
        "sodium",
        "1.21",
        "fabric",
        &HashSet::new(),
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
        &provider,
        &client,
        root,
        "root",
        "root",
        "1.21",
        "fabric",
        &HashSet::new(),
    )
    .await;

    // root + dep-b + shared-dep = 3, not 4
    assert_eq!(plan.downloads.len(), 3);
}

// ── CP2: pure helper tests ────────────────────────────────────────────────

use super::{
    attribute_outcomes, build_download_items, merge_mod_entries, planned_to_mod_entry, AddModResult,
};
use crate::core::download::ExpectedHash;
use crate::core::instances::ModEntry;
use std::path::Path;

fn make_planned(
    file_name: &str,
    url: &str,
    sha512: Option<&str>,
    sha1: Option<&str>,
) -> super::PlannedMod {
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
        downloads: vec![make_planned(
            "sodium.jar",
            "https://dl.example.com/sodium.jar",
            Some("abc"),
            None,
        )],
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
        downloads: vec![make_planned(
            "mod.jar",
            "https://dl.example.com/mod.jar",
            Some("sha512hex"),
            Some("sha1hex"),
        )],
        ..Default::default()
    };
    let items = build_download_items(&plan, Path::new("/mods"));
    assert_eq!(
        items[0].expected_hash,
        Some(ExpectedHash::Sha512("sha512hex".to_string()))
    );
}

/// sha1 used when sha512 absent.
#[test]
fn build_download_items_falls_back_to_sha1() {
    let plan = InstallPlan {
        downloads: vec![make_planned(
            "mod.jar",
            "https://dl.example.com/mod.jar",
            None,
            Some("sha1hex"),
        )],
        ..Default::default()
    };
    let items = build_download_items(&plan, Path::new("/mods"));
    assert_eq!(
        items[0].expected_hash,
        Some(ExpectedHash::Sha1("sha1hex".to_string()))
    );
}

/// No hash present → `expected_hash = None`.
#[test]
fn build_download_items_no_hash_gives_none() {
    let plan = InstallPlan {
        downloads: vec![make_planned(
            "mod.jar",
            "https://dl.example.com/mod.jar",
            None,
            None,
        )],
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
    let planned = make_planned(
        "sodium.jar",
        "https://dl.example.com/sodium.jar",
        Some("deadbeef"),
        None,
    );
    let entry = planned_to_mod_entry(&planned);
    assert_eq!(entry.provider, "modrinth");
    assert_eq!(entry.project_id, "proj-a");
    assert_eq!(entry.version_id, "v1");
    assert_eq!(entry.file_name, "sodium.jar");
    assert!(entry.enabled);
    assert_eq!(entry.side, "unknown");
    assert_eq!(entry.hashes.get("sha512"), Some(&"deadbeef".to_string()));
    // D1: user-added mods must always have from_pack=false.
    assert!(
        !entry.from_pack,
        "planned_to_mod_entry must set from_pack=false for user-added mods"
    );
}

/// `merge_mod_entries` appends new entries that don't clash.
#[test]
fn merge_mod_entries_appends_new() {
    let mut existing: Vec<ModEntry> = Vec::new();
    let entry = planned_to_mod_entry(&make_planned(
        "sodium.jar",
        "https://example.com/a.jar",
        None,
        None,
    ));
    merge_mod_entries(&mut existing, vec![entry.clone()]);
    assert_eq!(existing.len(), 1);
    assert_eq!(existing[0].project_id, "proj-a");
}

/// `merge_mod_entries` skips entry with duplicate `project_id`.
#[test]
fn merge_mod_entries_dedup_by_project_id() {
    let entry = planned_to_mod_entry(&make_planned(
        "sodium.jar",
        "https://example.com/a.jar",
        None,
        None,
    ));
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
    let entry = planned_to_mod_entry(&make_planned(
        "sodium.jar",
        "https://example.com/a.jar",
        None,
        None,
    ));
    let mut existing = vec![entry];
    let mut dup = planned_to_mod_entry(&make_planned(
        "sodium.jar",
        "https://example.com/b.jar",
        None,
        None,
    ));
    dup.project_id = "other-project".to_string(); // different project_id, same file_name
    merge_mod_entries(&mut existing, vec![dup]);
    assert_eq!(existing.len(), 1, "duplicate file_name must be skipped");
}

/// `merge_mod_entries` is idempotent: calling twice with same input stays at len 1.
#[test]
fn merge_mod_entries_idempotent() {
    let entry = planned_to_mod_entry(&make_planned(
        "sodium.jar",
        "https://example.com/a.jar",
        None,
        None,
    ));
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

    let sodium = make_planned(
        "sodium.jar",
        "https://dl.example.com/sodium.jar",
        Some("abc"),
        None,
    );
    let mut fabric_api = make_planned(
        "fabric-api.jar",
        "https://dl.example.com/fa.jar",
        None,
        Some("sha1val"),
    );
    fabric_api.project_id = "fabric-api".to_string();
    fabric_api.version_id = "fa-v1".to_string();
    let mut broken = make_planned(
        "broken.jar",
        "https://dl.example.com/broken.jar",
        None,
        None,
    );
    broken.project_id = "broken-mod".to_string();
    broken.version_id = "b-v1".to_string();

    // downloads: [sodium, fabric-api, broken]
    let downloads = vec![sodium.clone(), fabric_api.clone(), broken.clone()];

    // outcomes in DIFFERENT order: broken first, then sodium, then fabric-api
    let plan_result = PlanResult {
        outcomes: vec![
            ItemOutcome {
                url: broken.url.clone(),
                status: ItemStatus::Failed {
                    error: "HTTP 404".to_string(),
                },
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
    assert!(
        added
            .iter()
            .any(|e| e.project_id == "proj-a" && e.file_name == "sodium.jar"),
        "sodium in added"
    );
    assert!(
        added.iter().any(|e| e.project_id == "fabric-api"),
        "fabric-api in added"
    );

    // broken (Failed) → in failed
    assert_eq!(failed.len(), 1, "only broken-mod failed");
    assert_eq!(failed[0].file_name, "broken.jar");
    assert_eq!(failed[0].error, "HTTP 404");
}

/// `attribute_outcomes` treats a missing outcome as failed with a descriptive error.
#[test]
fn attribute_outcomes_missing_outcome_becomes_failed() {
    use crate::core::download::{ItemOutcome, ItemStatus, PlanResult};

    let sodium = make_planned(
        "sodium.jar",
        "https://dl.example.com/sodium.jar",
        Some("abc"),
        None,
    );
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

// ── CP4b: traversal validation for provider-supplied file names ───────────

use super::partition_by_file_name;

/// `partition_by_file_name`: `"../evil.jar"` is rejected into the invalid bucket.
#[test]
fn partition_traversal_path_rejected() {
    let evil = make_planned("../evil.jar", "https://dl.example.com/evil.jar", None, None);
    let (valid, invalid) = partition_by_file_name(vec![evil]);
    assert!(valid.is_empty(), "traversal path must not be valid");
    assert_eq!(invalid.len(), 1);
    assert_eq!(invalid[0].file_name, "../evil.jar");
    assert!(invalid[0].error.contains("invalid provider file name"));
}

/// `partition_by_file_name`: absolute path is rejected.
#[test]
fn partition_absolute_path_rejected() {
    let abs = make_planned(
        "/etc/passwd.jar",
        "https://dl.example.com/x.jar",
        None,
        None,
    );
    let (valid, invalid) = partition_by_file_name(vec![abs]);
    assert!(valid.is_empty(), "absolute path must not be valid");
    assert_eq!(invalid.len(), 1);
}

/// `partition_by_file_name`: non-`.jar` extension is rejected.
#[test]
fn partition_non_jar_extension_rejected() {
    let evil = make_planned("evil.sh", "https://dl.example.com/evil.sh", None, None);
    let (valid, invalid) = partition_by_file_name(vec![evil]);
    assert!(valid.is_empty(), "non-jar extension must not be valid");
    assert_eq!(invalid.len(), 1);
    assert!(invalid[0].error.contains("invalid provider file name"));
}

/// `partition_by_file_name`: valid name passes through unchanged.
#[test]
fn partition_valid_name_passes() {
    let good = make_planned(
        "sodium-v2.jar",
        "https://dl.example.com/sodium-v2.jar",
        Some("abc"),
        None,
    );
    let (valid, invalid) = partition_by_file_name(vec![good]);
    assert_eq!(valid.len(), 1);
    assert!(invalid.is_empty());
    assert_eq!(valid[0].file_name, "sodium-v2.jar");
}

/// `partition_by_file_name`: mix of valid and invalid → each routed correctly.
#[test]
fn partition_mixed_routes_correctly() {
    let good = make_planned(
        "sodium.jar",
        "https://dl.example.com/sodium.jar",
        None,
        None,
    );
    let mut bad = make_planned("../evil.jar", "https://dl.example.com/evil.jar", None, None);
    bad.project_id = "evil-proj".to_string();
    let (valid, invalid) = partition_by_file_name(vec![good, bad]);
    assert_eq!(valid.len(), 1);
    assert_eq!(invalid.len(), 1);
    assert_eq!(valid[0].file_name, "sodium.jar");
    assert_eq!(invalid[0].file_name, "../evil.jar");
}

/// `partition_by_file_name`: backslash separator is rejected.
#[test]
fn partition_backslash_separator_rejected() {
    let evil = make_planned(
        "sub\\evil.jar",
        "https://dl.example.com/evil.jar",
        None,
        None,
    );
    let (valid, invalid) = partition_by_file_name(vec![evil]);
    assert!(valid.is_empty(), "backslash separator must not be valid");
    assert_eq!(invalid.len(), 1);
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

// ── CP4: decide_update + apply_swap tests ─────────────────────────────────

use super::{apply_swap, decide_update, UpdateAction};
use std::collections::BTreeMap;

fn stub_mod_entry(version_id: &str, file_name: &str, enabled: bool) -> ModEntry {
    let mut hashes = BTreeMap::new();
    hashes.insert("sha512".to_string(), "oldhash".to_string());
    ModEntry {
        provider: "modrinth".to_string(),
        project_id: "proj-x".to_string(),
        version_id: version_id.to_string(),
        file_name: file_name.to_string(),
        hashes,
        enabled,
        side: "unknown".to_string(),
        from_pack: false,
    }
}

fn stub_version_file(url: Option<&str>, file_name: &str) -> VersionFile {
    let mut hashes = HashMap::new();
    hashes.insert("sha512".to_string(), "newhash".to_string());
    VersionFile {
        url: url.map(|s| s.to_string()),
        file_name: file_name.to_string(),
        size: None,
        hashes,
        primary: true,
    }
}

fn stub_project_version(version_id: &str, files: Vec<VersionFile>) -> ProjectVersion {
    ProjectVersion {
        provider: ProviderKind::Modrinth,
        id: version_id.to_string(),
        name: format!("Version {version_id}"),
        version_number: "2.0.0".to_string(),
        game_versions: vec!["1.21".to_string()],
        loaders: vec!["fabric".to_string()],
        files,
        dependencies: vec![],
    }
}

/// `decide_update`: no compatible version → `Unresolved`.
#[test]
fn decide_update_none_gives_unresolved() {
    let entry = stub_mod_entry("v1", "mod.jar", true);
    assert_eq!(decide_update(&entry, None), UpdateAction::Unresolved);
}

/// `decide_update`: newest version id == current → `UpToDate`.
#[test]
fn decide_update_same_version_id_gives_up_to_date() {
    let entry = stub_mod_entry("v1", "mod.jar", true);
    let newest = stub_project_version(
        "v1",
        vec![stub_version_file(
            Some("https://dl.example.com/mod.jar"),
            "mod.jar",
        )],
    );
    assert_eq!(decide_update(&entry, Some(&newest)), UpdateAction::UpToDate);
}

/// `decide_update`: newer version with a URL → `Swap` with the new file.
#[test]
fn decide_update_newer_version_gives_swap() {
    let entry = stub_mod_entry("v1", "mod-v1.jar", true);
    let newest = stub_project_version(
        "v2",
        vec![stub_version_file(
            Some("https://dl.example.com/mod-v2.jar"),
            "mod-v2.jar",
        )],
    );
    match decide_update(&entry, Some(&newest)) {
        UpdateAction::Swap {
            new_file,
            new_version_id,
        } => {
            assert_eq!(new_version_id, "v2");
            assert_eq!(new_file.file_name, "mod-v2.jar");
            assert_eq!(
                new_file.url,
                Some("https://dl.example.com/mod-v2.jar".to_string())
            );
        }
        other => panic!("expected Swap, got {other:?}"),
    }
}

/// `decide_update`: newer version but `url == None` → `Manual`.
#[test]
fn decide_update_url_none_gives_manual() {
    let entry = stub_mod_entry("v1", "mod.jar", true);
    let newest = stub_project_version("v2", vec![stub_version_file(None, "mod-v2.jar")]);
    match decide_update(&entry, Some(&newest)) {
        UpdateAction::Manual { file_name, .. } => {
            assert_eq!(file_name, "mod-v2.jar");
        }
        other => panic!("expected Manual, got {other:?}"),
    }
}

/// `decide_update`: version with no files at all → `Unresolved`.
#[test]
fn decide_update_no_files_gives_unresolved() {
    let entry = stub_mod_entry("v1", "mod.jar", true);
    let newest = stub_project_version("v2", vec![]);
    assert_eq!(
        decide_update(&entry, Some(&newest)),
        UpdateAction::Unresolved
    );
}

/// `apply_swap`: updates `version_id`, `file_name`, `hashes`; preserves `enabled`.
#[test]
fn apply_swap_updates_fields_preserves_enabled() {
    let mut entry = stub_mod_entry("v1", "mod-v1.jar", false); // disabled
    let new_file = stub_version_file(Some("https://dl.example.com/mod-v2.jar"), "mod-v2.jar");
    apply_swap(&mut entry, &new_file, "v2");

    assert_eq!(entry.version_id, "v2");
    assert_eq!(entry.file_name, "mod-v2.jar");
    assert_eq!(entry.hashes.get("sha512"), Some(&"newhash".to_string()));
    // enabled must NOT have changed
    assert!(
        !entry.enabled,
        "enabled state must be preserved (was false)"
    );
    // provider/project_id/side unchanged
    assert_eq!(entry.provider, "modrinth");
    assert_eq!(entry.project_id, "proj-x");
    assert_eq!(entry.side, "unknown");
}

/// `apply_swap`: `enabled = true` is also preserved.
#[test]
fn apply_swap_preserves_enabled_true() {
    let mut entry = stub_mod_entry("v1", "mod-v1.jar", true);
    let new_file = stub_version_file(Some("https://dl.example.com/mod-v2.jar"), "mod-v2.jar");
    apply_swap(&mut entry, &new_file, "v2");
    assert!(entry.enabled, "enabled state must be preserved (was true)");
}
