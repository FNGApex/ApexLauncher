# providers

## What it does

Normalized mod/modpack search backend for Modrinth, CurseForge, and FTB. Exposes a `ModProvider` async_trait with `search`, `get_versions`, `get_project`, `get_projects_brief`, and `get_pack_summary` methods; all three providers share unified types. `PackInfo` (full project detail for the `BrowsePackInfo` page), `PackSummary` (update-check result for `refresh_pack_meta`), `ModBrief` (batched lightweight metadata for `enrich_instance_mods`), and `get_projects_brief` (batch fetch of `ModBrief`s by project IDs) are shared abstractions. `ProjectSummary.page_url` carries the provider page URL captured at add-time. FTB is keyless for browse; FTB modpack install requires the CF API key (mod jars are CurseForge-hosted) and reuses the CF resolution + `pending_manual` pipeline.

## CLI code

- `src-tauri/build.rs` — parses gitignored `src-tauri/.env` for `MODLOADER_CF_API_KEY` and emits `cargo:rustc-env`; missing `.env` bakes nothing; build always succeeds
- `src-tauri/src/core/providers.rs` — normalized types: `ProjectSummary`, `ProjectVersion`, `VersionFile`, `Dependency`, `SearchParams`, `SearchResult`, `ProviderKind` (`Modrinth/CurseForge/Ftb`, serde `"modrinth"/"curseForge"/"ftb"`), `ProjectType` (Mod/Modpack), `ModBrief { project_id, name, icon_url, summary }`, `PackInfo { name, description, icon_url, author, downloads, project_type, provider }`, `PackSummary { latest_version_id, latest_version, page_url }`; `ModProvider` trait with all 5 methods; `ProviderHttpClient` trait + `ReqwestProviderClient`; `ProviderError` enum; `cf_api_key_from`; raw MR + CF serde deserialization types; 35 tests in sibling `providers_tests.rs`
- `src-tauri/src/core/modrinth.rs` — `ModrinthProvider` impl; `get_projects_brief` uses `GET /v2/projects?ids=[...]` batched call; `get_project` uses `GET /v2/project/{id}` + `GET /v2/project/{id}/members` for author; `get_pack_summary` uses `GET /v2/project/{id}/version?loaders=...&game_versions=...`; page_url built as `https://modrinth.com/{hit.project_type}/{hit.slug}`; 41 tests in sibling `modrinth_tests.rs`
- `src-tauri/src/core/curseforge.rs` — `CurseForgeProvider` impl; `get_projects_brief` uses `POST /v1/mods` (batch mods endpoint); `get_project` uses `GET /v1/mods/{id}` + description endpoint; `get_pack_summary` uses `GET /v1/mods/{id}/files` filtered to release type; `classId=6` (mods) or `classId=4471` (modpacks); `get_file(client, project_id, file_id)` single-file resolver; `get_mod_slug(client, project_id: &str) -> Result<Option<String>>` — calls `GET /v1/mods/{id}`, returns the `slug` field from `CfModData` (`#[serde(default)]`, absent on older/partial responses); `CfModData` now carries `slug: Option<String>` alongside `name`, `logo`, `authors`, `summary`; 72 tests in sibling `curseforge_tests.rs`
- `src-tauri/src/core/ftb.rs` — `FtbProvider` impl (keyless); browse API base `https://api.modpacks.ch`; list endpoints return `packs: Vec<u64>` → N+1 detail fetch for visible window; `search`: empty query → featured+popular deduped feed, term → `/public/modpack/search/{limit}?term=...`; `get_versions`: detail endpoint, sorted newest-first by id, `files` empty (manifest consumed at install time); `get_project`: detail → `PackInfo`; `get_pack_summary`: detail → `PackSummary`; `get_projects_brief`: no-op (no batch FTB mod-metadata endpoint; api-frugality); `newest_release_version(client, id)` + `get_version_manifest(client, id, version_id)` public helpers for the install planner; pub manifest types: `FtbVersionManifest { name, manifest_type, targets, specs, files }`, `FtbFile { name, path, url, sha1, size, file_type, clientonly, serveronly, optional, curseforge }`, `FtbCurseforge { project, file }`, `FtbTarget { name, target_type, version }`, `FtbSpecs { minimum, recommended }`; `FtbVersionManifest` has helper methods `loader()`, `minecraft()`, `loader_version()`; 11 tests in sibling `ftb_tests.rs`
- `src-tauri/src/lib.rs` — `search_mods`, `get_mod_versions`, `get_pack_info`, `refresh_pack_meta` Tauri commands all dispatch on `"ftb"` provider arm; `ProviderCommandError { kind, message }` IPC error type; `enrich_instance_mods` command uses `get_projects_brief` to backfill mod metadata

## Artifacts

- `src/lib/ipc.ts` — `ProjectType`, `ProjectSummary`, `ProjectVersion`, `VersionFile`, `Dependency`, `SearchResult`, `ProviderCommandError`, `PackInfo`, `ModBrief` interfaces; `searchMods`, `getModVersions`, `getPackInfo` wrappers; provider routing strings lowercase (`"modrinth"`, `"curseforge"`, `"ftb"`)
- `src/lib/bindings.ts` — `ProviderKind` union is `"modrinth" | "curseForge" | "ftb"` (note: `"curseForge"` camelCase from Rust serde, but ipc.ts routing uses lowercase `"curseforge"`; these are two distinct string shapes)

## Docs

