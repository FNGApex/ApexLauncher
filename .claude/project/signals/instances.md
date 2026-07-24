# instances

## Overview

Instance CRUD and on-disk layout. Instances stored as `instance.json` under `<data>/instances/<slug>/`. Newly created instances seed `memory_mb` from global settings. Playtime and last-played recorded on game exit via `record_playtime`. Hardlink materializer in `materialize.rs`.

**Per-instance Java/RAM:** `JavaCfg` (`major`, `memory_mb`, `min_memory_mb`, `args_override`, `path_override`, `use_pack_settings`); persisted by `set_instance_java`. `java_resolve.rs` pure 3-tier resolver used at launch time.

**ModEntry metadata:** `name`, `icon_url`, `summary` captured at add-time; old manifests have `None`.

**CF manual-download tracking:** `PendingManual { project_id, file_id, file_name, page_url, expected_sha1, size }`; `Instance.pending_manual: Vec<PendingManual>` + `Instance.suppress_pending_launch_warning: bool` (both `#[serde(default)]`). `reconcile_pending_manual(inst, mods_dir)` — pure, idempotent, hash-verified; returns `Vec<ResolvedManual>`.

**Custom icons:** `Instance.icon: Option<String>` holds relative filename (e.g. `icon-1751000000000.png`). `write_instance_icon` — ext allowlist {png,jpg,jpeg,webp,gif}, 4 MiB cap, removes prior `icon-*` files. `clear_instance_icon_file` removes all `icon-*` files.

## CLI code

- `src-tauri/src/core/instances.rs` — `Instance` (carries `icon`, `pending_manual`, `suppress_pending_launch_warning`), `PendingManual`, `ResolvedManual`, `Loader`, `JavaCfg`, `RecommendedJava`, `Source` (`provider, project_id, file_id, pack_version, recommended, page_url, icon_url, author, last_update_check, latest_version, latest_version_id`), `ModEntry` (`provider, project_id, version_id, file_name, hashes, enabled, side, from_pack, name, icon_url, summary`), `FolderMod`, `InstanceDetail`, `CreateInstanceReq`; `list`, `create`, `get`, `delete`; `record_playtime`; `load_manifest`/`save_manifest`; `set_mod_enabled`/`set_mod_enabled_on_disk`; `remove_mod`/`remove_mod_from_disk`; `ensure_not_locked`; `set_pack_lock`/`set_pack_lock_on_disk`; `set_instance_java`/`set_instance_java_on_disk`; `set_pending_launch_warning_suppressed`/`set_pending_launch_warning_suppressed_on_disk`; `reconcile_pending_manual`; `write_instance_icon`; `clear_instance_icon_file`; `slugify`/`unique_slug`/`validate_slug`/`validate_mod_file_name`; `scan_mods`; `needs_update_check`; 52 unit tests in `instances_tests.rs`
- `src-tauri/src/core/java_resolve.rs` — `EffectiveJava { xmx_mb, xms_mb, extra_args, java_path }`; `resolve_effective_java(inst, settings)` pure, no I/O; 3-tier: (1) `inst.source.recommended`, (2) per-instance when `use_pack_settings == true`, (3) settings global; 11 tests in `java_resolve_tests.rs`
- `src-tauri/src/core/materialize.rs` — `materialize_core` (injectable `link_fn`) + `materialize` (hardlink with copy fallback); 9 tests in `materialize_tests.rs`
- `src-tauri/src/core/settings.rs` — `Settings`: `schema=1`, `default_memory_mb=4096`, `default_java_args`, `curseforge_api_key`, `offline_mode`, `sidebar_start_collapsed`, `auto_download_java`, `show_console_default`, `keep_launcher_open`, `maximize_on_start`; `load`/`save`; blank API key normalized to `None`; 15 tests in `settings_tests.rs`
- `src-tauri/src/core/store.rs` — path helpers: `data_dir`, `instances_dir`, `cache_dir`, cache subdirs, `account_file`; 13 tests in `store_tests.rs`
- `src-tauri/src/lib.rs` — Tauri commands: `list_instances`, `create_instance`, `get_instance`, `delete_instance`, `get_settings`, `save_settings`, `app_paths`, `set_mod_enabled`, `remove_mod`, `set_pack_lock`, `set_instance_java`, `set_pending_launch_warning_suppressed`, `rescan_pending_manual`, `import_manual_file`, `start_pending_watch`, `stop_pending_watch`, `set_instance_icon`, `clear_instance_icon`; `PendingWatcher` managed state (`Mutex<Option<(slug, Debouncer)>>`); `reconcile_and_emit` helper (reconciles, persists, emits `manual://resolved`)

## Artifacts

- `src/routes/Home.tsx` — instance grid; delete via confirmation; opens `NewInstanceModal`; query key `["instances"]`
- `src/routes/InstanceDetail.tsx` — tabbed detail shell (Info/Modlist/Tech/Java); instance icon header with hover Set/Remove picker; `N missing` badge + `PendingLaunchModal`
- `src/components/NewInstanceModal.tsx` — Create and Import pack tabs
- `src/routes/instance-tabs/JavaTab.tsx` — per-instance Java/RAM config form; calls `setInstanceJava` on save

## Docs

- `docs/ARCHITECTURE.md` §2-3 — on-disk layout and `instance.json` schema
- `docs/spec/cf-manual-download-ux.md` — PendingManual, reconcile, watcher, launch gate
- `docs/spec/theme-and-icons.md` — custom instance icon spec

## Coupling

- `create` seeds `memory_mb` from `settings::load`.
- `launch_instance` calls `java_resolve::resolve_effective_java` (step 2) and `materialize` (step 6b); calls `reconcile_pending_manual` at prep time.
- `import_mrpack`/`import_curseforge_zip` (modpack domain) call `instances::create`/`load_manifest`/`save_manifest`; populate `instance.pending_manual` from `CfManualFile` entries.
- `refresh_pack_meta` (modpack domain) calls `load_manifest`/`save_manifest`/`needs_update_check`; writes `last_update_check`/`latest_version`/`latest_version_id`.
- `enrich_instance_mods` calls `load_manifest`/`save_manifest` and updates `ModEntry.name/icon_url/summary`.

## Conventions

- Slugs: lowercase alphanumeric + `-` only; `validate_slug` guards path traversal.
- `validate_mod_file_name`: must end in `.jar`, no `/\:`, no `..`, no absolute paths.
- `scan_mods`: recognizes `.disabled`/`.DISABLED` suffix; non-`.jar` files ignored; result sorted case-insensitively.
- `set_mod_enabled_on_disk` is idempotent. `remove_mod_from_disk` removes both `.jar` and `.jar.disabled`; missing file is not an error.
- `SCHEMA_VERSION = 1` in both `instances.rs` and `settings.rs`.
- Instance list sorted by `created` (ISO 8601 string compare, oldest first).
- TanStack Query keys: `["instances"]` (list), `["instance", slug]` (detail).
- `reconcile_pending_manual` is idempotent. `try_resolve_pending` checks file existence then verifies sha1 if `expected_sha1` is `Some`; size mismatch alone is not rejection.
- `PendingWatcher` watches one instance's `mods/` at a time; `start_pending_watch` with a new slug replaces any prior watch.
- `Instance.icon` is a relative filename, not an absolute path. Frontend constructs a `convertFileSrc` URL from `<data_dir>/instances/<slug>/<icon>`.
- `JavaCfg.use_pack_settings == false` → use global settings default. `== true` → use per-instance override.
