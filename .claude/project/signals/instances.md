# instances

## What it does

Manages the full lifecycle of Minecraft instances: create, list, get (with mods-folder reconciliation), and delete. Instances are stored as `instance.json` under `<data>/instances/<slug>/`. Newly created instances inherit the global default memory from `settings.json`.

## Artifacts

- `src/routes/Home.tsx` — instance grid, delete button, opens NewInstanceModal
- `src/routes/InstanceDetail.tsx` — instance detail view: stats + reconciled mods list (managed vs. unmanaged)
- `src/components/NewInstanceModal.tsx` — create-instance dialog; fetches MC version and loader lists live; guards submit on name + MC + (if non-vanilla) loader build being set

## CLI code

- `src-tauri/src/core/instances.rs` — `Instance`, `FolderMod`, `InstanceDetail`, `CreateInstanceReq` structs; `list`, `create`, `get`, `delete` fns; `slugify`/`unique_slug`/`validate_slug` helpers; `scan_mods` reconciles `mc/mods/` against `mods[]`
- `src-tauri/src/core/settings.rs` — `Settings` struct (schema=1, defaultMemoryMb=4096, defaultJavaArgs=`-XX:+UseG1GC`, curseforgeApiKey, `offline_mode: bool` added in Phase 3); `load`/`save`; blank API key normalized to `None` on save; `offline_mode` defaults to `false`
- `src-tauri/src/core/store.rs` — `data_dir`, `instances_dir`, `java_dir`, `accounts_file` via Tauri path API; creates respective dirs on demand; `accounts_file` added in Phase 3 for auth domain
- `src-tauri/src/lib.rs` — thin Tauri command wrappers: `list_instances`, `create_instance`, `get_instance`, `delete_instance`, `get_settings`, `save_settings`, `app_paths`

## Docs

- `docs/ARCHITECTURE.md` §2-3 — on-disk layout and `instance.json` schema
- `docs/ROADMAP.md` Phase 1 — scope of what is implemented

## Coupling

- `NewInstanceModal` calls into the metadata domain to populate MC version and loader dropdowns; a change to `McVersion` or `LoaderOption` IPC types requires updating `src/lib/ipc.ts` and the modal.
- `create_instance` reads `settings::load` to seed `memory_mb` on new instances; settings domain changes cascade here.
- `src/lib/ipc.ts` hand-mirrors Rust struct field names (camelCase via `serde rename_all`); types are not generated — drift is a manual risk.

## Conventions worth knowing

- Slugs: lowercase alphanumeric + `-` only; `validate_slug` guards path traversal on `get`/`delete`.
- `scan_mods` recognizes `.disabled` / `.DISABLED` suffix as the enable/disable convention; non-`.jar` files are ignored.
- `SCHEMA_VERSION = 1` in both `instances.rs` and `settings.rs`; bump triggers future migrations.
- Instance list is sorted by `created` (ISO 8601 string compare, oldest first).
- TanStack Query key for instances is `["instances"]`; mutations call `invalidateQueries` on that key.
