# metadata

## Overview

Fetches and disk-caches MC version lists (Mojang piston-meta), per-version loader build lists (Forge/Fabric/Quilt/NeoForge), and Fabric/Quilt loader profile JSONs. Parses Forge/NeoForge `version.json` from disk via `load_forge_profile`. All network responses cached in `<data>/meta-cache/` with a 6-hour TTL. Per-loader fetch failures swallowed (returns empty list for that loader) rather than failing the whole request.

## CLI code

- `src-tauri/src/core/versions.rs` — `McVersion`; `list_releases` fetches `piston-meta.mojang.com/mc/game/version_manifest_v2.json`, filters to `type=release`, floor `1.7.10`; TTL 6h
- `src-tauri/src/core/loaders.rs` — `LoaderOption { kind, versions }`; `for_mc` returns vanilla + whichever of Forge/Fabric/Quilt/NeoForge have builds; per-loader fetch helpers:
  - Forge: `maven-metadata.xml` at `maven.minecraftforge.net`; strips `{mc}-` prefix; handles 1.7.10 doubled-suffix quirk
  - Fabric/Quilt: JSON API at `meta.fabricmc.net/v2/versions/loader/{mc}` / `meta.quiltmc.org/v3/versions/loader/{mc}`
  - NeoForge: `maven.neoforged.net` XML; version encodes MC (e.g. `20.4.237` → 1.20.4); legacy artifact for 1.20.1
- `src-tauri/src/core/meta.rs` — `cached_text(app, url, key, ttl)`: checks `meta-cache/<key>` mtime vs TTL, GETs if stale, writes back on success (best-effort); one `reqwest::Client` per call (not shared)
- `src-tauri/src/core/loader_profile.rs` — `LoaderProfile` / `LoaderLibrary { url: Option<String> }` (`None` for Forge processor-produced libs); `inherits_from: Option<String>`; `fetch_profile` (Fabric/Quilt only, cached); `load_forge_profile(path)` reads Forge/NeoForge `version.json` from disk; `maven_coord_to_path(coord)` converts Maven coordinate to relative path; `profile_url` returns `Err` for forge/neoforge; 20 tests in `loader_profile_tests.rs`
- `src-tauri/src/lib.rs` — `list_minecraft_versions`, `get_loaders` Tauri commands

## Artifacts

- `src/components/NewInstanceModal.tsx` — consumes `listMinecraftVersions` + `getLoaders`; query keys `["mc-versions"]` and `["loaders", minecraft]`; staleTime = `META_STALE_TIME` (6h)
- `src/lib/prefetch.ts` — `prefetchStartupData`: pre-warms `["mc-versions"]` and `["loaders", latest]`; failures swallowed

## Docs

- `docs/spec/fabric-quilt-launch.md` — CP1: `fetch_profile` + `LoaderProfile`/`LoaderLibrary` types
- `docs/spec/neoforge-forge-launch.md` — `load_forge_profile`, Forge-format `LoaderLibrary` structs

## Coupling

- `loader_profile.rs` imports `Arguments`/`ArgumentEntry` from `resolver.rs` — type changes propagate to both.
- `launch_instance` calls `loader_profile::fetch_profile` (fabric/quilt) or `loader_profile::load_forge_profile` (forge/neoforge).
- `forge_installer.rs` imports `loader_profile::maven_coord_to_path`.
- `src/lib/query.ts` exports `META_STALE_TIME = 6h`; changing the Rust TTL should be mirrored here.

## Conventions

- NeoForge version-to-MC: 3-part `A.B.C` → `1.A.B`; 4-part `A.B.C.D` → `A.B.C`. `neoforge_matches` handles both plus `.0` suffix stripping.
- `sort_versions_desc` splits on `.` and `-`, parses segments as u64, compares by segment tuple.
- Cache key for Fabric/Quilt loader list: `fabric-loader-{mc}.json` / `quilt-loader-{mc}.json`; loader profile: `<kind>-profile-<mc>-<loader>.json`; both sanitize version strings (non-alphanumeric non-`.` → `_`).
- `LoaderLibrary.url` is `Option<String>`: `None` for Forge processor-produced libs. `Some("")` treated as no-download in `merge_loader_profile`.
- `profile_url` returns `Err` for "forge"/"neoforge" — those are disk-loaded, not HTTP-fetched.
