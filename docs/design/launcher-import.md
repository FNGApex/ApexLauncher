# Design: Import instances from other launchers

Status: proposed (Phase 7 polish). Author: ax-plan. Date: 2026-06-27.

Let a user point ApexLauncher at an existing Minecraft instance created by **another launcher**
and import it as a native ApexLauncher instance — preserving MC version, mod loader + loader
version, mods, the whole game dir (configs/overrides/resourcepacks/saves/…), instance name, and
icon. End state: **"Import from launcher"** becomes a real entry point in `NewInstanceModal`
alongside the existing modpack import, producing a launchable ApexLauncher instance.

---

## 1. Scope (v1)

**v1 supports the Prism Launcher / MultiMC / PolyMC family only** — they share one on-disk
format (PolyMC and Prism are forks of MultiMC; the instance layout, `instance.cfg`, and
`mmc-pack.json` are identical across all three). This is the single highest-value target: it is
the dominant power-user launcher lineage and its format is fully self-describing on disk.

**Vanilla official launcher import is deferred** (see §7). Its profiles point at a *shared*
`.minecraft` rather than per-instance dirs, modded profiles reference loader-injected version ids
that we'd have to reverse-map, and there is no per-profile mod isolation — it maps far less
cleanly than Prism and is lower value. Documented here as a non-goal for v1.

---

## 2. Format findings (Prism / MultiMC / PolyMC) — primary sources

### 2.1 Instance directory layout

```
<dataroot>/instances/<InstanceName>/
  instance.cfg        ← INI-ish key=value: name, iconKey, java/memory overrides, notes
  mmc-pack.json       ← component list (MC version + loader + loader version)
  .minecraft/         ← the game dir (mods/, config/, resourcepacks/, saves/, options.txt, …)
  mmc-instance.json   ← (optional, newer) extra metadata; not needed for import
```

- **Game dir name:** Prism uses `.minecraft/` by default; some older MultiMC instances use
  `minecraft/` (no dot). The importer must probe both (`.minecraft` first, then `minecraft`).
  Source: Prism wiki "Data Locations" + Rubenerd "Where Prism stores the Minecraft folder".
- `instance.cfg` is a flat `key=value` file (newer files may carry a leading `[General]` section
  header). Parse line-oriented, tolerate an optional section header, ignore unknown keys.

### 2.2 `mmc-pack.json` — component list and the uid → loader mapping

`mmc-pack.json` is `{ "formatVersion": 1, "components": [ … ] }`. Each component:

```json
{
  "uid": "net.neoforged",
  "version": "21.1.209",
  "cachedName": "NeoForge",
  "cachedVersion": "21.1.209",
  "cachedRequires": [ { "uid": "net.minecraft", "equals": "1.21.1" } ],
  "important": true
}
```

Fields used: **`uid`** (identifies the component), **`version`** (the authoritative version
string — `cachedVersion` is only a display cache). Components may also carry
`dependencyOnly: true` (auto-pulled deps like the Fabric intermediary — skip these).

**uid → ApexLauncher loader mapping (authoritative — from the Prism `meta-launcher/index.json`):**

| `mmc-pack` uid | meta name | ApexLauncher `loader.kind` | `loader.version` source |
|----------------|-----------|----------------------------|-------------------------|
| `net.minecraft` | Minecraft | (sets `Instance.minecraft`, not a loader) | `version` → `Instance.minecraft` |
| `net.fabricmc.fabric-loader` | Fabric Loader | `"fabric"` | component `version` |
| `org.quiltmc.quilt-loader` | Quilt Loader | `"quilt"` | component `version` |
| `net.minecraftforge` | Forge | `"forge"` | component `version` (Forge build, e.g. `47.2.0`) |
| `net.neoforged` | NeoForge | `"neoforge"` | component `version` (e.g. `21.1.209`) |
| `net.fabricmc.intermediary` | Intermediary Mappings | *(ignored — dependencyOnly)* | — |
| `org.lwjgl`, `org.lwjgl3` | LWJGL 2/3 | *(ignored — vanilla substrate)* | — |
| `net.adoptium.java`, `net.minecraft.java`, `com.azul.java`, `com.ibm.java` | Java Runtimes | *(ignored — ApexLauncher manages Java)* | — |
| `com.mumfrey.liteloader` | LiteLoader | **unsupported** → import as vanilla w/ warning, or reject | — |

