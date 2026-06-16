# metadata

## What it does

Fetches and disk-caches Minecraft version lists (Mojang piston-meta), per-version mod-loader build lists (Forge, Fabric, Quilt, NeoForge), and Fabric/Quilt loader profile JSONs (mainClass + libraries + args). Also parses Forge/NeoForge `version.json` files from disk (produced by the headless installer) via `load_forge_profile`. All network responses are cached in `<data>/meta-cache/` with a 6-hour TTL. Network failures per loader are swallowed ("not available for this MC") rather than failing the whole request.

## CLI code

- `src-tauri/src/core/versions.rs` — `McVersion` struct; `list_releases` fetches `piston-meta.mojang.com/mc/game/version_manifest_v2.json`, filters to `type=release`, stops at floor `1.7.10`; TTL 6h
- `src-tauri/src/core/loaders.rs` — `LoaderOption` struct (`kind`, `versions`); `for_mc` returns vanilla + whichever of Forge/Fabric/Quilt/NeoForge have builds for that MC; per-loader fetch helpers:
  - Forge: `maven-metadata.xml` at `maven.minecraftforge.net`; strips `{mc}-` prefix; handles 1.7.10 doubled-suffix quirk
  - Fabric/Quilt: JSON API at `meta.fabricmc.net/v2/versions/loader/{mc}` / `meta.quiltmc.org/v3/versions/loader/{mc}`; already newest-first
  - NeoForge: `maven.neoforged.net` XML; version encodes MC (e.g. `20.4.237` → 1.20.4); legacy artifact for 1.20.1 at `net/neoforged/forge`
- `src-tauri/src/core/meta.rs` — `cached_text(app, url, key, ttl)`: checks `meta-cache/<key>` mtime vs TTL, GETs if stale, writes back on success (cache write is best-effort); builds one `reqwest::Client` per call (not shared)
- `src-tauri/src/core/loader_profile.rs` — `LoaderProfile` / `LoaderLibrary` structs (`url: Option<String>` — `None` for Forge processor-produced libs with no download URL; Fabric/Quilt profiles always supply a URL); `inherits_from: Option<String>` field (present in Forge/NeoForge `version.json`, absent in Fabric/Quilt); `fetch_profile(app, kind, mc, loader_version)` fetches and caches Fabric/Quilt launcher-profile JSON (Forge/NeoForge not fetched via HTTP — loaded from disk); `profile_url(kind, mc, loader)` pure URL builder (fabric|quilt only; returns `Err` for "forge"/"neoforge"); `load_forge_profile(path)` reads and parses a Forge/NeoForge `version.json` from disk, mapping `downloads.artifact.url` → `LoaderLibrary.url` (absent artifact → `url=None`); `maven_coord_to_path(coord)` converts Maven coordinate (3 or 4 segments) to relative Maven repo path; `ForgeVersionJson` / `ForgeLibrary` / `ForgeArtifact` / `ForgeLibraryDownloads` internal structs for Forge-format parse; cache key `<kind>-profile-<sanitized-mc>-<sanitized-loader>.json`; 233 lines (down from 447 — tests relocated); 20 unit tests live in sibling `src-tauri/src/core/loader_profile_tests.rs` (253 lines), wired via `#[cfg(test)] #[path = "loader_profile_tests.rs"] mod tests;` stub at the end of the file; `fixtures/fabric_profile.json` + `fixtures/neoforge_profile.json`
- `src-tauri/src/lib.rs` — Tauri commands: `list_minecraft_versions` (async), `get_loaders` (async, takes `minecraft: String`)

## Artifacts

- `src/components/NewInstanceModal.tsx` — consumes `listMinecraftVersions` + `getLoaders`; query keys `["mc-versions"]` and `["loaders", minecraft]`; staleTime mirrors Rust TTL (6h via `META_STALE_TIME`)
- `src/lib/prefetch.ts` — `prefetchStartupData`: pre-warms `["mc-versions"]` and `["loaders", latest]` at app start; failures swallowed

## Docs

- `docs/ARCHITECTURE.md` §2 — `meta-cache/` location
- `docs/PROVIDERS.md` — CurseForge and Modrinth API details (relevant to future Phase 5 extension of this layer)
- `docs/ROADMAP.md` Phase 2/4 note — version/loader metadata pulled forward early
- `docs/spec/fabric-quilt-launch.md` — spec for Phase 4 slice A; `loader_profile.rs` implements CP1 (fetch + cache) and the `LoaderProfile`/`LoaderLibrary` types
- `docs/design/fabric-quilt-launch.md` — design rationale for the profile-overlay approach
- `docs/spec/neoforge-forge-launch.md` — Phase 4 slice B spec; `load_forge_profile` and the Forge-format `LoaderLibrary` structs are defined here
- `docs/design/neoforge-forge-launch.md` — design rationale for headless installer; explains Forge `downloads.artifact.url` vs Fabric flat-url format

## Coupling

- `NewInstanceModal` (instances domain) reads from this domain's query cache; `McVersion`/`LoaderOption` IPC type changes require updating `src/lib/ipc.ts` and the modal.
- `src/lib/query.ts` exports `META_STALE_TIME = 6h` used in both `NewInstanceModal` and `prefetch.ts`; changing the Rust TTL should be mirrored here.
- `meta.rs` builds a new `reqwest::Client` per `cached_text` call — no shared client.
- `loader_profile.rs` imports `Arguments` / `ArgumentEntry` from `resolver.rs` — a change to those types requires updating both modules.
- **launch domain:** `lib.rs::launch_instance` calls `loader_profile::fetch_profile` (fabric/quilt) or `loader_profile::load_forge_profile` (forge/neoforge); `LoaderProfile` is consumed by the resolver domain via `merge_loader_profile`.
- **forge_installer (launch domain):** `forge_installer.rs` imports `loader_profile::maven_coord_to_path` to build installer Maven URLs; changes to `maven_coord_to_path` affect installer URL construction.

## Conventions worth knowing

- NeoForge version-to-MC derivation: 3-part version `A.B.C` → `1.A.B`; 4-part `A.B.C.D` → `A.B.C`. The `neoforge_matches` fn handles both plus `.0` suffix stripping.
- `sort_versions_desc` splits on `.` and `-`, parses each segment as u64 (non-numeric → 0), compares lexicographically by segment tuple.
- Cache key for Fabric/Quilt loader list is `fabric-loader-{mc}.json` / `quilt-loader-{mc}.json`; loader profile cache key is `<kind>-profile-<mc>-<loader>.json`; both sanitize the version string (non-alphanumeric non-`.` → `_`).
- Loaders with zero builds for a given MC are excluded from the result (not returned as an empty entry).
- `maven_coord_to_path` supports 3-segment (`group:artifact:version`) and 4-segment (`group:artifact:version:classifier`) Maven coordinates; group dots become path separators.
- `LoaderLibrary.url` is `Option<String>`: `None` for Forge processor-produced libs (no artifact block); `Some("")` can also occur and is treated as no-download in `merge_loader_profile`.
- `profile_url` returns `Err` for "forge"/"neoforge" — those profiles are loaded from disk via `load_forge_profile`, not fetched over HTTP.
