# Spec: Import instances from other launchers (Prism/MultiMC/PolyMC)

Design: `docs/design/launcher-import.md`. Status: proposed. Build/test ONLY via
`scripts/build.sh`. Rust tests = sibling `<stem>_tests.rs`; pure parse/plan fns unit-tested with
fixtures under `src-tauri/src/core/fixtures/`.

**Goal (current truth):** "Import from launcher" is a real entry point in `NewInstanceModal`. The
user points ApexLauncher at a Prism/MultiMC/PolyMC instance directory; ApexLauncher parses
`instance.cfg` + `mmc-pack.json`, creates a native instance, copies the whole game dir into
`<slug>/mc/`, maps name/loader/MC-version/icon/Java overrides, and (optionally, opt-in) restores
mod provenance via a single keyless batched Modrinth SHA-1 lookup. The result is launchable
(loader auto-installs on first launch).

**New module:** `src-tauri/src/core/launcher_import.rs` (+ sibling `launcher_import_tests.rs`).
**New command:** `import_external_instance` in `lib.rs`. **New job:** `ImportExternalJob`.

---

## Checkpoint table

| CP | Deliverable | Files touched | Fixtures / tests | Bindings regen? | Verify via `scripts/build.sh` |
|----|-------------|---------------|------------------|-----------------|-------------------------------|
| CP-1 | `instance.cfg` parser → `PrismInstanceCfg` | new `core/launcher_import.rs` (+ `mod` decl in `core/mod.rs`); sibling `launcher_import_tests.rs` | fixtures `prism_instance.cfg`, `prism_instance_general_header.cfg`, `prism_instance_legacy.cfg`; tests: parse name/iconKey, gated Java/mem keys, `[General]` header tolerance, `Legacy` rejected, unknown keys ignored | No | `test launcher_import` |
| CP-2 | `mmc-pack.json` parser + uid→loader mapping + version normalization | `core/launcher_import.rs`; `launcher_import_tests.rs` | fixtures `mmc_pack_fabric.json`, `mmc_pack_quilt.json`, `mmc_pack_neoforge.json`, `mmc_pack_forge.json`, `mmc_pack_vanilla.json`, `mmc_pack_liteloader.json`; tests: each uid→`Loader{kind,version}`; vanilla (no loader) → `kind="vanilla",version=None`; `dependencyOnly`/intermediary/lwjgl/java ignored; liteloader → unsupported result; **forge/neoforge version string validated against `core::loaders` expected form** | No | `test launcher_import` |
| CP-3 | Game-dir resolution + safe recursive copy planner | `core/launcher_import.rs`; `launcher_import_tests.rs`; `pub`-export `validate_relative_path`+`is_safe_dest` from `core/modpack.rs` (or local equivalent) | tempdir-built source tree fixtures; tests: resolve `.minecraft` then `minecraft`; copy preserves subtree incl. `mods/`, `config/`, `.disabled` jars; optional skip of `logs/`+`crash-reports/`; path-escape rejected; missing game dir → error | No | `test launcher_import` |
| CP-4 | Icon resolution helper (`iconKey` → central `icons/` file) | `core/launcher_import.rs`; `launcher_import_tests.rs` | tempdir `icons/<key>.png`; tests: custom file found (ext allowlist) → path; built-in name (no file) → `None`; data root inferred from instance dir | No | `test launcher_import` |
| CP-5 | `ImportExternalJob` + `import_external_instance` command | `lib.rs` (job struct + `TaskJob` impl + `#[tauri::command]` + register in `collect_commands!`); `core/launcher_import.rs` (orchestrating plan fn) | `lib_tests.rs`: plan-assembly unit test (no Tauri); job flow asserted via existing TaskJob test harness if present, else manual dev smoke | **YES** — new command + DTOs (`ExternalImportResult`, request fields incl. `identify_mods: bool`, `name_override`, `skip_logs`) | `check` + `test`; then dev smoke (import a real Prism instance, launch it) |
| CP-6 | Opt-in Modrinth mod identification (batched SHA-1) | `core/launcher_import.rs` (identify fn over injectable provider HTTP seam); `lib.rs` (wire `identify_mods` into the job's planning phase); `launcher_import_tests.rs` | mock HTTP client fixture `modrinth_version_files.json`; tests: batched request body shape; matches → `ModEntry` w/ provider/project_id/version_id/hashes; unmatched jars stay folder-only; `identify_mods=false` → zero calls | No (param already shipped in CP-5) | `test launcher_import` (mock; no live) |
| CP-7 | Frontend "Import from launcher" entry | `src/components/NewInstanceModal.tsx` (new tab/button: folder picker, name override, "Identify mods (Modrinth)" checkbox, "Skip logs" toggle); `src/lib/ipc.ts` (`importExternalInstance` `unwrap` wrapper) | tsc only (no FE test infra yet) | No (consumes CP-5 bindings) | `check`; then dev smoke |
| CP-8 | (Stretch) Auto-detect known launcher data dirs | `core/launcher_import.rs` (`scan_known_launchers` → per-OS default paths + portable hints); `lib.rs` (`list_external_instances` command + register); `ipc.ts`; `NewInstanceModal.tsx` (discovered-instances list) | `launcher_import_tests.rs`: per-OS path builder + instances enumeration over a tempdir `instances/` | **YES** — new `list_external_instances` command + `DiscoveredInstance` DTO | `check` + `test`; dev smoke |

Each CP is independently landable: CP-1→CP-4 are pure functions verifiable by `test
launcher_import`; CP-5 wires them into a working command (the first user-visible milestone after
CP-7); CP-6 adds identification; CP-7 is the UI; CP-8 is an additive convenience.

---

## Data shapes (CP-1, CP-2, CP-5)

```rust
// CP-1
pub struct PrismInstanceCfg {
    pub name: Option<String>,
    pub icon_key: Option<String>,
    pub instance_type: Option<String>,   // "OneSix" expected; "Legacy" -> reject
    pub override_memory: bool,
    pub min_mem_mb: Option<u32>,         // MinMemAlloc
    pub max_mem_mb: Option<u32>,         // MaxMemAlloc
    pub override_java_location: bool,
    pub java_path: Option<String>,
    pub override_java_args: bool,
    pub jvm_args: Option<String>,
}

// CP-2
pub enum ImportedLoader { Vanilla, Loader { kind: String, version: String }, Unsupported(String) }
pub struct MmcPack { pub minecraft: String, pub loader: ImportedLoader }

// CP-5 (DTO; drives bindings regen)
pub struct ExternalImportResult {
    pub slug: String,
    pub name: String,
    pub loader: String,            // "vanilla"|"fabric"|...
    pub files_copied: u32,
    pub mods_identified: u32,
    pub warnings: Vec<String>,     // non-fatal notices (e.g. unsupported loader → vanilla)
}
// import_external_instance(instance_dir: String, name_override: Option<String>,
//                          identify_mods: bool, skip_logs: bool) -> Result<u64, String>
```

`import_external_instance` returns a **task id** (`Promise<number>` on the FE), matching the
task-queue contract of every other import command; the terminal `ExternalImportResult` arrives via
the `task://update` event (`status.kind === "done"`, `task.result` set) and is surfaced by the
AppShell store subscriber.

---

## Field mapping (the contract a builder follows verbatim)

Prism instance dir `D`:
1. Parse `D/instance.cfg` → `PrismInstanceCfg`. If `instance_type == "Legacy"` → fail
   ("legacy MultiMC instances unsupported").
2. Parse `D/mmc-pack.json` → `MmcPack`. `net.minecraft.version` → `minecraft` (required; fail if
   absent). Loader uid (table in design §2.2) → `ImportedLoader`. `Unsupported(x)` →
   import as vanilla **with a warning recorded in the result** (do not silently mislabel).
3. `instances::create(app, CreateInstanceReq { name: name_override ?? cfg.name ?? "Imported",
   minecraft, loader: Loader { kind, version } })`.
4. Resolve game dir: `D/.minecraft` else `D/minecraft` (fail if neither).
5. Copy game dir tree → `staging_dir`; on each file enforce path safety; optionally skip
   `logs/`, `crash-reports/`. Then `promote_staging(staging_dir, <slug>/mc)`; remove staging.
6. Java/memory: when `override_memory` → `java.memory_mb = max_mem_mb`, `java.min_memory_mb =
   min_mem_mb`, `java.use_pack_settings = true`. When `override_java_location` → `java.path_override
   = java_path`. When `override_java_args` → `java.args_override = jvm_args`.
7. Icon: if `icon_key` resolves to a file under `<D>/../../icons/<key>.<ext>` (ext allowlist) →
   `write_instance_icon`; else leave `None`.
8. `source = None`, `pack_locked = false`. `save_manifest`.
9. (CP-6) if `identify_mods`: SHA-1 every jar in `mc/mods/`, one batched
   `POST /v2/version_files`, emit `ModEntry`s for matches; `save_manifest`.
10. Result: `ExternalImportResult { slug, name, loader, files_copied, mods_identified }`.

---

## Resolved decisions (locked before execution — 2026-06-27)

- **Forge version string** ✅ RESOLVED FROM CODE: `forge_installer.rs` composes the installer
  artifact/Maven coord as `forge-{mc_version}-{loader_version}` from a stored **bare build number**
  (`loader_version` = e.g. `47.2.0`). Prism's `net.minecraftforge` component `version` is ALSO the
  bare build number → map **directly, no normalization** (NeoForge `21.1.209` likewise maps
  direct). CP-2 just copies the component version through. Known edge: ancient Forge (1.7.10) uses
  a doubled `mc-build-mc` form — flag/skip-with-warning if encountered (not a v1 target).
- **Mod identification** ✅ opt-in checkbox, **default OFF** (honors api-frugality; offline by
  default; CP-6 runs the batched keyless Modrinth SHA-1 lookup only when ticked).
- **CP-8 auto-detect** ✅ **DEFER to a follow-up** — v1 ships folder-picker only (works everywhere
  incl. portable MultiMC). CP-8 is NOT in this release's scope.
- **LiteLoader** ✅ import-as-vanilla + warning (keep worlds/configs; no loader).
- **`logs/`/`crash-reports/`** ✅ skip by default, with a toggle to include.

---

## Change log

- 2026-06-27 — Initial spec drafted (ax-plan). v1 = Prism/MultiMC/PolyMC; vanilla deferred;
  opaque-import default with opt-in keyless Modrinth SHA-1 identify; copy (not hardlink); folder
  picker primary + auto-detect stretch (CP-8). No checkpoints implemented yet.
- 2026-06-27 — Decisions locked: Forge version maps direct (bare build number — resolved from
  forge_installer.rs); mod-identify opt-in default OFF; CP-8 auto-detect DEFERRED to follow-up
  (v1 = picker only); LiteLoader → vanilla+warn; logs/crash-reports skip-by-default w/ toggle.
  Cleared for implementation (CP-1…CP-7; CP-8 out of scope).
- 2026-06-27 — CP-1…CP-5 implemented + merged-ready. CP-5 resolved the spec's
  warnings-self-contradiction: `ExternalImportResult` gains `warnings: Vec<String>`
  (threaded from `plan_external_import`) so an Unsupported-loader→vanilla demotion is
  surfaced to the UI, not only `log::warn!`'d. Builder reorder: `resolve_game_dir` runs
  before `instances::create` (avoids an orphan instance when no game dir exists) — kept.
  Followup 002 filed: shared post-create cleanup so a mid-import copy/promote failure does
  not leave an orphan instance dir (pre-existing across all import jobs).
