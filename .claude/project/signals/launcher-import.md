# launcher-import

## Overview

Import existing Prism Launcher / MultiMC / PolyMC instances into ApexLauncher. All parse/plan functions in `launcher_import.rs` are pure (no I/O except `copy_game_dir` and `resolve_icon_path`). Orchestration (stage+promote, Tauri job) lives in `lib.rs`. CP-8 (auto-detect launcher dirs) deferred; v1 is folder-picker only.

## Key files

| File | Role |
|------|------|
| `src-tauri/src/core/launcher_import.rs` | Pure parse/plan module |
| `src-tauri/src/core/launcher_import_tests.rs` | 82 unit tests |
| `src-tauri/src/lib.rs` | `ImportExternalJob` (TaskJob) + `import_external_instance` command + `enqueue_import_external` + `ExternalImportResult` DTO |
| `src/components/NewInstanceModal.tsx` | "From launcher" third tab |
| `src/lib/ipc.ts` | `importExternalInstance` wrapper |
| `src/components/Toasts.tsx` | Amber toast for `ExternalImportResult.warnings` |

## Parse/plan functions (all pure)

| Function | Signature | Notes |
|----------|-----------|-------|
| `parse_instance_cfg(text)` | `&str → Result<PrismInstanceCfg>` | flat `key=value` INI; tolerates `[General]` header; unknown keys ignored |
| `parse_mmc_pack(text)` | `&str → Result<MmcPack>` | parses `mmc-pack.json`; uid→loader map; `dependencyOnly` skipped |
| `resolve_game_dir(dir)` | `&Path → Option<PathBuf>` | checks `.minecraft/` first, then `minecraft/` |
| `copy_game_dir(src, dest, skip_logs)` | `(&Path, &Path, bool) → Result<u32>` | recursive; path-safe; skips `logs/`+`crash-reports/` when flag set; Windows junction/symlink skip; returns file count |
| `resolve_icon_path(dir, key)` | `(&Path, &str) → Option<PathBuf>` | filesystem stat; traversal-guarded via `icon_key_is_safe` |
| `plan_external_import(cfg, pack, name_override)` | `(&PrismInstanceCfg, &MmcPack, Option<&str>) → Result<ExternalImportPlan>` | name = name_override[non-empty] ?? cfg.name ?? "Imported"; accumulates `warnings` |
| `identify_mods_modrinth(client, mods_dir)` | `(&dyn ProviderHttpClient, &Path) → Result<Vec<ModEntry>>` | opt-in (CP-6); SHA-1 hashes `*.jar`/`*.jar.disabled`; ONE batched `POST /v2/version_files`; empty dir → zero HTTP calls; `name/icon_url/summary = None` (api-frugality) |

## Key types

**`PrismInstanceCfg`** (from `instance.cfg`): `name`, `icon_key`, `instance_type`, `override_memory`, `min_mem_mb`, `max_mem_mb`, `override_java_location`, `java_path`, `override_java_args`, `jvm_args`

**`MmcPack`**: `minecraft: String`, `loader: ImportedLoader`

**`ImportedLoader`**: `Vanilla`, `Loader { kind, version }` (fabric/quilt/forge/neoforge), `Unsupported(String)` (unrecognized uid → import as vanilla + warning)

**uid → loader mapping:**

| uid | loader |
|-----|--------|
| `net.fabricmc.fabric-loader` | `"fabric"` |
| `org.quiltmc.quilt-loader` | `"quilt"` |
| `net.minecraftforge` | `"forge"` (bare build; version contains `-` → `Unsupported("forge-legacy")`) |
| `net.neoforged` | `"neoforge"` |
| `com.mumfrey.liteloader` | `Unsupported("liteloader")` |
| `net.fabricmc.intermediary`, `org.lwjgl*`, `*.java` | ignored (substrate/runtime) |
| `dependencyOnly: true` | skipped |

**`ExternalImportResult`** (Tauri DTO, `#[derive(specta::Type)]`): `slug`, `name`, `loader`, `files_copied: u32`, `mods_identified: u32`, `warnings: Vec<String>`; in `TaskResult::ExternalImport` union.

**`LauncherImportError`**: `MissingField / MalformedMmcPack / MalformedField / NoGameDir / UnsafePath / Io / Rejected / HttpError(u16) / HttpClientError / IdentifyParseError`

## Tauri command

`import_external_instance(instance_dir, name_override, identify_mods, skip_logs) -> Result<u64, String>` — returns task id. `ImportExternalJob` uses stage-and-promote. `identify_mods` default OFF.

## Frontend surface

`NewInstanceModal.tsx` "From launcher" tab: folder picker, name override field, "Skip logs & crash reports" toggle (default ON), "Identify mods (Modrinth)" toggle (default OFF).

`Toasts.tsx` surfaces `ExternalImportResult.warnings` as amber toast on task completion.

## Safety helpers

- `icon_key_is_safe(key)` — traversal guard for icon key
- `relative_path_is_safe(path)` / `dest_is_contained(root, dest)` — zip-slip-style guards
- `is_reparse_or_symlink(meta)` — Windows junction/symlink detection (skipped during copy)

## Scope / deferred

- CP-8 (auto-detect known launcher dirs) deferred; v1 = folder picker only.
- `InstanceType = "Legacy"` (pre-1.6): rejected at job layer via `LauncherImportError::Rejected`.
- Forge legacy (version contains `-`): `Unsupported("forge-legacy")`, imported as vanilla + warning.

## Tests

82 unit tests in `launcher_import_tests.rs`: cfg parse, mmc-pack parse, uid mapping, plan logic, name resolution, Legacy rejection, path-safety helpers, copy behavior, Modrinth identify mock. No live HTTP in CI.
