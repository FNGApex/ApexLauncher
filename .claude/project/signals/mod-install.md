# mod-install

## What it does

Resolves and installs mods from Modrinth or CurseForge into an instance via a split planner + executor architecture. `resolve_install` performs a BFS dependency walk and returns a partitioned `InstallPlan`; the `add_mod` and `update_mod` Tauri commands enqueue `ModAddJob`/`ModUpdateJob` tasks and return a `u64` task id. Both jobs stage downloads into `<inst_dir>/.staging-<task_id>/mods/` then promote via atomic rename. Three additional Tauri commands manage installed mods synchronously. `enrich_instance_mods` (added ui-overhaul) backfills `name/icon_url/summary` on existing `ModEntry`s via batched `get_projects_brief` — runs at most once per instance per session via `enrichedSlugs` guard in `InstanceDetail.tsx`. Mod metadata (`name`, `icon_url`, `summary`) is now captured at add-time from `ProjectSummary` and stored in `ModEntry`.

## Artifacts

- `src/routes/InstanceDetail.tsx` — `ManageInstallsPanel` (two-tab Installed/Add panel, exported and used by `ModlistTab`); `ModRow`, `ModSearchCard`, `AddResultSummary`, `ManualEntry` sub-components; `enrichInstanceMods` called once per session per instance to backfill metadata; `enrichedSlugs` module-level Set gates the backfill call
- `src/routes/instance-tabs/ModlistTab.tsx` (21 lines) — thin wrapper: renders `ManageInstallsPanel` from `InstanceDetail.tsx` inside the Modlist route tab
- `src/components/ProviderBadge.tsx` — inline platform badge with color coding; used in Browse and in the Add tab

## CLI code

- `src-tauri/src/core/mod_install.rs` — pure planner: `resolve_install`, `InstallPlan`, `PlannedMod`, `ManualMod`, `UnresolvedDep`, `Suggestion`, `IncompatibleWarning`, `AddModResult`, `UpdateModResult`, `FailedMod`; executor helpers: `build_download_items`, `planned_to_mod_entry`, `merge_mod_entries`, `partition_by_file_name`, `attribute_outcomes`; update helpers: `decide_update`, `apply_swap`, `UpdateAction`; `fetch_newest_compatible`; `page_url_for`; `planned_to_mod_entry` now captures `name`, `icon_url`, `summary` from `PlannedMod` (sourced from `ProjectSummary` at add-time); 41 unit tests in sibling `mod_install_tests.rs`
- `src-tauri/src/lib.rs` — `add_mod` (async): checks `ensure_not_locked`, enqueues `ModAddJob`, returns `u64` task id; `update_mod` (async): same with `ModUpdateJob`; `set_mod_enabled` / `remove_mod` synchronous (instant off-queue); `enrich_instance_mods` (async): calls `collect_missing_ids` on instance mods, calls `get_projects_brief` per provider, calls `apply_briefs` to patch `ModEntry.name/icon_url/summary` fields, saves manifest; `ModAddJob` and `ModUpdateJob` are `TaskJob` implementors defined in `lib.rs`; `collect_missing_ids(mods: &[ModEntry]) -> (Vec<String>, Vec<String>)` partitions by provider; `apply_briefs(mods, mr_briefs, cf_briefs)` patches entries in-place

## Docs

- `docs/spec/mod-install.md` — implementation contract; 5 checkpoints; implementation log (shipped 2026-06-15)
- `docs/spec/mod-metadata-ux.md` — metadata at add-time + `enrich_instance_mods` backfill spec
- `docs/spec/download-runner-rework/cp-4-mod-ops-fast-path.md` — CP-4 spec: `ModAddJob`/`ModUpdateJob` task wrappers, staging pattern, task-id return contract
- `docs/design/mod-install.md` — problem statement, domain model with Mermaid flow, approach table

## Coupling

- **download-runner domain** — `add_mod`/`update_mod` commands enqueue `TaskJob` objects into `TaskManager`; `ModAddJob`/`ModUpdateJob` call `execute_plan_cancellable`; `remap_to_staging`/`promote_staging` from `modpack.rs` used for staged delivery.
- **providers domain** — `resolve_install` depends on `ModProvider` trait; `enrich_instance_mods` calls `provider.get_projects_brief`; `ModBrief.name/icon_url/summary` fields must match `ModEntry` fields or `apply_briefs` silently writes nothing.
- **instances domain** — `PlannedMod` → `ModEntry` conversion via `planned_to_mod_entry`; `set_mod_enabled` and `remove_mod` delegate entirely to `instances::set_mod_enabled` and `instances::remove_mod`; `load_manifest`/`save_manifest` called by every command.
- **frontend-shell / IPC** — `addMod`/`updateMod` return `Promise<number>` (task id); results arrive via `task://update` event. `enrichInstanceMods` returns result directly (not a task). `ManageInstallsPanel` is defined in `InstanceDetail.tsx` and imported by `ModlistTab` — it is not a standalone component file.

## Conventions worth knowing

- `resolve_install` takes `root_project_id` separately from `root_slug`; `ProjectVersion.id` is the version id.
- `attribute_outcomes` matches by URL, not by index.
- `PlannedMod.side` is hardcoded `"unknown"` — per-file client/server side not surfaced by providers yet.
- `merge_mod_entries` deduplicates by both `project_id` and `file_name`; new installs always set `enabled = true`.
- Hash preference order: sha512 > sha1 > None.
- Dep resolution stays within the originating provider (cross-provider is a non-goal).
- Staging dir: `<inst_dir>/.staging-<task_id>/mods/`; `remap_to_staging` and `promote_staging` defined in `core/modpack.rs` and shared across all five job types.
- `enrich_instance_mods` is idempotent: only patches entries where `name`, `icon_url`, and `summary` are all `None`; entries already with metadata are skipped. Makes zero network calls when nothing is missing.
- Metadata fields on `ModEntry` (`name`, `icon_url`, `summary`) are `Option<String>` with `#[serde(default)]`; old manifests load with `None` — no schema bump required.
