# mod-install

## Overview

Split planner + executor for Modrinth and CurseForge mod installs. `resolve_install` performs a BFS dependency walk and returns a partitioned `InstallPlan`. `add_mod`/`update_mod` enqueue `ModAddJob`/`ModUpdateJob` and return a `u64` task id. Both jobs stage downloads into `<inst_dir>/.staging-<task_id>/mods/` then promote via atomic rename. `enrich_instance_mods` backfills `name/icon_url/summary` on existing `ModEntry`s via batched `get_projects_brief` — called at most once per instance per session via `enrichedSlugs` guard.

## CLI code

- `src-tauri/src/core/mod_install.rs` — pure planner: `resolve_install`, `InstallPlan`, `PlannedMod`, `ManualMod`, `UnresolvedDep`, `Suggestion`, `IncompatibleWarning`, `AddModResult`, `UpdateModResult`, `FailedMod`; executor helpers: `build_download_items`, `planned_to_mod_entry` (captures `name/icon_url/summary` from `ProjectSummary`), `merge_mod_entries`, `partition_by_file_name`, `attribute_outcomes`; update helpers: `decide_update`, `apply_swap`, `UpdateAction`; `fetch_newest_compatible`; `page_url_for`; 41 tests in `mod_install_tests.rs`
- `src-tauri/src/lib.rs` — `add_mod` (enqueues `ModAddJob`, returns `u64`); `update_mod` (enqueues `ModUpdateJob`, returns `u64`); `set_mod_enabled` / `remove_mod` synchronous; `enrich_instance_mods` async (not task-queued); `collect_missing_ids(mods) -> (Vec<String>, Vec<String>)` partitions by provider; `apply_briefs(mods, mr_briefs, cf_briefs)` patches entries in-place

## Artifacts

- `src/routes/InstanceDetail.tsx` — `ManageInstallsPanel` (Installed/Add two-tab panel; exported, used by `ModlistTab`); `ModRow`, `ModSearchCard`, `AddResultSummary`, `ManualEntry`; `enrichedSlugs` module-level Set gates backfill
- `src/routes/instance-tabs/ModlistTab.tsx` — thin wrapper: renders `ManageInstallsPanel`

## Docs

- `docs/spec/mod-install.md` — 5 checkpoints implementation contract
- `docs/spec/mod-metadata-ux.md` — metadata at add-time + `enrich_instance_mods` backfill spec
- `docs/spec/download-runner-rework/cp-4-mod-ops-fast-path.md` — `ModAddJob`/`ModUpdateJob` task wrappers, staging pattern

## Coupling

- `TaskManager` domain — `add_mod`/`update_mod` enqueue `TaskJob` objects; `execute_plan_cancellable` used in job bodies; `remap_to_staging`/`promote_staging` from `modpack.rs`.
- `providers` domain — `resolve_install` depends on `ModProvider` trait; `enrich_instance_mods` calls `provider.get_projects_brief`. FTB/ATL `get_projects_brief` are no-ops — those mods skip enrichment.
- `instances` domain — `set_mod_enabled` and `remove_mod` delegate to `instances::` equivalents; `load_manifest`/`save_manifest` called by every command.

## Conventions

- `attribute_outcomes` matches by URL, not by index.
- `merge_mod_entries` deduplicates by both `project_id` and `file_name`; new installs always `enabled = true`.
- Hash preference: sha512 > sha1 > None.
- Dep resolution stays within the originating provider (cross-provider is a non-goal).
- `PlannedMod.side` hardcoded `"unknown"` — per-file client/server side not surfaced by providers.
- `enrich_instance_mods` is idempotent: only patches entries where `name`, `icon_url`, and `summary` are all `None`. Makes zero network calls when nothing is missing.
- `ModEntry.name/icon_url/summary` are `Option<String>` with `#[serde(default)]`; old manifests load with `None`.
