# modpack

## What it does

Imports a local modpack archive (Modrinth `.mrpack` slice A, CurseForge `.zip` slice B), installs a pack from Browse one-click (slice C), or updates/locks an installed pack (slice D). `core/modpack.rs` is a pure parse/plan module (no Tauri commands, no filesystem writes except `extract_overrides`). All executing commands (`import_mrpack`, `import_curseforge_zip`, `install_modpack`, `update_modpack`) enqueue a `TaskJob` into `TaskManager` and return the `u64` task id; the terminal result (e.g. `MrpackImportResult`, `CfImportResult`, `ModpackInstallResult`, `PackUpdateResult`) is attached to the task snapshot via `finish_done_with_result` and arrives on the `task://update` event. `set_pack_lock` remains synchronous (no task). `core/modpack.rs` also provides `remap_to_staging` + `promote_staging` shared by all five job types (`ImportMrpackJob`, `ImportCfZipJob`, `UpdateModpackJob`, `ModAddJob`, `ModUpdateJob`).

## Artifacts

- `src/components/NewInstanceModal.tsx` — Import tab: single file picker via `@tauri-apps/plugin-dialog`'s `open()`, routes by extension (`.mrpack` → `importMrpack`, `.zip` → `importCurseforgeZip`); on success the `task://update` Done event carries the slug; toast rendered via `Toasts` component from the store; navigates to new instance
- `src/routes/Home.tsx` — exports `ImportResultToast` / `CfImportResultToast` as named exports (installed/skipped/failed counts; manual list with project-page links); used by Browse cards; no import buttons directly on this page
- `src/routes/Browse.tsx` — `ModpackCard` has a primary **Install** button (calls `installModpack(pack.provider, pack.id, pack.pageUrl ?? undefined)`, which returns a task id; result toast rendered via `Toasts` + `ImportResultToast`/`CfImportResultToast` from store once `Done`) and a secondary open-page button; version dropdown fetched lazily
- `src/routes/InstanceDetail.tsx` — `PackSourcePanel` component: shows pack source (provider + project id + version), **Update** button (`updateModpack(slug, selectedVersionId)` → task id), version dropdown (lazily fetched), and **Lock/Unlock** toggle (`setPackLock(slug, !packLocked)`); `packLocked` prop propagates to `InstalledModsTab` and `AddModTab` to disable mod-mutation actions

## CLI code

- `src-tauri/src/core/modpack.rs` (1188 lines) — pure parse/plan/extract module; slice A: `MrpackManifest`/`MrpackFile`/`FileEnv`/`PackLoader` types, `parse_modrinth_index`, `PackPlan`, `build_pack_plan` (host allowlist, hash pick sha512>sha1, env filter, path-safety guard), `extract_overrides`/`extract_prefix`/`is_safe_dest`, `read_mrpack`; slice B: `CfManifest`/`CfManifestFile`/`CfManualFile`/`CfResolveFailure` types, `parse_cf_manifest`, `CfPackPlan`, `build_cf_pack_plan`, `resolve_and_build_cf_plan`; slice C: `ResolvedPackFile`, `resolve_pack_file`; slice D: `PackUpdatePlan`, `plan_pack_update`; shared: `ModpackError`, `validate_relative_path`; **stage-and-promote helpers** `remap_to_staging(items, mc_dir, staging_dir)` (rewrites `dest` fields to land under `staging_dir`), `promote_staging(staging_dir, target_dir)` (atomic rename each file from staging into target; sibling-dir constraint makes rename atomic at OS level); 79 unit tests in sibling `src-tauri/src/core/modpack_tests.rs`, wired via `#[cfg(test)] #[path = "modpack_tests.rs"] mod tests;` stub
- `src-tauri/src/core/curseforge.rs` — `CurseForgeProvider::get_file(client, project_id, file_id)` (single-file resolver for slice B)
- `src-tauri/src/lib.rs` — `ImportMrpackJob`, `ImportCfZipJob`, `UpdateModpackJob` are `TaskJob` implementors (Planning → Downloading → Applying lifecycle via `TaskContext`); `import_mrpack` / `import_curseforge_zip` / `update_modpack` async Tauri commands enqueue these jobs and return `u64` task ids; `install_modpack` also enqueues (Mrpack or CF branch); `set_pack_lock` remains synchronous; `ModpackInstallResult` tagged enum (`Mrpack`/`Curseforge`/`Manual`) and `PackUpdateResult` struct serialized as camelCase; `staging_dir_for(inst_dir, task_id)` returns `<inst_dir>/.staging-<task_id>/`

## Docs

