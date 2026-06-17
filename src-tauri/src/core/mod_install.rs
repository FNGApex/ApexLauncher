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

// ── CP4: Update types and pure helpers ────────────────────────────────────────

/// Decision returned by `decide_update`.
#[derive(Debug, PartialEq)]
pub enum UpdateAction {
    /// The installed version is already the newest compatible version.
    UpToDate,
    /// A newer version is available and can be downloaded automatically.
    Swap {
        /// The new file to download (carries url, file_name, hashes).
        new_file: crate::core::providers::VersionFile,
        /// The new version's id (stored in `ModEntry.version_id`).
        new_version_id: String,
    },
    /// The newest version has `url == None` (distribution disabled); user must
    /// download manually from the provider page.
    Manual {
        /// Project page URL the user should open.
        page_url: String,
        /// File name the user is expected to drop in.
        file_name: String,
    },
    /// No compatible version found for this instance's MC version + loader.
    Unresolved,
}

/// Pure decision function: given the current manifest entry and the newest
/// compatible `ProjectVersion` (if any), decide what `update_mod` should do.
///
/// `newest` is `None` when `fetch_newest_compatible` returns `None`.
pub fn decide_update(
    current: &crate::core::instances::ModEntry,
    newest: Option<&crate::core::providers::ProjectVersion>,
) -> UpdateAction {
    let Some(version) = newest else {
        return UpdateAction::Unresolved;
    };

    if version.id == current.version_id {
        return UpdateAction::UpToDate;
    }

    // Find the primary file (or first file) for the new version.
    let file = version
        .files
        .iter()
        .find(|f| f.primary)
        .or_else(|| version.files.first());

    match file {
        None => UpdateAction::Unresolved,
        Some(f) if f.url.is_none() => UpdateAction::Manual {
            page_url: String::new(), // caller fills this in via page_url_for
            file_name: f.file_name.clone(),
        },
        Some(f) => UpdateAction::Swap {
            new_file: f.clone(),
            new_version_id: version.id.clone(),
        },
    }
}

/// Apply a `Swap` result to `entry` in place.
///
/// Updates `version_id`, `file_name`, and `hashes` from `new_file` and
/// `new_version_id`. Preserves `enabled`, `provider`, `project_id`, `side`.
///
/// `enabled` preservation is critical: if the mod was disabled before the
/// update, the caller must ensure the new file is placed with the `.disabled`
/// suffix so disk state matches the flag.
pub fn apply_swap(
    entry: &mut crate::core::instances::ModEntry,
    new_file: &crate::core::providers::VersionFile,
    new_version_id: &str,
) {
    entry.version_id = new_version_id.to_string();
    entry.file_name = new_file.file_name.clone();
    let mut hashes = std::collections::BTreeMap::new();
    for (k, v) in &new_file.hashes {
        hashes.insert(k.clone(), v.clone());
    }
    entry.hashes = hashes;
    // entry.enabled is intentionally NOT changed.
}

/// Structured result returned by the `update_mod` Tauri command.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateModResult {
    /// One of `"updated"` | `"upToDate"` | `"manual"` | `"unresolved"` | `"failed"`.
    pub status: String,
    /// Updated `ModEntry` on `"updated"`; `None` otherwise.
    pub entry: Option<crate::core::instances::ModEntry>,
    /// Project page URL on `"manual"` (open in browser).
    pub page_url: Option<String>,
    /// Human-readable error description on `"failed"`.
    pub error: Option<String>,
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Fetch the newest compatible version of `project_id`.
///
/// If `preferred_version_id` is `Some`, try to return that specific version first;
/// fall back to first returned version if not found (providers may not support
/// single-version lookup in `get_versions`).
pub(crate) async fn fetch_newest_compatible(
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
        from_pack: false,
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

/// Partition `planned` downloads into (valid, invalid) by validating each entry's
/// `file_name` via `instances::validate_mod_file_name`.
///
/// Valid entries are returned in the first vector (preserving order) and passed to
/// `build_download_items`. Invalid entries become `FailedMod`s in the second vector
/// so `add_mod` can route them to `AddModResult.failed` without touching the FS.
pub fn partition_by_file_name(
    planned: Vec<PlannedMod>,
) -> (Vec<PlannedMod>, Vec<FailedMod>) {
    use crate::core::instances::validate_mod_file_name;
    let mut valid: Vec<PlannedMod> = Vec::new();
    let mut invalid: Vec<FailedMod> = Vec::new();
    for p in planned {
        match validate_mod_file_name(&p.file_name) {
            Ok(_) => valid.push(p),
            Err(e) => invalid.push(FailedMod {
                file_name: p.file_name,
                error: format!("invalid provider file name: {e}"),
            }),
        }
    }
    (valid, invalid)
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
#[path = "mod_install_tests.rs"]
mod tests;
