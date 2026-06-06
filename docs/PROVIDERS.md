# Provider notes: CurseForge & Modrinth

Both host Minecraft content, but their APIs and modpack formats differ significantly.
This is the messy-reality reference for the `providers` and `packs` modules.

## Modrinth

- **API:** `https://api.modrinth.com/v2` — open, no key. Set a descriptive
  `User-Agent` (`modloader/<version> (contact)`) per their etiquette, or get rate-limited.
- **IDs:** base62 project ids (e.g. Sodium = `AANobbMI`). Also resolvable by slug.
- **Search:** `/search` with `facets` for loaders, MC versions, project type.
- **Versions:** `/project/{id}/version` → files with **sha1 + sha512**, `dependencies`
  (required/optional with version or project refs), and `game_versions`/`loaders`.
- **Hash lookup:** `/version_file/{hash}` lets us identify an unknown jar by its sha512 —
  great for reconciling manually added mods.
- **Modpack format `.mrpack`:** a zip containing `modrinth.index.json`:
  - `files[]`: each has `path`, `hashes` (sha1/sha512), `downloads[]` (direct URLs),
    `env` (client/server requirement).
  - `dependencies`: `minecraft`, plus `fabric-loader`/`forge`/`quilt-loader`/`neoforge`.
  - `overrides/` (and `client-overrides/`, `server-overrides/`): files copied verbatim
    into the instance after downloads.
  - All downloads are **direct, no auth** → simplest path; resolution is basically
    "parse index → DownloadPlan + copy overrides."

## CurseForge

- **API:** `https://api.curseforge.com` — **requires an API key** in the `x-api-key`
  header. Free from <https://console.curseforge.com>. Minecraft game id = `432`.
- **IDs:** numeric `modId` / `fileId`.
- **Search:** `/v1/mods/search?gameId=432` with `modLoaderType`, `gameVersion`,
  `classId` (modpacks vs mods vs resource packs), sort, pagination.
- **Files:** `/v1/mods/{id}/files` → `downloadUrl`, `fileFingerprint` (Murmur2, **not**
  a normal hash), `hashes[]` (usually sha1), `dependencies[]` (relationType: required=3,
  optional=2, etc.), `gameVersions[]` (mixes MC versions and loader names).
- **⚠️ "download disabled" mods:** some authors set
  `allowModDistribution: false` (or `downloadUrl: null`). We **cannot** fetch those
  programmatically — the UI must detect this and open the project page for a manual
  download, then let the user drop the jar in. This is the single biggest CF gotcha.
- **Fingerprints:** to identify an existing jar, CF uses a Murmur2 hash over the file
  with whitespace bytes (`\t \n \r space`) stripped → `/v1/fingerprints` matches it.
  Needed for "what is this jar" and some pack flows.
- **Modpack format (CF zip):** contains `manifest.json`:
  - `minecraft.version`, `minecraft.modLoaders[]` (e.g. `forge-47.1.0`, primary flag).
  - `files[]`: `{ projectID, fileID, required }` — note: **no direct URL**; we must call
    the API per file to get its `downloadUrl` (and hit the disabled-download case above).
  - `overrides/` folder copied into the instance.
- **Distribution rules:** CF's API terms require honoring the distribution flag and not
  proxying disabled files. Keep that constraint in the resolver.

## Normalized domain types (Rust)

Both map onto shared shapes so the UI and pack resolver don't branch on provider:

```rust
struct ProjectSummary { provider, id, slug, name, summary, downloads, icon_url, categories }
struct Version {
    provider, id, name, version_number,
    game_versions: Vec<String>, loaders: Vec<Loader>,
    files: Vec<VersionFile>, dependencies: Vec<Dependency>,
}
struct VersionFile { url: Option<String>, file_name, size, hashes, primary }
//                        ^ None = download disabled (CF) → manual path
struct ResolvedPack {
    minecraft: String, loader: LoaderSpec,
    downloads: Vec<DownloadItem>,   // → feeds the DownloadPlan
    overrides: Vec<OverrideCopy>,
    manual: Vec<ManualDownload>,    // CF disabled-download items for the UI to surface
}
```

## Practical resolution flow

1. **Modrinth pack:** unzip → parse `modrinth.index.json` → DownloadPlan from `files[]` +
   override copies. Done.
2. **CF pack:** unzip → parse `manifest.json` → for each file, call `/v1/mods/{p}/files/{f}`
   → if `downloadUrl` present, add to plan; else add to `manual[]`. Copy `overrides/`.
3. After downloads: ensure MC version + loader (from pack deps) installed, then it's a
   normal launchable instance.