- `docs/spec/providers-browse.md` — slice A spec + implementation log
- `docs/spec/curseforge-api-key.md` — baked CF key tier spec
- `docs/design/providers.md` — normalized pipeline diagram, rationale
- `docs/spec/browse-rework.md` — BR-A through BR-D; `get_pack_info`, per-provider split
- `docs/spec/mod-metadata-ux.md` — `ModBrief`/`get_projects_brief`/`enrich_instance_mods` spec
- `docs/spec/ftb-integration.md` — FTB provider spec: API facts, install flow, CF-referenced file resolution
- `docs/design/ftb-integration.md` — FTB design rationale: keyless browse, CF-hosted jars, pending_manual pipeline reuse

## Fixtures

- `src-tauri/src/core/fixtures/modrinth_search.json` — 2-hit MR search response
- `src-tauri/src/core/fixtures/modrinth_versions.json` — 3-version MR version list
- `src-tauri/src/core/fixtures/modrinth_project_sodium.json` — full project detail for get_project test
- `src-tauri/src/core/fixtures/modrinth_members_sodium.json` — team members for get_project author test
- `src-tauri/src/core/fixtures/modrinth_projects_batch.json` — batch projects response for get_projects_brief test
- `src-tauri/src/core/fixtures/cf_search.json` — 2-mod CF search response
- `src-tauri/src/core/fixtures/cf_files.json` — 3-file CF files response (includes null downloadUrl)
- `src-tauri/src/core/fixtures/cf_mod_jei.json` — single mod detail for get_project test
- `src-tauri/src/core/fixtures/cf_mod_jei_description.json` — description response for get_project test
- `src-tauri/src/core/fixtures/cf_mods_batch.json` — batch mods response for get_projects_brief test
- `src-tauri/src/core/fixtures/ftb_featured.json` — FTB featured pack id list response
- `src-tauri/src/core/fixtures/ftb_popular.json` — FTB popular pack id list response
- `src-tauri/src/core/fixtures/ftb_search.json` — FTB search result (pack id list)
- `src-tauri/src/core/fixtures/ftb_modpack_detail.json` — FTB pack detail response (`/public/modpack/{id}`)

## Coupling

- `ipc.ts` `ProviderKind` response value `"curseForge"` (camelCase) vs routing param `"curseforge"` (lowercase): two distinct string shapes. `Browse.tsx`, `BrowsePackInfo.tsx`, `InstanceDetail.tsx`, `InfoTab.tsx` all branch on `mod.provider`/`instance.source.provider`; any serialization change breaks all these files. `"ftb"` is the same in both shapes (all-lowercase).
- `ProviderCommandError.kind === "key_missing"` is checked by name in `Browse.tsx` and `InstanceDetail.tsx`; any rename in `lib.rs` breaks the frontend key-missing UI state. FTB browse is keyless so `key_missing` cannot arise there; it can arise on FTB install (CF key required for CF-referenced jars).
- `get_pack_info` result (`PackInfo`) is used by `BrowsePackInfo.tsx` and `InfoTab.tsx` for pack detail display; `PackSummary` is used by `refresh_pack_meta` in `lib.rs` to write update-check fields to the instance manifest.
- `enrich_instance_mods` in `lib.rs` calls `provider.get_projects_brief(mr_ids, cf_ids)` for each provider separately; FTB `get_projects_brief` is a no-op (no batch API), so FTB-added mods skip enrichment.
- modpack domain (`core/modpack.rs`) reuses `ProviderHttpClient`, `ProviderError`, `VersionFile`, and `CurseForgeProvider::get_file` for CF file resolution (used by both CF pack builds and FTB's CF-referenced file subset).
- `settings.rs` `curseforge_api_key` field feeds `cf_api_key_from` at the command layer; FTB install calls `cf_api_key_from` inside `resolve_and_build_ftb_plan` for the CF-referenced files.
- FTB `get_versions` returns empty `files` on each `ProjectVersion` (no single jar); the install path consumes `FtbVersionManifest` directly via `get_version_manifest`.

## Conventions worth knowing

- Injectable HTTP seam pattern: `ProviderHttpClient` mirrors `AuthHttpClient` in `auth.rs`. No live HTTP in any test.
- CF key resolved at command layer via `cf_api_key_from(env_val, settings_val, baked_val)`. Precedence: env → settings → baked → `None`. Key never reaches the frontend.
- All IPC-crossing structs carry `#[serde(rename_all = "camelCase")]`. CF numeric ids stringified to `String`.
- `ProjectType::Mod` → MR facet `project_type:mod`, CF `classId=6`. `ProjectType::Modpack` → facet `project_type:modpack`, CF `classId=4471`. FTB is modpack-only; `ProjectType::Mod` is never used with FTB.
- `ModProvider` is object-safe: `Box<dyn ModProvider>` compiles (asserted in `providers_tests.rs`).
- `mod_install.rs` has no filesystem I/O and no Tauri commands — pure resolver/planner.
- FTB N+1 detail fetch is acceptable because the catalog is small (~130 packs) and `api.modpacks.ch` caches aggressively (`max-age=900`). Only the visible window is fetched, not the full catalog.
- FTB pack page URL format: `https://www.feed-the-beast.com/modpacks/{id}` (numeric id, no slug — FTB has no slug concept).
- FTB `ProviderKind` serializes as `"ftb"` (all-lowercase, unambiguous in both Rust serde `camelCase` form and ipc.ts routing form).
