# modpack

## Overview

Pure parse/plan module (`core/modpack.rs`) with no Tauri commands and no filesystem writes (except `extract_overrides` and `extract_atl_configs`). All executing commands enqueue a `TaskJob` and return a `u64` task id; the terminal result rides `task://update` via `finish_done_with_result`. `set_pack_lock` is synchronous.

`remap_to_staging` + `promote_staging` defined here and shared by all 8 job types. `refresh_pack_meta` command checks for updates once per 24h.

**CF manual-download pipeline:** `CfManualFile { project_id, file_id, file_name, page_url, expected_sha1, size }`; `impl From<&CfManualFile> for PendingManual`; `cf_file_page_url(slug, project_id, file_id)` builds file-page URL using slug when available.

**FTB planner:** `FtbPackPlan`; `build_ftb_pack_plan` (pure); `resolve_and_build_ftb_plan` (async; calls `CurseForgeProvider::get_file` per CF-referenced file); `ftb_dest_path`. FTB install sets `Source.recommended` from `specs.recommended`. FTB update-apply deferred to v2.

**ATL planner:** `AtlPackPlan { downloads, manual, overrides, configs_url }`; `build_atl_pack_plan` (pure; type→folder map; `Ok(None)` skips unsupported types); `atl_dest_path`; `extract_atl_configs` (downloads Configs.zip, extracts root-relative paths, zip-slip-guarded). ATL install sets `Source.recommended` from `manifest.memory > 0`. ATL update-apply deferred to v2.

## CLI code

- `src-tauri/src/core/modpack.rs` — slice A: `MrpackManifest`/`MrpackFile`/`FileEnv`/`PackLoader`, `parse_modrinth_index`, `PackPlan`, `build_pack_plan` (host allowlist, hash pick sha512>sha1, env filter, path-safety), `extract_overrides`, `read_mrpack`; slice B: `CfManifest`/`CfManifestFile`/`CfManualFile`/`CfResolveFailure`, `parse_cf_manifest`, `CfPackPlan`, `build_cf_pack_plan`, `resolve_and_build_cf_plan`; `cf_file_page_url`; `impl From<&CfManualFile> for PendingManual`; FTB: `FtbPackPlan`, `build_ftb_pack_plan`, `resolve_and_build_ftb_plan`, `ftb_dest_path`; ATL: `AtlPackPlan`, `build_atl_pack_plan`, `atl_dest_path`, `extract_atl_configs`; slice C: `ResolvedPackFile`, `resolve_pack_file`; slice D: `PackUpdatePlan`, `plan_pack_update`; shared: `ModpackError`, `validate_relative_path`, `remap_to_staging`, `promote_staging`; 113 tests in `modpack_tests.rs`
- `src-tauri/src/core/curseforge.rs` — `CurseForgeProvider::get_file`; `CurseForgeProvider::get_mod_slug`
- `src-tauri/src/lib.rs` — `ImportMrpackJob`, `ImportCfZipJob`, `ImportFtbJob`, `ImportAtlJob`, `UpdateModpackJob` (all `TaskJob` impls); `import_mrpack`, `import_curseforge_zip`, `update_modpack`, `install_modpack` (all four providers) Tauri commands — enqueue jobs, return `u64`; `import_mrpack`/`import_curseforge_zip`/FTB/ATL imports populate `instance.pending_manual` from `CfManualFile` entries; `ImportFtbJob` holds pre-fetched `FtbVersionManifest`; `ImportAtlJob` resolves version + configs; `update_modpack` for "ftb"/"atlauncher" returns error immediately (no task enqueued); `set_pack_lock` synchronous; `refresh_pack_meta` throttled 24h; `staging_dir_for(inst_dir, task_id)` → `<inst_dir>/.staging-<task_id>/`

## Artifacts

- `src/components/NewInstanceModal.tsx` — Import tab: file picker, routes by extension
- `src/routes/Browse.tsx` — Install button calls `installModpack` (returns task id); result toast from store once Done
- `src/routes/BrowsePackInfo.tsx` — `getPackInfo` (lazy), `getModVersions` (lazy on version-modal), `installModpack` on confirm
- `src/routes/instance-tabs/InfoTab.tsx` — `PackSourcePanel`; update-available banner; Update button → `updateModpack`; Lock/Unlock → `setPackLock`

## Docs

- `docs/spec/modpack-import.md` — all four slices
- `docs/spec/cf-manual-download-ux.md` — CfManualFile, pending_manual pipeline
- `docs/spec/ftb-integration.md`, `docs/spec/atlauncher-integration.md`
- `docs/spec/download-runner-rework/cp-3-pack-ops-stage-promote.md` — staging dir pattern

## Coupling

- `task-manager` domain — all five pack job types enqueue into `TaskManager`; `execute_plan_cancellable` is the download step; `remap_to_staging`/`promote_staging` also shared with `ModAddJob`/`ModUpdateJob` in `lib.rs`.
- `instances` domain — job impls call `instances::create`/`load_manifest`/`save_manifest`; `ModEntry.from_pack: bool` is in `instances.rs`.
- `providers` domain — `resolve_and_build_cf_plan` calls `CurseForgeProvider::get_file`; `resolve_pack_file` calls provider APIs.
- `mod-install` domain — `remap_to_staging`/`promote_staging` defined here, imported by `ModAddJob`/`ModUpdateJob` in `lib.rs`.

## Conventions

- `staging_dir_for(inst_dir, task_id)` → `<inst_dir>/.staging-<task_id>/`; staging is sibling of instance dir so `rename()` is same-filesystem.
- `remap_to_staging` rewrites only items whose `dest` falls under `mc_dir`; others pass through unchanged.
- `promote_staging` is a recursive rename-walk; cancel/fail → caller calls `remove_dir_all` on staging dir.
- `plan_pack_update` diff: user-added mods (`from_pack=false`) survive unless filename overridden by a new pack entry.
- mrpack host allowlist (`ALLOWED_HOSTS`): `cdn.modrinth.com`, `github.com`, `raw.githubusercontent.com`, `gitlab.com`. URL outside this list aborts the whole import via `ModpackError::DisallowedHost`.
- `validate_relative_path` rejects absolute paths, `\`-prefixed, Windows drive letters, any `..` component.
- CF hash preference in `build_cf_pack_plan`: only `sha1` checked — file with only md5 routes to manual.
- FTB file routing: `FtbFile.url` non-empty → FTB-CDN download (sha1 verified); `FtbFile.curseforge` set + `url` empty → CF-referenced (resolved via `get_file`); resolved CF with sha1 → downloads, without sha1 → manual.
- ATL type→folder: `mods`→`mods/`, `resourcepack`→`resourcepacks/`, `shaderpack`→`shaderpacks/`, `texturepack`→`resourcepacks/`, `datapack`→`datapacks/`, `jarmod`→`jarmods/`, or `path_override`; unrecognized type → `Ok(None)` (skipped, not an error).
- `Source.recommended` (`Option<u64>` in MB): populated from `FtbVersionManifest.specs.recommended` (FTB) or `manifest.memory > 0` (ATL); activates tier-1 Java/RAM precedence.
- `src-tauri/tests/curseforge_live.rs` — `#[ignore]`d integration test hitting real CF API.
