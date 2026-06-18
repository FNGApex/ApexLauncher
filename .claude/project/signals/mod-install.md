# mod-install

## What it does

Resolves and installs mods from Modrinth or CurseForge into an instance via a split planner + executor architecture: `resolve_install` performs a BFS dependency walk (required/optional/incompatible/embedded handling, cycle guard, dedup against already-installed manifest entries) and returns a partitioned `InstallPlan` (downloads, manual, unresolved, suggestions, warnings); the `add_mod` Tauri command enqueues a `ModAddJob` task and returns its `u64` task id; the `update_mod` Tauri command enqueues a `ModUpdateJob` task and returns its task id. Both jobs stage downloads into `<inst_dir>/.staging-<task_id>/mods/` then promote via atomic rename. Three additional Tauri commands manage installed mods: `set_mod_enabled` (renames file ±`.disabled` suffix, flips `ModEntry.enabled`, instant off-queue), `remove_mod` (deletes file + drops manifest entry, instant off-queue), and `update_mod` (enqueued — resolves newest compatible version, stages new file, swaps, preserves `enabled` state). All file names crossing the IPC boundary are traversal-validated by `validate_mod_file_name` (rejects `../`, `/`, `\`, absolute paths, drive-letter prefixes, non-`.jar` extensions) before any filesystem access. Mods are installed directly into `<instances>/<slug>/mc/mods/` — no shared cache or hardlink dedup.

## Artifacts

- `src/routes/InstanceDetail.tsx` — "Manage installs" slide-over entry point (opened via a "Manage installs" button in the mods section); two tabs: **Installed** (enable/disable via `setModEnabled`, remove via `removeMod`, update via `updateMod` → returns a task id; result shown in `AddResultSummary` once the `task://update` event fires with `Done`) and **Add mod** (CF/Modrinth source toggle, debounced search via `searchMods(..., "mod")`, version-resolve + `addMod` → task id, result shown in `AddResultSummary`); `ModRow`, `ModSearchCard`, `AddResultSummary`, `ManualEntry` sub-components; `ProviderBadge` used for source labeling in mod search cards
- `src/components/SlideOver.tsx` — reusable right-side panel with overlay, Escape-key close, and configurable `widthClass`; used by `InstanceDetail`'s Manage installs panel
- `src/components/ProviderBadge.tsx` — inline platform badge (`"Modrinth"` / `"CurseForge"`) with color coding; used in both Browse and the slide-over Add tab

## CLI code

- `src-tauri/src/core/mod_install.rs` (600 lines) — pure planner (`resolve_install`, `InstallPlan`, `PlannedMod`, `ManualMod`, `UnresolvedDep`, `Suggestion`, `IncompatibleWarning`), executor helpers (`build_download_items`, `planned_to_mod_entry`, `merge_mod_entries`, `partition_by_file_name`, `attribute_outcomes`), update helpers (`decide_update`, `apply_swap`, `UpdateAction`, `UpdateModResult`), result types (`AddModResult`, `FailedMod`), page-URL builder (`page_url_for`), internal version fetcher (`fetch_newest_compatible`); ends with a `#[path = "mod_install_tests.rs"] mod tests;` stub
- `src-tauri/src/core/mod_install_tests.rs` (1170 lines) — 40 unit tests using `MockProvider` (VecDeque-backed) and `MockProviderClient`, wired back via the `#[path]` stub
- `src-tauri/src/lib.rs` — `add_mod` (async Tauri command): checks `ensure_not_locked` synchronously then enqueues `ModAddJob` via `TaskManager`, returns `u64` task id; `update_mod` (async Tauri command): same pattern with `ModUpdateJob`; `set_mod_enabled` / `remove_mod` remain synchronous (instant off-queue); `ModAddJob` and `ModUpdateJob` are `TaskJob` implementors defined in `lib.rs`: they call `resolve_install`/`fetch_newest_compatible`, stage downloads via `execute_plan_cancellable`, promote via `promote_staging`, call `finish_done_with_result` with `AddModResult`/`UpdateModResult` JSON; registered in `tauri::generate_handler!`

## Docs

- `docs/spec/mod-install.md` — implementation contract; 5 checkpoints; success criteria; risks; implementation log (shipped 2026-06-15)
- `docs/spec/ui-modpack-rework.md` — CP5 spec: slide-over layout, Installed/Add tabs, source toggle
- `docs/spec/download-runner-rework/cp-4-mod-ops-fast-path.md` — CP-4 spec: `ModAddJob`/`ModUpdateJob` task wrappers, staging pattern, task-id return contract
- `docs/design/mod-install.md` — problem statement, goals/non-goals, domain model with Mermaid flow, approach table, open questions
- `docs/design/ui-modpack-rework.md` — design rationale for the slide-over approach
- `docs/design/download-runner-rework.md` — overall design: task queue, job trait, staging/promote pattern

## Coupling

- **download-runner domain** — `add_mod`/`update_mod` commands enqueue `TaskJob` objects into `TaskManager`; `ModAddJob`/`ModUpdateJob` call `execute_plan_cancellable` (cancel seam from download domain); `remap_to_staging`/`promote_staging` from `modpack.rs` are used for staged delivery. Task results (`AddModResult`/`UpdateModResult`) ride `task://update` events to the frontend store.
- **providers domain** — `resolve_install` depends on `ModProvider` trait and `ProviderHttpClient` from `providers.rs`; `fetch_newest_compatible` calls `provider.get_versions`; any change to `ProjectVersion`, `VersionFile`, or `Dependency` structs requires updating planner logic and test fixtures. `searchMods` in `InstanceDetail.tsx` passes `"mod"` as `ProjectType` — distinct from Browse's `"modpack"`.
- **download domain** — executor helpers produce `DownloadItem` / `DownloadPlan` and consume `PlanResult` / `ItemStatus` from `download.rs`; `execute_plan_cancellable` called from `ModAddJob`/`ModUpdateJob`
- **instances domain** — `PlannedMod` → `ModEntry` conversion via `planned_to_mod_entry`; `set_mod_enabled` and `remove_mod` delegate entirely to `instances::set_mod_enabled` and `instances::remove_mod`; `validate_mod_file_name` and `validate_slug` from `instances.rs` gate all IPC inputs; `load_manifest` / `save_manifest` called by every command
- **frontend-shell / IPC** — `ipc.ts` hand-mirrors all Rust output types (`AddModResult`, `UpdateModResult`, `ManualMod`, `UnresolvedDep`, `Suggestion`, `IncompatibleWarning`, `FailedMod`) with camelCase field names; `addMod`/`updateMod` now return `Promise<number>` (task id); results arrive via `task://update` event when `status.kind === "done"` and `task.result` is set; any struct field addition or rename requires a matching `ipc.ts` edit (no codegen)

## Conventions worth knowing

- `resolve_install` takes `root_project_id` separately from `root_slug`; `ProjectVersion.id` is the version id (different namespace from `Dependency.project_id` used by the dedup visited set)
- `attribute_outcomes` matches by URL, not by index — `execute_plan` returns outcomes in `FuturesUnordered` completion order with already-skipped items prepended
- `PlannedMod.side` is hardcoded `"unknown"` — per-file client/server side is not surfaced by providers yet (tracked follow-up `mod-install-f-1`)
- `decide_update` returns `Manual { page_url: String::new(), .. }` as a placeholder; `update_mod` in `lib.rs` rebuilds the real page URL via `page_url_for` at the call site
- `merge_mod_entries` deduplicates by both `project_id` and `file_name` (either match → skip); new installs always set `enabled = true`
- Enable/disable rename and manifest write are not atomic — disk truth is the authority; `scan_mods` self-heals on next load (tracked follow-up `mod-install-f-6`)
- Hash preference order: sha512 > sha1 > None (engine allows `expected_hash = None`; CF files often omit hashes)
- Dep resolution stays within the originating provider — cross-provider dep resolution is a non-goal
- `add_mod` passes the mod's `project_id` as both `root_project_id` and `root_slug`; dep page URLs fall back to id-based form
- `ModSearchCard` in `AddModTab` picks the first compatible version (`versionsQuery.data?.[0]`) without user selection; a version picker is not present
- Staging dir: `<inst_dir>/.staging-<task_id>/mods/`; `remap_to_staging` and `promote_staging` are defined in `core/modpack.rs` and shared across `ModAddJob`, `ModUpdateJob`, `ImportMrpackJob`, `ImportCfZipJob`, `UpdateModpackJob`
