# Domain: launcher-import

Pure parse/plan layer for importing existing Prism Launcher / MultiMC / PolyMC instances into ApexLauncher. Orchestration (stage+promote, Tauri job) lives in `lib.rs`; all parse/plan functions in the module are pure (no I/O except `copy_game_dir` and `resolve_icon_path`).

## Key files

| File | Role |
|------|------|
| `src-tauri/src/core/launcher_import.rs` | Pure parse/plan module (CP-1..CP-6) |
| `src-tauri/src/core/launcher_import_tests.rs` | 82 unit tests |
| `src-tauri/src/lib.rs` | `ImportExternalJob` (TaskJob) + `import_external_instance` command + `enqueue_import_external` + `ExternalImportResult` DTO |
| `src/components/NewInstanceModal.tsx` | "From launcher" third tab (folder picker + controls) |
| `src/lib/ipc.ts` | `importExternalInstance` wrapper |
| `src/components/Toasts.tsx` | Surfaces `ExternalImportResult.warnings` as amber toast |
| `docs/spec/launcher-import.md` | Specification |
| `docs/design/launcher-import.md` | Design doc |

## Parse/plan functions (all pure)

| Function | Input → Output | Notes |
|----------|----------------|-------|
| `parse_instance_cfg(text)` | `&str → Result<PrismInstanceCfg, LauncherImportError>` | flat `key=value` INI; tolerates `[General]` header; unknown keys silently ignored |
| `parse_mmc_pack(text)` | `&str → Result<MmcPack, LauncherImportError>` | parses `mmc-pack.json`; uid→loader map; `dependencyOnly` components skipped |
| `resolve_game_dir(dir)` | `&Path → Option<PathBuf>` | checks `.minecraft/` first, then `minecraft/` |
| `copy_game_dir(src, dest, skip_logs)` | `(&Path, &Path, bool) → Result<u32, LauncherImportError>` | recursive; path-safe; optional skip of `logs/` + `crash-reports/`; Windows junction/symlink skip; returns file count |
| `resolve_icon_path(dir, key)` | `(&Path, &str) → Option<PathBuf>` | filesystem stat; traversal-guarded via `icon_key_is_safe` |
| `plan_external_import(cfg, pack, name_override)` | `(&PrismInstanceCfg, &MmcPack, Option<&str>) → Result<ExternalImportPlan, LauncherImportError>` | pure orchestration; name = name_override[non-empty] ?? cfg.name ?? "Imported"; accumulates `warnings` |
| `identify_mods_modrinth(client, mods_dir)` | `(&dyn ProviderHttpClient, &Path) → Result<Vec<ModEntry>, LauncherImportError>` | CP-6; opt-in; SHA-1 hashes `*.jar`/`*.jar.disabled` in mods_dir; ONE batched `POST /v2/version_files`; empty dir → zero HTTP calls; `name/icon_url/summary = None` per api-frugality |

## Key types

**`PrismInstanceCfg`** — parsed from `instance.cfg`:
- `name: Option<String>`, `icon_key: Option<String>`, `instance_type: Option<String>`
- `override_memory: bool`, `min_mem_mb: Option<u32>`, `max_mem_mb: Option<u32>`
- `override_java_location: bool`, `java_path: Option<String>`
- `override_java_args: bool`, `jvm_args: Option<String>`

**`MmcPack`** — parsed from `mmc-pack.json`:
- `minecraft: String`, `loader: ImportedLoader`

**`ImportedLoader`** enum:
- `Vanilla` — no loader component present
- `Loader { kind: String, version: String }` — recognized loader (`fabric`/`quilt`/`forge`/`neoforge`)
- `Unsupported(String)` — unrecognized uid (e.g. `liteloader`); job imports as vanilla + warning

**uid → loader mapping** (from Prism meta):
| uid | loader |
|-----|--------|
| `net.fabricmc.fabric-loader` | `"fabric"` |
| `org.quiltmc.quilt-loader` | `"quilt"` |
| `net.minecraftforge` | `"forge"` (bare build number; legacy `-` form → `Unsupported("forge-legacy")`) |
| `net.neoforged` | `"neoforge"` (bare version, `-beta`/`-alpha` suffix passed through verbatim) |
| `com.mumfrey.liteloader` | `Unsupported("liteloader")` |
| `net.fabricmc.intermediary`, `org.lwjgl*`, `*.java` | ignored (substrate/runtime) |
| `dependencyOnly: true` | skipped in all cases |

**`ExternalImportPlan`** — output of `plan_external_import`; fields mirror `PrismInstanceCfg` Override gates + resolved name/loader; includes `warnings: Vec<String>`.

**`ExternalImportResult`** (Tauri DTO, `#[derive(specta::Type)]`):
- `slug`, `name`, `loader` (string), `files_copied: u32`, `mods_identified: u32`, `warnings: Vec<String>`
- In `TaskResult::ExternalImport(ExternalImportResult)` union; `bindings.ts` regenerated (`importExternalInstance`, `ExternalImportResult`)

**`LauncherImportError`** enum: `MissingField / MalformedMmcPack / MalformedField / NoGameDir / UnsafePath / Io / Rejected / HttpError(u16) / HttpClientError / IdentifyParseError`

## Tauri command

`import_external_instance(instance_dir, name_override, identify_mods, skip_logs) -> Result<u64, String>` — returns a **task id** (task-queue contract). Job: `ImportExternalJob` (TaskJob); uses stage-and-promote (`remap_to_staging` / `promote_staging`) like all other import jobs. `identify_mods` default OFF.

## Safety helpers

- `icon_key_is_safe(key)` — traversal guard for icon key
- `relative_path_is_safe(path)` / `dest_is_contained(root, dest)` — zip-slip-style guards
- `is_reparse_or_symlink(meta)` — Windows junction/symlink detection (skipped during copy)

## Frontend surface

`NewInstanceModal.tsx` third tab "From launcher":
- Folder picker (native dialog)
- Name override field
- "Skip logs & crash reports" toggle (default ON)
- "Identify mods (Modrinth)" toggle (default OFF)

`Toasts.tsx` surfaces `ExternalImportResult.warnings` as an amber "imported — \<warning\>" toast on task completion.

## Scope / deferred

- **v1 = folder picker only.** CP-8 (auto-detect known launcher dirs) DEFERRED.
- `InstanceType = "Legacy"` (pre-1.6): rejected at job layer via `LauncherImportError::Rejected`; never silently dropped.
- Forge legacy (version contains `-`): `Unsupported("forge-legacy")`, imported as vanilla + warning.

## Tests

82 unit tests in `launcher_import_tests.rs` covering: cfg parse, mmc-pack parse, uid mapping, plan logic, name resolution, Legacy rejection, path-safety helpers, copy behavior, Modrinth identify mock. No live HTTP calls in CI.
