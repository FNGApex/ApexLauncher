# instances

## What it does

Manages the full lifecycle of Minecraft instances: create, list, get (with mods-folder reconciliation), delete, and per-instance mod state operations (enable/disable, remove, update). Instances are stored as `instance.json` under `<data>/instances/<slug>/`. Newly created instances inherit the global default memory from `settings.json`. Playtime and last-played timestamp are recorded on game exit via `record_playtime`. Per-instance Java/RAM configuration: `JavaCfg` struct holds `major`, `memory_mb`, `min_memory_mb`, `args_override`, `path_override`, `use_pack_settings`; `set_instance_java` / `set_instance_java_on_disk` persist it; `java_resolve.rs` provides the pure 3-tier resolver used at launch time. `ModEntry` now carries `name`, `icon_url`, `summary` captured at add-time. `Source` carries pack update-check fields (`last_update_check`, `latest_version`, `latest_version_id`) and display fields (`icon_url`, `author`). `needs_update_check` helper throttles to once per 24h. **CF manual-download UX:** `PendingManual` struct (`project_id: u64`, `file_id: u64`, `file_name: String`, `page_url: String`, `expected_sha1: Option<String>`, `size: Option<u64>`); `Instance` carries `pending_manual: Vec<PendingManual>` + `suppress_pending_launch_warning: bool` (both `#[serde(default)]`); `reconcile_pending_manual(inst, mods_dir)` (pure, idempotent, hash-verified) removes resolved entries and returns `Vec<ResolvedManual>`; `ResolvedManual { file_name, sha1_matched: bool, sha1_mismatch: Option<String> }`; `set_pending_launch_warning_suppressed(_on_disk)` helpers persist the suppression flag.

## Artifacts

- `src/routes/Home.tsx` — instance grid; delete via confirmation dialog; opens `NewInstanceModal`; TanStack Query key `["instances"]`
- `src/routes/InstanceDetail.tsx` — tabbed detail shell; tabs: Info, Modlist, Tech, Java
- `src/components/NewInstanceModal.tsx` — two-tab dialog: **Create** and **Import pack**
- `src/routes/instance-tabs/JavaTab.tsx` — per-instance Java/RAM config form; `validateJavaPath` probe; calls `setInstanceJava` on save
- `src/routes/instance-tabs/TechTab.tsx` — read-only instance stats; effective Java/RAM display

## CLI code

- `src-tauri/src/core/instances.rs` — `Instance` (adds `pending_manual: Vec<PendingManual>`, `suppress_pending_launch_warning: bool`), `PendingManual { project_id, file_id, file_name, page_url, expected_sha1, size }`, `ResolvedManual { file_name, sha1_matched, sha1_mismatch }`, `Loader`, `JavaCfg` (`major: Option<u32>`, `args_override: Option<String>`, `memory_mb: u32`, `min_memory_mb: Option<u32>`, `path_override: Option<String>`, `use_pack_settings: bool`), `RecommendedJava`, `Source` (`provider, project_id, file_id, pack_version, recommended, page_url, icon_url, author, last_update_check, latest_version, latest_version_id`), `ModEntry` (`provider, project_id, version_id, file_name, hashes, enabled, side, from_pack, name, icon_url, summary`), `FolderMod`, `InstanceDetail`, `CreateInstanceReq`; `list`, `create`, `get`, `delete`; `record_playtime`; `load_manifest`/`save_manifest`; `set_mod_enabled`/`set_mod_enabled_on_disk`, `remove_mod`/`remove_mod_from_disk`/`remove_mod_from_disk_files`; `ensure_not_locked`; `set_pack_lock`/`set_pack_lock_on_disk`; `set_instance_java`/`set_instance_java_on_disk`; `set_pending_launch_warning_suppressed`/`set_pending_launch_warning_suppressed_on_disk`; `reconcile_pending_manual(inst: &mut Instance, mods_dir: &Path) -> Vec<ResolvedManual>` (pure, idempotent; scans mods dir for each pending entry by file name; validates sha1 if `expected_sha1` present; removes resolved entries from `pending_manual`); `slugify`/`unique_slug`/`validate_slug`/`validate_mod_file_name`; `scan_mods`; `needs_update_check(last: Option<&str>, now: DateTime<Utc>) -> bool`; 46 unit tests in sibling `instances_tests.rs`
- `src-tauri/src/core/java_resolve.rs` (106 lines) — `EffectiveJava { xmx_mb, xms_mb, extra_args, java_path }`; `resolve_effective_java(inst, settings) -> EffectiveJava`; pure function, no I/O; 3-tier precedence: (1) `inst.source.recommended` fields when `Some`, (2) per-instance `inst.java` when `use_pack_settings == true`, (3) `settings` global defaults; 11 unit tests in sibling `java_resolve_tests.rs`
- `src-tauri/src/core/materialize.rs` (149 lines) — `materialize_core` (injectable `link_fn` closure) + `materialize` (public wrapper using `std::fs::hard_link`); copies relative paths from `cache_root` into `instance_dir`; cross-device copy fallback; 9 unit tests in sibling `materialize_tests.rs`
- `src-tauri/src/core/settings.rs` — `Settings` struct: `schema=1`, `default_memory_mb=4096`, `default_java_args="-XX:+UseG1GC"`, `curseforge_api_key: Option<String>`, `offline_mode: bool`, `sidebar_start_collapsed: bool`, `auto_download_java: bool`, `show_console_default: bool`, `keep_launcher_open: bool`, `maximize_on_start: bool`; `load`/`save`; blank API key normalized to `None` on save; 15 unit tests in sibling `settings_tests.rs`
- `src-tauri/src/core/store.rs` (132 lines) — path helpers: `data_dir`, `instances_dir`, `cache_dir`, `cache_assets_dir`, `cache_libraries_dir`, `cache_versions_dir`, `cache_java_dir`, `cache_meta_dir`, `cache_installers_dir`, `account_file`; `data_root_from_base` + `cache_subdir_path` pure helpers; 13 unit tests in sibling `store_tests.rs`
- `src-tauri/src/lib.rs` — Tauri command wrappers: `list_instances`, `create_instance`, `get_instance`, `delete_instance`, `get_settings`, `save_settings`, `app_paths`, `set_mod_enabled`, `remove_mod`, `set_pack_lock`, `set_instance_java`, `set_pending_launch_warning_suppressed`, `rescan_pending_manual`, `import_manual_file`, `start_pending_watch`, `stop_pending_watch`; `add_mod`/`update_mod` enqueue `TaskJob` into `TaskManager` and return `u64` task id; all others are synchronous (no task id); `PendingWatcher` managed state (`Mutex<Option<(slug, Debouncer)>>`) — lazy `notify` watch on the open instance's `mods/` dir; `reconcile_and_emit` internal helper (reconciles, persists, emits `manual://resolved` event); `launch_instance` calls `reconcile_pending_manual` at prep time and persists if anything resolved