If **no** loader component is present (only `net.minecraft` + lwjgl/java), the instance is
**vanilla** → `loader.kind = "vanilla"`, `loader.version = None`.

**Forge/NeoForge version-string caveat (flagged, §6/CP-2):** Prism stores the Forge build number
*without* the MC prefix (`47.2.0`, not `1.20.1-47.2.0`). ApexLauncher's launch-time Forge
installer (`forge_installer::run_installer(installer_kind, loader_version, minecraft, …)`) takes
`loader_version` + `minecraft` separately, so the bare build number should match — but this must
be validated against `core/loaders.rs`'s expected form before shipping (CP-2 includes a check).

### 2.3 `instance.cfg` keys (from Prism `MinecraftInstance.cpp` + base instance)

| `instance.cfg` key | Meaning | Maps to |
|--------------------|---------|---------|
| `name` | Display name | `Instance.name` (slug auto-derived by `instances::create`) |
| `iconKey` | Icon identifier (builtin theme name OR custom file stem) | `Instance.icon` (custom only — see §2.4) |
| `notes` | Free text | *(dropped in v1)* |
| `OverrideMemory` | `true`/`false` gate for memory keys | gate for the next two |
| `MinMemAlloc` | `-Xms` MiB | `JavaCfg.min_memory_mb` |
| `MaxMemAlloc` | `-Xmx` MiB | `JavaCfg.memory_mb` |
| `OverrideJavaLocation` | gate | gate for `JavaPath` |
| `JavaPath` | absolute path to a `java`/`javaw` binary | `JavaCfg.path_override` |
| `OverrideJavaArgs` | gate | gate for `JvmArgs` |
| `JvmArgs` | extra JVM args | `JavaCfg.args_override` |
| `InstanceType` | `OneSix` (modern) / `Legacy` | sanity check; `Legacy` = pre-1.6, reject in v1 |

Keys present but ignored in v1: window size, native workarounds, perf toggles (FeralGamemode,
MangoHud, Zink), env, account binding, server-autojoin.

### 2.4 Icons

Prism `iconKey` is **not** a path. It is either (a) a built-in theme icon name (e.g. `default`,
`flame`, `chicken`) or (b) the stem of a user-supplied custom icon file stored in the launcher's
**central** `<dataroot>/icons/` folder (e.g. `iconKey=mypack` → `<dataroot>/icons/mypack.png`).
Icons are *not* stored inside the instance dir. To import a custom icon we resolve
`<dataroot>/icons/<iconKey>.<ext>` for ext in our allowlist `{png,jpg,jpeg,webp,gif}`; if found,
copy via `instances::write_instance_icon`. If the key is a built-in name (no matching file) →
leave `Instance.icon = None` (placeholder). `<dataroot>` is inferred as
`<instance_dir>/../../` (instance dir → `instances/` → data root), so a folder-picked instance
still finds the sibling `icons/` folder.

### 2.5 Default data-dir locations (auto-detect feasibility — §6/CP-8)

| Launcher | Windows | macOS | Linux |
|----------|---------|-------|-------|
| Prism | `%APPDATA%\PrismLauncher` | `~/Library/Application Support/PrismLauncher` | `~/.local/share/PrismLauncher` |
| PolyMC | `%APPDATA%\PolyMC` | `~/Library/Application Support/PolyMC` | `~/.local/share/PolyMC` |
| MultiMC | (portable — install dir) | (app bundle dir) | `~/.local/share/multimc` or install dir |

Instances live under `<dataroot>/instances/`; icons under `<dataroot>/icons/`. MultiMC is
commonly portable (data next to the executable), so auto-detect is best-effort; the **folder
picker is the reliable primary path** and auto-detect is an additive convenience (CP-8).

---

## 3. Mapping to ApexLauncher's model

