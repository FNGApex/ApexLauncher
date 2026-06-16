# instances

## What it does

Manages the full lifecycle of Minecraft instances: create, list, get (with mods-folder reconciliation), delete, and per-instance mod state operations (enable/disable, remove, update). Instances are stored as `instance.json` under `<data>/instances/<slug>/`. Newly created instances inherit the global default memory from `settings.json`. Playtime and last-played timestamp are recorded on game exit via `record_playtime`.

## Artifacts

- `src/routes/Home.tsx` — instance grid; delete via confirmation dialog; opens `NewInstanceModal`; TanStack Query key `["instances"]`
- `src/routes/InstanceDetail.tsx` — detail view: stats (memory, Java, mod count, created, last-played, playtime), reconciled mods list (`folderMods`) with per-mod enable/disable toggle, update, and remove buttons; launch/stop controls; log console (max 500 lines); subscribes to `launch://log`, `launch://exit`, `install://log` events
- `src/components/NewInstanceModal.tsx` — create-instance dialog; fetches MC version list and per-MC loader builds live; guards submit on name + MC version + (if non-vanilla) loader build being set

## CLI code

- `src-tauri/src/core/instances.rs` — `Instance`, `Loader`, `JavaCfg`, `Source`, `ModEntry`, `FolderMod`, `InstanceDetail`, `CreateInstanceReq` structs; `list`, `create`, `get`, `delete` fns; `record_playtime` (injectable clock, path-based, no `AppHandle`); `load_manifest`/`save_manifest` pub helpers; `set_mod_enabled`/`set_mod_enabled_on_disk`, `remove_mod`/`remove_mod_from_disk`, `remove_mod_from_disk_files` mod-state ops; `slugify`/`unique_slug`/`validate_slug`/`validate_mod_file_name` validators; `scan_mods` reconciles `mc/mods/` against `mods[]`; 14 unit tests
- `src-tauri/src/core/materialize.rs` — `materialize_core` (injectable `link_fn` closure) + `materialize` (public wrapper using `std::fs::hard_link`); copies relative paths from `cache_root` into `instance_dir`; creates parent dirs; skips existing destinations (idempotent); copy fallback fires only on cross-device errors via `is_cross_device` predicate (`io::ErrorKind::CrossesDevices` — covers Linux/macOS EXDEV=18 and Windows ERROR_NOT_SAME_DEVICE=17 — plus a raw `Some(18)` arm to keep injected-EXDEV unit tests green on non-Windows hosts); all other link errors propagate as `Err`; 8 unit tests; called from `launch_instance` (step 6b) in `lib.rs` via `launch::rewrite_classpath_for_instance` + `materialize`
- `src-tauri/src/core/settings.rs` — `Settings` struct (schema=1, `default_memory_mb`=4096, `default_java_args`=`-XX:+UseG1GC`, `curseforge_api_key: Option<String>`, `offline_mode: bool`); `load`/`save`; blank API key normalized to `None` on save; missing file returns `Settings::default()`
- `src-tauri/src/core/store.rs` — path helpers via `app.path().data_dir()` + join `"ApexLauncher"`: `data_dir`, `instances_dir`, `cache_dir`, `cache_assets_dir`, `cache_libraries_dir`, `cache_versions_dir`, `cache_java_dir`, `cache_meta_dir`, `cache_installers_dir`, `account_file`, `java_dir` (alias for `cache_java_dir`); creates dirs on demand; data root independent of bundle id; two pure pub helpers `data_root_from_base(base: &Path)` and `cache_subdir_path(root: &Path, sub: &str)` for integration-test use without `AppHandle`; 13 inline unit tests
- `src-tauri/src/lib.rs` — Tauri command wrappers for this domain: `list_instances`, `create_instance`, `get_instance`, `delete_instance`, `get_settings`, `save_settings`, `app_paths`, `add_mod`, `set_mod_enabled`, `remove_mod`, `update_mod`; `launch_instance` calls `materialize` at step 6b

## Docs

- `docs/ARCHITECTURE.md` §2-3 — on-disk layout and `instance.json` schema
- `docs/ROADMAP.md` Phase 1 — scope of what is implemented

## Coupling

- `NewInstanceModal` calls into the metadata domain to populate MC version and loader dropdowns; a change to `McVersion` or `LoaderOption` IPC types requires updating `src/lib/ipc.ts` and the modal.
- `create` reads `settings::load` to seed `memory_mb` on new instances; settings domain changes cascade here.
- `launch_instance` in `lib.rs` calls `materialize` after `launch::rewrite_classpath_for_instance`; changes to the classpath rewrite contract affect what paths are materialized into the instance tree (launch domain).
- `add_mod` and `update_mod` in `lib.rs` both call into the mod-install domain (`core/mod_install.rs`), which owns the resolve/swap/update logic; `update_mod` additionally calls `instances::remove_mod_from_disk_files` + `instances::save_manifest` directly.
- `src/lib/ipc.ts` hand-mirrors Rust struct field names (camelCase via `serde rename_all`); types are not generated — drift is a manual risk. Note: the `Settings` interface in `ipc.ts` is missing the `offlineMode` field that exists in the Rust `Settings` struct.

## Conventions worth knowing

- Slugs: lowercase alphanumeric + `-` only; `validate_slug` guards path traversal on `get`/`delete`/`set_mod_enabled`/`remove_mod`.
- `validate_mod_file_name` guards mod file name inputs: must end in `.jar`, no `/`, `\`, `:`, no `..` traversal, no absolute paths.
- `scan_mods` recognizes `.disabled` / `.DISABLED` suffix as the enable/disable convention; strips the suffix to get the base `.jar` name; non-`.jar` files are ignored; result sorted case-insensitively by `file_name`.
- `set_mod_enabled_on_disk` is idempotent: already-in-target-state is a no-op; also flips `enabled` on the matching `ModEntry` in the manifest.
- `remove_mod_from_disk` removes both `.jar` and `.jar.disabled` forms; missing file is not an error; drops the `ModEntry` from the manifest. `remove_mod_from_disk_files` removes files only, no manifest write (used by `update_mod`).
- `SCHEMA_VERSION = 1` in both `instances.rs` and `settings.rs`; bump triggers future migrations.
- Instance list is sorted by `created` (ISO 8601 string compare, oldest first).
- TanStack Query key for instance list is `["instances"]`; per-instance key is `["instance", slug]`; mutations call `invalidateQueries` on the appropriate key.
- `record_playtime` takes `inst_dir: &Path` directly (not `AppHandle`) so it can be called from the launch domain after process exit without threading `AppHandle` through the exit handler.