- `docs/spec/modpack-import.md` — implementation contract for all four slices; all shipped
- `docs/spec/ui-modpack-rework.md` — CP1-CP2 spec: import flow moved to NewInstanceModal Import tab
- `docs/spec/download-runner-rework/cp-3-pack-ops-stage-promote.md` — CP-3 spec: `ImportMrpackJob`, `ImportCfZipJob`, `UpdateModpackJob` task wrappers; staging dir pattern; `finish_done_with_result` result carrier
- `docs/design/modpack-import.md` — design doc: slice A/B format ground truth, behavior rules, Mermaid architecture diagrams, file-resolution approach comparison, rejected approaches
- `docs/design/download-runner-rework.md` — overall task queue design covering all CP-3–CP-4 job implementations
- `docs/design/ui-modpack-rework.md` — rationale for moving import into NewInstanceModal

## Coupling

- **download-runner domain** — all three pack job types enqueue into `TaskManager` and use `TaskContext` for lifecycle progression; `execute_plan_cancellable` (from `download.rs`) is the download step; `remap_to_staging`/`promote_staging` (defined here in `modpack.rs`) are shared with `ModAddJob`/`ModUpdateJob` in `lib.rs`; terminal results ride `task://update` via `finish_done_with_result`
- **instances domain** — `import_mrpack`/`import_curseforge_zip`/`update_modpack` job implementations call `instances::create`, `instances::load_manifest`, `instances::save_manifest`; `ModEntry` (defined in `instances.rs`, includes `from_pack: bool`) is the output shape all planners produce; `Instance.pack_locked` + `instances::ensure_not_locked`/`set_pack_lock` are in `instances.rs`
- **providers domain** — `resolve_and_build_cf_plan` calls `CurseForgeProvider::get_file` and depends on `ProviderHttpClient`/`ProviderError`/`VersionFile`; `resolve_pack_file` calls provider APIs; any change to those types or `cf_api_key_from` requires a matching update here
- **download domain** — both job implementations build a `DownloadPlan` and call `download::execute_plan_cancellable`/`build_client`; `ItemStatus`/`NoOpSink` come from `download.rs`
- **frontend-shell / IPC** — `MrpackImportResult`, `CfImportResult`, `CfManualFile`, `ModpackInstallResult`, `PackUpdateResult` are hand-mirrored in `src/lib/ipc.ts`; `importMrpack`/`importCurseforgeZip`/`installModpack`/`updateModpack` return `Promise<number>` (task id); results arrive on `task://update` when `status.kind === "done"` and `task.result` is set; any Rust struct change requires a manual `ipc.ts` edit
- **mod-install domain** — `remap_to_staging`/`promote_staging` defined here are imported by `ModAddJob`/`ModUpdateJob` in `lib.rs` (`use core::modpack::{promote_staging, remap_to_staging}`)

## Conventions worth knowing

- Pure-planner / thin-executor split mirrors `mod_install.rs` and `resolver.rs`: all security-critical logic lives in unit-tested pure functions; the Tauri command only does I/O orchestration.
- `staging_dir_for(inst_dir, task_id)` → `<inst_dir>/.staging-<task_id>/`; task_id is the `u64` assigned at enqueue; staging dir is a sibling of the instance mods dir so `rename()` is same-filesystem and atomic.
- `remap_to_staging` rewrites only items whose `dest` falls under `mc_dir`; items outside `mc_dir` are passed through unchanged (e.g. items already pointing at cache).
- `promote_staging` is a recursive rename-walk; on success staging dir is left empty (caller removes it); on cancel/fail caller calls `remove_dir_all` on the staging dir.
- `plan_pack_update` is a pure diff: given `current_mods` and `new_plan_entries`, partitions entries by `from_pack` flag and filename collision. User-added mods (`from_pack=false`) survive unless their filename is overridden by a new pack entry.
- mrpack download host allowlist (`ALLOWED_HOSTS`): `cdn.modrinth.com`, `github.com`, `raw.githubusercontent.com`, `gitlab.com` — a `downloads[]` URL outside this list aborts the whole import via `ModpackError::DisallowedHost`.
- `validate_relative_path` rejects absolute paths, `\`-prefixed paths, Windows drive-letter prefixes, and any `..` component.
- CF hash preference in `build_cf_pack_plan`: only `sha1` is checked — a resolved file with only an md5 hash routes to `manual`.
- `CfManualFile.page_url` is built as `https://www.curseforge.com/projects/<projectID>` (numeric-id-based; tracked follow-up `modpack-import-cf-manual-slug-link`).
- No rollback on partial failure: a half-populated instance may be left on disk — tracked follow-up `modpack-import-partial-cleanup`.
- `src-tauri/tests/curseforge_live.rs` is a `#[ignore]`d integration test hitting the real CF API; reads the key from a gitignored `.env`.