ApexLauncher instance (`src-tauri/src/core/instances.rs`): game files live under
`<dataroot>/instances/<slug>/mc/` (mods at `mc/mods/`), manifest is `instance.json`. Loaders are
**not installed at import** — `instances::create` only records `Loader { kind, version }`; first
`launch_instance` resolves/installs the loader automatically (Fabric/Quilt via
`loader_profile::fetch_profile`; Forge/NeoForge via the headless `forge_installer`). So a correct
`loader.kind` + `loader.version` is all an imported instance needs to launch.

**Field-by-field map for a Prism instance → ApexLauncher `Instance`:**

| Prism source | ApexLauncher field | Notes |
|--------------|--------------------|-------|
| `instance.cfg` `name` | `Instance.name` | slug auto-generated (`unique_slug`) |
| `mmc-pack.json` `net.minecraft.version` | `Instance.minecraft` | required; abort if missing |
| loader uid (table §2.2) | `Instance.loader.kind` | `"vanilla"` if no loader component |
| loader component `version` | `Instance.loader.version` | `None` for vanilla |
| `<game-dir>/` whole tree (copied) | `<slug>/mc/` | recursive copy (§4) |
| `<game-dir>/mods/*.jar(.disabled)` | appear via `scan_mods` automatically | folder scan is authoritative for display + enable state |
| (optional) Modrinth sha1 match | `Instance.mods[]` `ModEntry` | only for identified mods (§5) |
| `iconKey` custom file | `Instance.icon` | via `write_instance_icon`; else `None` |
| `OverrideMemory`+`Max/MinMemAlloc` | `JavaCfg.memory_mb` / `min_memory_mb` | only when `OverrideMemory=true`; set `use_pack_settings=true` |
| `OverrideJavaLocation`+`JavaPath` | `JavaCfg.path_override` | only when override true |
| `OverrideJavaArgs`+`JvmArgs` | `JavaCfg.args_override` | only when override true |
| — | `Instance.source` | **`None`** (no provider pack behind an external import) |
| — | `Instance.pack_locked` | **`false`** (user fully manages an imported instance) |

Because `scan_mods` lists the real jars in `mc/mods/` regardless of any `ModEntry`, **copying the
jars is sufficient for them to display, toggle (`.disabled`), and launch.** `ModEntry`s are only
needed to light up update-checking, which is exactly what mod identification (§5) restores.

---

## 4. Reusing the existing import infrastructure

Mirror the modpack import jobs (`ImportFtbJob`/`ImportAtlJob` in `lib.rs`):

- **TaskJob lifecycle:** new `ImportExternalJob` implements `TaskJob` — `enter_planning` (parse
  `instance.cfg` + `mmc-pack.json`, resolve game dir, build copy plan, optional Modrinth
  identify), then **no download phase** (all source files are local), `enter_applying` (copy →
  promote), `finish_done_with_result`.
- **Staging/promote:** copy the source game dir into `staging_dir = <inst_dir>/.staging-<task_id>/`
  then `promote_staging(staging_dir, mc_dir)`. On cancel/fail `remove_dir_all(staging_dir)`. This
  reuses the exact crash-safety pattern of every other import; `instances::create` makes the
  empty `mc/` skeleton first.
- **Safety gate:** the recursive copy must reject path escapes. `modpack.rs`'s
  `validate_relative_path` + `is_safe_dest` are the existing gate but are private — CP-3 either
  `pub`-exports them or adds an equivalent local guard in the new module.
- **Instance creation + manifest:** `instances::create(app, CreateInstanceReq{name, minecraft,
  loader})` then `load_manifest`/`save_manifest` to write mods/icon/java. `write_instance_icon`
  for the icon.
- **Command + bindings:** add one `#[tauri::command] #[specta::specta]` fn
  `import_external_instance(...) -> Result<u64, String>` (returns a task id like the other import
  commands), register it in the single `collect_commands!` list in `make_builder()`, regenerate
  `bindings.ts`, add an `unwrap`-wrapped wrapper in `ipc.ts`. Frontend entry: a new tab/button in
  `NewInstanceModal.tsx` using `open({ directory: true })` from `@tauri-apps/plugin-dialog`.

---

## 5. Mod provenance decision

Mods in a Prism instance are **opaque jars** in `mods/` — Prism does not reliably store
CurseForge/Modrinth project ids (it keeps a `.metadata/` index only for instances it manages, not
for hand-added jars). Two options:

