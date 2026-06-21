# Research: Do modpack providers expose recommended Java / RAM settings?

Status: research findings + recommendation. No code written.
Date: 2026-06-20. Branch: `ui-overhaul`.

## Question

Our Java settings UI (`src/routes/instance-tabs/JavaTab.tsx:127-128`) tells users:
"Modpacks don't ship recommended Java settings, so these are your own per-instance values."
The user believes this is wrong for CurseForge. This doc verifies the truth per provider and
recommends what to do with our dormant `RecommendedJava` plumbing.

## Per-provider verdict

| Provider | Recommended RAM? | JVM args / Java ver? | Field(s) | Where |
|----------|------------------|----------------------|----------|-------|
| **CurseForge** | **NO** | NO | — | distributed `manifest.json` and Mod/File API objects carry none |
| **Modrinth** | **NO** | NO | — | `modrinth.index.json` and Version API carry none |
| **FTB** | **YES** | NO (RAM only) | `specs.minimum`, `specs.recommended` (RAM in MB) | per-version in `api.modpacks.ch` modpack/version response |

### CurseForge — NO (the user's belief is incorrect)

- Distributed CF modpack zip = `manifest.json` + `overrides/` + `modlist.html`. The
  `manifest.json` schema has only: `minecraft.version`, `minecraft.modLoaders[]`
  (`{id, primary}`), `name`, `version`, `author`, `files[] {projectID, fileID, required}`,
  `overrides`. No memory / JVM / Java field. This matches our own `RawCfManifest` parser
  (`src-tauri/src/core/modpack.rs:291-327`) which already deserializes the full shape.
- CurseForge REST API (`api.curseforge.com`): the **Mod** object (id, name, slug, links,
  summary, categories, classId, authors, logo, mainFileId, latestFiles,
  `allowModDistribution`, downloadCount, …) and the **File** object (id, displayName,
  fileName, hashes, fileLength, downloadUrl, gameVersions, dependencies, fileFingerprint,
  modules, isServerPack, …) contain **no** recommended-memory / JVM-args / Java-version
  field. Verified against the official REST API reference.
- The `minecraftinstance.json` the user may be thinking of is the **CurseForge/Overwolf
  desktop app's local per-instance file** — it is NOT part of an exported/distributed
  modpack zip, and RAM allocation in that app is a launcher-wide setting, not a per-pack
  field. It is not reachable programmatically from the pack or the public API.

### Modrinth — NO

- `.mrpack` `modrinth.index.json` fields: `formatVersion`, `game`, `versionId`, `name`,
  `summary?`, `files[]`, `dependencies` (keys limited to `minecraft`, `forge`, `neoforge`,
  `fabric-loader`, `quilt-loader`). No memory / JVM / Java field.
- Modrinth Version API object (name, version_number, changelog, dependencies, game_versions,
  version_type, loaders, featured, status, id, project_id, author_id, date_published,
  downloads, files) — no memory / JVM / Java field.

### FTB — YES (future provider, for reference)

- The FTB modpacks API (`api.modpacks.ch`) returns, per **version**, a `specs` object:
  `{ id, minimum, recommended }` where `minimum` / `recommended` are **RAM in MB**
  (e.g. `4096` / `6144`). RAM only — no JVM args, no Java version.
- Lives on the per-version payload, so capture would be at the FTB import / install step
  when we add FTB as a provider (not in scope today).

## Impact on our code

- `RecommendedJava { memory_mb: Option<u32>, java_args: Option<String> }`
  (`src-tauri/src/core/instances.rs:56-59`) and `Source.recommended`
  (`instances.rs:70`) are **correctly dormant** for CF + Modrinth — neither provider can
  ever populate them. The doc comment "providers don't expose it yet" is accurate today
  and will stay accurate for our two shipping providers.
- The 3-tier precedence in `java_resolve.rs` (recommended → per-instance → global) already
  reads `inst.source.recommended` first, so if/when FTB lands, `memory_mb` plugs straight
  in with zero resolver change. `java_args` would stay `None` for FTB (FTB has no args field).

## UI copy verdict

The current copy is **misleading, not strictly false**. For CF + Modrinth (our only
providers) it is effectively true — they ship no recommended Java settings. But the blanket
phrasing "Modpacks don't ship recommended Java settings" will be wrong once FTB is added.

Recommended rephrase (provider-accurate, future-proof):
> "When off, this instance uses the global default. CurseForge and Modrinth packs don't
> publish recommended Java settings, so these are your own per-instance values."

This is a one-line text edit in `JavaTab.tsx:127-128`; no logic change.

## Recommendation

1. **Do not** attempt to pull recommended Java/RAM from CurseForge or Modrinth — the fields
   do not exist in either the pack manifest or the public API. The user's premise about CF
   is incorrect.
2. **Edit the UI copy** to name CurseForge + Modrinth specifically rather than "modpacks"
   broadly (see rephrase above). Trivial, do it now.
3. **Keep** the `RecommendedJava` / `Source.recommended` plumbing — it is the correct seam
   for FTB. When FTB is implemented, capture `specs.recommended` → `RecommendedJava.memory_mb`
   at the FTB import/install step (analogous to where `parse_cf_manifest` /
   `import_curseforge_zip` build the `Source`). Leave `java_args = None`.

## Sources

- CurseForge REST API reference — Mod & File object schemas: https://docs.curseforge.com/rest-api/
- Modrinth modpack format (`.mrpack` / `modrinth.index.json`):
  https://support.modrinth.com/en/articles/8802351-modrinth-modpack-format-mrpack
- Modrinth Version API object: https://docs.modrinth.com/api/operations/getversion/
- FTB modpacks API (`specs.minimum` / `specs.recommended`, RAM in MB): https://api.modpacks.ch/public/modpack/91
- Our CF manifest parser (full distributed schema): `src-tauri/src/core/modpack.rs:291-389`
