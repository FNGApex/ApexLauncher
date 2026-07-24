# providers

## Overview

Normalized mod/modpack search backend for Modrinth, CurseForge, FTB, and ATLauncher. `ModProvider` async_trait with `search`, `get_versions`, `get_project`, `get_projects_brief`, `get_pack_summary`. Shared types: `PackInfo` (full detail for BrowsePackInfo), `PackSummary` (update-check result), `ModBrief` (batched lightweight metadata for `enrich_instance_mods`).

FTB: keyless browse; CF API key required for install (mod jars are CurseForge-hosted). ATL: keyless for both browse AND install (jars CDN-hosted on ATL servers).

## CLI code

- `src-tauri/build.rs` — parses `src-tauri/.env` for `MODLOADER_CF_API_KEY`, emits `cargo:rustc-env`; missing `.env` bakes nothing; build always succeeds
- `src-tauri/src/core/providers.rs` — `ProjectSummary`, `ProjectVersion`, `VersionFile`, `Dependency`, `SearchParams`, `SearchResult`, `ProviderKind` (`Modrinth/CurseForge/Ftb/Atlauncher`; serde `"modrinth"/"curseForge"/"ftb"/"atlauncher"`), `ProjectType` (Mod/Modpack), `ModBrief { project_id, name, icon_url, summary }`, `PackInfo`, `PackSummary`; `ModProvider` trait (5 methods); `ProviderHttpClient` trait + `ReqwestProviderClient` (injectable seam); `ProviderError`; `cf_api_key_from`; 35 tests in `providers_tests.rs`
- `src-tauri/src/core/modrinth.rs` — `ModrinthProvider`; `get_projects_brief` uses `GET /v2/projects?ids=[...]`; `get_project` uses `GET /v2/project/{id}` + members for author; 41 tests in `modrinth_tests.rs`
- `src-tauri/src/core/curseforge.rs` — `CurseForgeProvider`; `get_projects_brief` uses `POST /v1/mods`; `get_project` uses `GET /v1/mods/{id}` + description endpoint; `classId=6` (mods) / `classId=4471` (modpacks); `get_file(client, project_id, file_id)` single-file resolver; `get_mod_slug(client, project_id) -> Result<Option<String>>`; `CfModData { slug: Option<String>, name, logo, authors, summary }` (`#[serde(default)]`); 72 tests in `curseforge_tests.rs`
- `src-tauri/src/core/ftb.rs` — `FtbProvider` (keyless); base `https://api.modpacks.ch`; empty query → featured+popular deduped feed; term → `/public/modpack/search/{limit}?term=...`; `get_versions`: newest-first by id, `files` empty (manifest consumed at install time); `get_projects_brief`: no-op (api-frugality); `newest_release_version` + `get_version_manifest` helpers; pub types: `FtbVersionManifest { name, manifest_type, targets, specs, files }`, `FtbFile { name, path, url, sha1, size, file_type, clientonly, serveronly, optional, curseforge }`, `FtbCurseforge { project, file }`, `FtbTarget { name, target_type, version }`, `FtbSpecs { minimum, recommended }`; 11 tests in `ftb_tests.rs`
- `src-tauri/src/core/atl.rs` — `AtlProvider` (keyless unit struct); base `https://api.atlauncher.com/v1`; UA-gated (Cloudflare-1020 guard); one `/packs/full/public` call powers Browse (client-side substring filter, name-sorted, windowed); `get_projects_brief`: no-op (api-frugality); pub types: `AtlConfigsManifest { mods, loader_version }`, `AtlLoader { loader_type, version }`, `AtlMod { name, optional, client_download, server_download, url, md5, size }`, `AtlConfigs`; `AtlEnvelope<T>` + `unwrap_envelope` (keys on `error: bool`, `code` optional); helpers `newest_version`, `get_configs_manifest`, `cdn_image_url` (no fetch, deterministic); version path segments percent-encoded; 18 tests in `atl_tests.rs`
- `src-tauri/src/lib.rs` — `search_mods`, `get_mod_versions`, `get_pack_info`, `refresh_pack_meta` dispatch on all four provider arms; `ProviderCommandError { kind, message }`; `enrich_instance_mods` calls `get_projects_brief` per provider

## Artifacts

- `src/lib/ipc.ts` — `ProjectType`, `ProjectSummary`, `ProjectVersion`, `VersionFile`, `Dependency`, `SearchResult`, `ProviderCommandError`, `PackInfo`, `ModBrief`; `searchMods`, `getModVersions`, `getPackInfo` wrappers; routing strings lowercase (`"modrinth"`, `"curseforge"`, `"ftb"`, `"atlauncher"`)
- `src/lib/bindings.ts` — `ProviderKind` union: `"modrinth" | "curseForge" | "ftb" | "atlauncher"` (note `"curseForge"` camelCase)

## Fixtures

`src-tauri/src/core/fixtures/`: `modrinth_search.json`, `modrinth_versions.json`, `modrinth_project_sodium.json`, `modrinth_members_sodium.json`, `modrinth_projects_batch.json`, `cf_search.json`, `cf_files.json`, `cf_mod_jei.json`, `cf_mod_jei_description.json`, `cf_mods_batch.json`, `ftb_featured.json`, `ftb_popular.json`, `ftb_search.json`, `ftb_modpack_detail.json`, `atl_packs_public.json`, `atl_pack_detail.json`, `atl_pack_latest.json`, `atl_configs.json`

## Docs

- `docs/spec/providers-browse.md`, `docs/spec/curseforge-api-key.md`, `docs/design/providers.md`
- `docs/spec/ftb-integration.md`, `docs/spec/atlauncher-integration.md`

## Coupling

- `ProviderKind` serde `"curseForge"` vs ipc.ts routing `"curseforge"`: two distinct string shapes. `"ftb"` and `"atlauncher"` are the same in both.
- `ProviderCommandError.kind === "key_missing"` checked by name in `Browse.tsx` and `InstanceDetail.tsx`. FTB browse is keyless; FTB install can raise `key_missing`. ATL never raises `key_missing`.
- `modpack.rs` reuses `ProviderHttpClient`, `ProviderError`, `VersionFile`, `CurseForgeProvider::get_file` for CF file resolution (CF pack builds + FTB CF-referenced files). ATL install does not call `CurseForgeProvider::get_file`.
- `enrich_instance_mods` calls `provider.get_projects_brief`; FTB and ATL `get_projects_brief` are no-ops — mods from those providers skip enrichment.

## Conventions

- No live HTTP in any test (injectable `ProviderHttpClient` seam mirrors `AuthHttpClient` pattern).
- CF key resolved at command layer via `cf_api_key_from(env_val, settings_val, baked_val)`. Precedence: env → settings → baked → `None`. Key never reaches frontend.
- `ModProvider` is object-safe: `Box<dyn ModProvider>` compiles.
- `ProjectType::Mod` → MR facet `project_type:mod`, CF `classId=6`. `ProjectType::Modpack` → facet `project_type:modpack`, CF `classId=4471`. FTB and ATL are modpack-only.
- FTB N+1 detail fetch acceptable: small catalog (~130 packs), `api.modpacks.ch` caches aggressively. Only visible window fetched.
- ATL one `/packs/full/public` call for whole catalog; client-side substring filter. `api.atlauncher.com` is UA-gated.
- FTB pack page URL: `https://www.feed-the-beast.com/modpacks/{id}` (numeric id, no slug).
- ATL pack page URL: `https://atlauncher.com/pack/{name}`.
