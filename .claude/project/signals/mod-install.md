# mod-install

## What it does

Resolves and installs mods from Modrinth or CurseForge into an instance via a split planner + executor architecture: `resolve_install` performs a BFS dependency walk (required/optional/incompatible/embedded handling, cycle guard, dedup against already-installed manifest entries) and returns a partitioned `InstallPlan` (downloads, manual, unresolved, suggestions, warnings); the `add_mod` Tauri command executes that plan against the download engine and merges resulting `ModEntry` records into `instance.json`. Three additional Tauri commands manage installed mods: `set_mod_enabled` (renames file ±`.disabled` suffix, flips `ModEntry.enabled`), `remove_mod` (deletes file + drops manifest entry), and `update_mod` (resolves newest compatible version, swaps file + updates entry, preserves `enabled` state). All file names crossing the IPC boundary are traversal-validated by `validate_mod_file_name` (rejects `../`, `/`, `\`, absolute paths, drive-letter prefixes, non-`.jar` extensions) before any filesystem access. Mods are installed directly into `<instances>/<slug>/mc/mods/` — no shared cache or hardlink dedup.

## Artifacts

- `src/routes/Browse.tsx` — `AddToInstanceModal` component: instance selector, version picker (first compatible version from `getModVersions`), calls `addMod`, shows `AddResultSummary`; entry point for mod install flow
- `src/routes/InstanceDetail.tsx` — per-mod enable/disable (`setModEnabled`), remove (`removeMod`), and update (`updateMod`) controls; displays `UpdateModResult` status

## CLI code

- `src-tauri/src/core/mod_install.rs` — pure planner (`resolve_install`, `InstallPlan`, `PlannedMod`, `ManualMod`, `UnresolvedDep`, `Suggestion`, `IncompatibleWarning`), executor helpers (`build_download_items`, `planned_to_mod_entry`, `merge_mod_entries`, `partition_by_file_name`, `attribute_outcomes`), update helpers (`decide_update`, `apply_swap`, `UpdateAction`, `UpdateModResult`), result types (`AddModResult`, `FailedMod`), page-URL builder (`page_url_for`), internal version fetcher (`fetch_newest_compatible`); 40 unit tests using `MockProvider` (VecDeque-backed) and `MockProviderClient`
- `src-tauri/src/lib.rs` — Tauri command implementations: `add_mod` (async), `set_mod_enabled`, `remove_mod`, `update_mod` (async); registered in the `tauri::generate_handler!` invocation; slug validated via `validate_slug` at the join site inside `add_mod` and `update_mod`; `set_mod_enabled` and `remove_mod` delegate validation into their `instances` counterparts

## Docs

- `docs/spec/mod-install.md` — implementation contract; 5 checkpoints; success criteria; risks; implementation log (shipped 2026-06-15, 369 Rust lib tests pass)
- `docs/design/mod-install.md` — problem statement, goals/non-goals, domain model with Mermaid flow, approach table (A/B/C), open questions (CF id-only page URL, newest-compatible tie-break)

## Coupling

- **providers domain** — `resolve_install` depends on `ModProvider` trait and `ProviderHttpClient` from `providers.rs`; `fetch_newest_compatible` calls `provider.get_versions`; any change to `ProjectVersion`, `VersionFile`, or `Dependency` structs requires updating planner logic and test fixtures
- **download domain** — executor helpers produce `DownloadItem` / `DownloadPlan` and consume `PlanResult` / `ItemStatus` from `download.rs`; `execute_plan` and `build_client` are called directly from `add_mod` and `update_mod` commands in `lib.rs`
- **instances domain** — `PlannedMod` → `ModEntry` conversion via `planned_to_mod_entry`; `set_mod_enabled` and `remove_mod` delegate entirely to `instances::set_mod_enabled` and `instances::remove_mod`; `validate_mod_file_name` and `validate_slug` from `instances.rs` gate all IPC inputs; `load_manifest` / `save_manifest` called by every command
- **frontend-shell / IPC** — `ipc.ts` hand-mirrors all Rust output types (`AddModResult`, `UpdateModResult`, `ManualMod`, `UnresolvedDep`, `Suggestion`, `IncompatibleWarning`, `FailedMod`) with camelCase field names via `serde rename_all`; any struct field addition or rename requires a matching `ipc.ts` edit (no codegen)

## Conventions worth knowing

- `resolve_install` takes `root_project_id` separately from `root_slug`; `ProjectVersion.id` is the version id (different namespace from `Dependency.project_id` used by the dedup visited set)
- `attribute_outcomes` matches by URL, not by index — `execute_plan` returns outcomes in `FuturesUnordered` completion order with already-skipped items prepended; index-based matching misattributes outcomes
- `PlannedMod.side` is hardcoded `"unknown"` — per-file client/server side is not surfaced by providers yet (tracked follow-up `mod-install-f-1`)
- `decide_update` returns `Manual { page_url: String::new(), .. }` as a placeholder; `update_mod` in `lib.rs` rebuilds the real page URL via `page_url_for` at the call site
- `merge_mod_entries` deduplicates by both `project_id` and `file_name` (either match → skip); new installs always set `enabled = true`
- Enable/disable rename and manifest write are not atomic — disk truth is the authority; `scan_mods` self-heals on next load (tracked follow-up `mod-install-f-6`)
- Hash preference order: sha512 > sha1 > None (engine allows `expected_hash = None`; CF files often omit hashes)
- Dep resolution stays within the originating provider — cross-provider dep resolution is a non-goal
- `add_mod` passes the mod's `project_id` as both `root_project_id` and `root_slug` (slug not separately threaded from Browse call site; dep page URLs fall back to id-based form)
