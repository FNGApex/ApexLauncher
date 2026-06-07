# metadata

## What it does

Fetches and disk-caches Minecraft version lists (Mojang piston-meta) and per-version mod-loader build lists (Forge, Fabric, Quilt, NeoForge). All responses are cached in `<data>/meta-cache/` with a 6-hour TTL. Network failures per loader are swallowed ("not available for this MC") rather than failing the whole request.

## CLI code

- `src-tauri/src/core/versions.rs` — `McVersion` struct; `list_releases` fetches `piston-meta.mojang.com/mc/game/version_manifest_v2.json`, filters to `type=release`, stops at floor `1.7.10`; TTL 6h
- `src-tauri/src/core/loaders.rs` — `LoaderOption` struct (`kind`, `versions`); `for_mc` returns vanilla + whichever of Forge/Fabric/Quilt/NeoForge have builds for that MC; per-loader fetch helpers:
  - Forge: `maven-metadata.xml` at `maven.minecraftforge.net`; strips `{mc}-` prefix; handles 1.7.10 doubled-suffix quirk
  - Fabric/Quilt: JSON API at `meta.fabricmc.net/v2/versions/loader/{mc}` / `meta.quiltmc.org/v3/versions/loader/{mc}`; already newest-first
  - NeoForge: `maven.neoforged.net` XML; version encodes MC (e.g. `20.4.237` → 1.20.4); legacy artifact for 1.20.1 at `net/neoforged/forge`
- `src-tauri/src/core/meta.rs` — `cached_text(app, url, key, ttl)`: checks `meta-cache/<key>` mtime vs TTL, GETs if stale, writes back on success (cache write is best-effort); builds one `reqwest::Client` per call (not shared)
- `src-tauri/src/lib.rs` — Tauri commands: `list_minecraft_versions` (async), `get_loaders` (async, takes `minecraft: String`)

## Artifacts

- `src/components/NewInstanceModal.tsx` — consumes `listMinecraftVersions` + `getLoaders`; query keys `["mc-versions"]` and `["loaders", minecraft]`; staleTime mirrors Rust TTL (6h via `META_STALE_TIME`)
- `src/lib/prefetch.ts` — `prefetchStartupData`: pre-warms `["mc-versions"]` and `["loaders", latest]` at app start; failures swallowed

## Docs

- `docs/ARCHITECTURE.md` §2 — `meta-cache/` location
- `docs/PROVIDERS.md` — CurseForge and Modrinth API details (relevant to future Phase 5 extension of this layer)
- `docs/ROADMAP.md` Phase 2/4 note — version/loader metadata pulled forward early

## Coupling

- `NewInstanceModal` (instances domain) reads from this domain's query cache; `McVersion`/`LoaderOption` IPC type changes require updating `src/lib/ipc.ts` and the modal.
- `src/lib/query.ts` exports `META_STALE_TIME = 6h` used in both `NewInstanceModal` and `prefetch.ts`; changing the Rust TTL (currently `6 * 3600` in both `versions.rs` and `loaders.rs`) should be mirrored here.
- `meta.rs` builds a new `reqwest::Client` per `cached_text` call — no shared client. `download.rs` (`build_client()`) and `resolver.rs` (via `meta::cached_text`) each use separate clients; three independent reqwest clients coexist at runtime.

## Conventions worth knowing

- NeoForge version-to-MC derivation: 3-part version `A.B.C` → `1.A.B`; 4-part `A.B.C.D` → `A.B.C`. The `neoforge_matches` fn handles both plus `.0` suffix stripping.
- `sort_versions_desc` splits on `.` and `-`, parses each segment as u64 (non-numeric → 0), compares lexicographically by segment tuple.
- Cache key for Fabric/Quilt is `fabric-loader-{mc}.json` / `quilt-loader-{mc}.json`; MC version is sanitized (non-alphanumeric non-`.` → `_`) before use as filename.
- Loaders with zero builds for a given MC are excluded from the result (not returned as an empty entry).