- **(a) Opaque import** — copy jars; create no `ModEntry`. They display via `scan_mods`, toggle,
  and launch, but get no update-checking.
- **(b) Re-identify via Modrinth hash lookup** — `POST /v2/version_files` with the jars' SHA-1
  hashes (one **batched** call, **keyless** — Modrinth needs no API key) returns a map of
  hash → version (with `project_id`), letting us synthesize provenance-bearing `ModEntry`s so the
  existing update-check lights up.

**Decision: do both, gated.** Default = (a) opaque (zero network, honors the api-frugality
standing rule). Expose an **opt-in "Identify mods (Modrinth)" checkbox** in the import dialog;
when checked, run a **single batched** `/version_files` SHA-1 lookup during planning and emit
`ModEntry`s for matches only (unmatched jars stay folder-only). Rationale: a one-time, batched,
keyless lookup at explicit user intent is a justified cost; making it opt-in keeps the default
path fully offline and respects api-frugality. **CurseForge fingerprint identification is out of
scope for v1** (needs the CF key + a Murmur2 fingerprint, not a SHA-1) — note as a future add.

Evidence: Modrinth `POST /version_files` (algorithm `sha1`|`sha512`, hashes array → map of
hash→version w/ `project_id`) is publicly accessible without auth (Modrinth API docs).

---

## 6. Decisions summary (for human approval)

1. **v1 scope:** Prism/MultiMC/PolyMC only. Vanilla launcher deferred.
2. **Mod provenance:** opaque by default; opt-in batched keyless Modrinth SHA-1 identify. No CF
   fingerprint in v1.
3. **Copy, not hardlink** the game dir — a one-time migration; isolates the new instance from the
   source launcher (no cross-launcher mutation). Optionally skip `logs/` + `crash-reports/`.
4. **Locate:** folder picker (user points at the instance dir) is the v1 primary path;
   auto-scan of default data dirs is an additive CP-8 convenience.
5. **Job shape:** new `ImportExternalJob` TaskJob + `import_external_instance` command mirroring
   the modpack import jobs; returns a task id; result `ExternalImportResult`.
6. **Icon:** resolve `iconKey` against the central `<dataroot>/icons/` folder → `write_instance_icon`;
   built-in/non-file key → placeholder.
7. **Java/memory:** map Prism overrides into `JavaCfg` (`memory_mb`/`min_memory_mb`/`path_override`/
   `args_override`, `use_pack_settings=true`) only when the corresponding `Override*` gate is true;
   else leave defaults.

---

## 7. Rejected / deferred approaches

- **Vanilla official launcher in v1** — shared `.minecraft`, loader-injected version ids, no
  per-profile mod isolation; messy mapping, low value. Deferred; revisit as a separate effort.
- **Hardlinking the source game dir** — risks the source launcher mutating ApexLauncher's
  instance (and vice-versa); imports should be a clean migration. Rejected in favor of copy.
- **CurseForge fingerprint identification in v1** — requires the CF key + Murmur2 fingerprint
  algorithm; deferred. Modrinth SHA-1 covers the keyless, frugal case.
- **Re-downloading mods from providers instead of copying jars** — wasteful, would fail for
  manual-only CF mods, and discards local config/state. Rejected; copy the on-disk files.
- **Auto-detect as the only locate mechanism** — MultiMC is frequently portable and Prism's data
  dir is relocatable, so detection is unreliable; the folder picker is the dependable primary.

---

## Sources

- Prism instance/component system + data locations: Prism Launcher Wiki — Instance Management,
  Version page, Data Locations (`prismlauncher.org/wiki`).
- Authoritative uid list: Prism `meta-launcher/index.json`
  (`github.com/PrismLauncher/meta-launcher`).
- `instance.cfg` keys: Prism `launcher/minecraft/MinecraftInstance.cpp`
  (`github.com/PrismLauncher/PrismLauncher`); MultiMC wiki "Instance settings".
- `mmc-pack.json` component example (NeoForge/Minecraft): PrismLauncher issue #4687.
- Game-dir name: Rubenerd "Where Prism stores the Minecraft folder".
- Modrinth hash lookup: Modrinth API docs — `POST /v2/version_files`.