## Docs

- `docs/ARCHITECTURE.md` §2-3 — on-disk layout and `instance.json` schema
- `docs/ROADMAP.md` Phase 1 — scope of what is implemented
- `docs/spec/ui-overhaul.md` WS-A — per-instance Java/RAM config spec
- `docs/spec/cf-manual-download-ux.md` — CF manual-download UX spec (PendingManual, reconcile, watcher, launch gate)
- `docs/design/cf-manual-download-ux.md` — design rationale

## Coupling

- `NewInstanceModal` calls into the metadata domain to populate MC version and loader dropdowns.
- `create` reads `settings::load` to seed `memory_mb` on new instances.
- `launch_instance` in `lib.rs` calls `java_resolve::resolve_effective_java` (step 2) to compute the effective Java/RAM config before JVM argv assembly; calls `materialize` (step 6b).
- `add_mod` and `update_mod` in `lib.rs` call into the mod-install domain (`core/mod_install.rs`).
- `import_mrpack`/`import_curseforge_zip` in `lib.rs` (modpack domain) call `instances::create`, `instances::load_manifest`, and `instances::save_manifest`; they also populate `instance.pending_manual` from `CfManualFile` entries via `From<&CfManualFile> for PendingManual`.
- `refresh_pack_meta` in `lib.rs` (modpack domain) calls `instances::load_manifest`, `instances::save_manifest`, `needs_update_check` to throttle, and writes `last_update_check`/`latest_version`/`latest_version_id` to the manifest.
- `enrich_instance_mods` in `lib.rs` calls `instances::load_manifest`/`instances::save_manifest` and updates `ModEntry.name/icon_url/summary` fields.
- `launch_instance` in `lib.rs` calls `instances::reconcile_pending_manual` at prep time before JVM argv assembly; persists the manifest if anything resolved; emits `manual://resolved` per resolution.
- `rescan_pending_manual` / `import_manual_file` / `start_pending_watch` / `stop_pending_watch` in `lib.rs` all call into `instances::reconcile_pending_manual` (via `reconcile_and_emit` helper) and manage the `PendingWatcher` state.

## Conventions worth knowing

- Slugs: lowercase alphanumeric + `-` only; `validate_slug` guards path traversal on `get`/`delete`/`set_mod_enabled`/`remove_mod`/`set_instance_java`.
- `validate_mod_file_name` guards mod file name inputs: must end in `.jar`, no `/`, `\`, `:`, no `..` traversal, no absolute paths.
- `scan_mods` recognizes `.disabled` / `.DISABLED` suffix; strips it to get the base `.jar` name; non-`.jar` files ignored; result sorted case-insensitively by `file_name`.
- `set_mod_enabled_on_disk` is idempotent: already-in-target-state is a no-op.
- `remove_mod_from_disk` removes both `.jar` and `.jar.disabled` forms; missing file is not an error.
- `SCHEMA_VERSION = 1` in both `instances.rs` and `settings.rs`; bump triggers future migrations.
- Instance list is sorted by `created` (ISO 8601 string compare, oldest first).
- TanStack Query keys: `["instances"]` (list), `["instance", slug]` (detail).
- `needs_update_check` returns `true` when `last` is `None` or the parsed timestamp is more than 24h before `now`. Used by `refresh_pack_meta` in `lib.rs` to skip the network call.
- `JavaCfg.use_pack_settings == false` means use the global default from `Settings`; `== true` means use the instance's own memory/args/path. The name "pack settings" is slightly misleading — it means "per-instance settings override", not pack-recommended.
- `reconcile_pending_manual` is idempotent: a second call with the same `mods_dir` state is a no-op once all pending entries are resolved (or still missing). `try_resolve_pending` (private) checks file existence by name, then verifies sha1 if `expected_sha1` is `Some`; a size mismatch alone is not grounds for rejection (sha1 is authoritative).
- `PendingWatcher` watches one instance's `mods/` dir at a time; `start_pending_watch` with a new slug replaces any prior watch (drops old debouncer). `stop_pending_watch` only stops if the slug matches the currently-watched slug.
- `suppress_pending_launch_warning` is a per-instance opt-out written to `instance.json`; it survives sessions but is reset to `false` if `pending_manual` becomes empty (reconcile clears both).
