# providers

## What it does

Normalized mod/modpack search backend for Modrinth and CurseForge. Exposes a `ModProvider` async_trait with `search`, `get_versions`, `get_project`, `get_projects_brief`, and `get_pack_summary` methods; both providers share unified types. On ui-overhaul: `PackInfo` (full project detail for the `BrowsePackInfo` page), `PackSummary` (update-check result for `refresh_pack_meta`), `ModBrief` (batched lightweight metadata for `enrich_instance_mods`), and `get_projects_brief` (batch fetch of `ModBrief`s by project IDs) were added. `ProjectSummary.page_url` carries the provider page URL captured at add-time.

## CLI code

- `src-tauri/build.rs` — parses gitignored `src-tauri/.env` for `MODLOADER_CF_API_KEY` and emits `cargo:rustc-env`; missing `.env` bakes nothing; build always succeeds
- `src-tauri/src/core/providers.rs` — normalized types: `ProjectSummary`, `ProjectVersion`, `VersionFile`, `Dependency`, `SearchParams`, `SearchResult`, `ProviderKind`, `ProjectType` (Mod/Modpack), `ModBrief { project_id, name, icon_url, summary }`, `PackInfo { name, description, icon_url, author, downloads, project_type, provider }`, `PackSummary { latest_version_id, latest_version, page_url }`; `ModProvider` trait with all 5 methods; `ProviderHttpClient` trait + `ReqwestProviderClient`; `ProviderError` enum; `cf_api_key_from`; raw MR + CF serde deserialization types; 35 tests in sibling `providers_tests.rs`
- `src-tauri/src/core/modrinth.rs` — `ModrinthProvider` impl; `get_projects_brief` uses `GET /v2/projects?ids=[...]` batched call; `get_project` uses `GET /v2/project/{id}` + `GET /v2/project/{id}/members` for author; `get_pack_summary` uses `GET /v2/project/{id}/version?loaders=...&game_versions=...`; page_url built as `https://modrinth.com/{hit.project_type}/{hit.slug}`; 40 tests in sibling `modrinth_tests.rs`
- `src-tauri/src/core/curseforge.rs` — `CurseForgeProvider` impl; `get_projects_brief` uses `POST /v1/mods` (batch mods endpoint); `get_project` uses `GET /v1/mods/{id}` + description endpoint; `get_pack_summary` uses `GET /v1/mods/{id}/files` filtered to release type; `classId=6` (mods) or `classId=4471` (modpacks); `get_file(client, project_id, file_id)` single-file resolver; 67 tests in sibling `curseforge_tests.rs`
- `src-tauri/src/lib.rs` — `search_mods`, `get_mod_versions`, `get_pack_info` Tauri commands; `ProviderCommandError { kind, message }` IPC error type; `enrich_instance_mods` command uses `get_projects_brief` to backfill mod metadata

## Artifacts

- `src/lib/ipc.ts` — `ProjectType`, `ProjectSummary`, `ProjectVersion`, `VersionFile`, `Dependency`, `SearchResult`, `ProviderCommandError`, `PackInfo`, `ModBrief` interfaces; `searchMods`, `getModVersions`, `getPackInfo` wrappers; provider routing strings lowercase (`"modrinth"`, `"curseforge"`)

## Docs

- `docs/spec/providers-browse.md` — slice A spec + implementation log
- `docs/spec/curseforge-api-key.md` — baked CF key tier spec
- `docs/design/providers.md` — normalized pipeline diagram, rationale
- `docs/spec/browse-rework.md` — BR-A through BR-D; `get_pack_info`, per-provider split
- `docs/spec/mod-metadata-ux.md` — `ModBrief`/`get_projects_brief`/`enrich_instance_mods` spec

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

## Coupling

- `ipc.ts` `ProviderKind` response value `"curseForge"` (camelCase) vs routing param `"curseforge"` (lowercase): two distinct string shapes. `Browse.tsx`, `BrowsePackInfo.tsx`, `InstanceDetail.tsx` all branch on `mod.provider`/`instance.source.provider`; any serialization change breaks all three files.
- `ProviderCommandError.kind === "key_missing"` is checked by name in `Browse.tsx` and `InstanceDetail.tsx`; any rename in `lib.rs` breaks the frontend key-missing UI state.
- `get_pack_info` result (`PackInfo`) is used by `BrowsePackInfo.tsx` and `InfoTab.tsx` for pack detail display; `PackSummary` is used by `refresh_pack_meta` in `lib.rs` to write update-check fields to the instance manifest.
- `enrich_instance_mods` in `lib.rs` calls `provider.get_projects_brief(mr_ids, cf_ids)` for each provider separately, merges `ModBrief` results back onto manifest `ModEntry` fields by `project_id`; any `ModBrief` field change requires updating `collect_missing_ids` and `apply_briefs` helpers in `lib.rs`.
- Phase 6 modpack domain (`core/modpack.rs`) reuses `ProviderHttpClient`, `ProviderError`, `VersionFile`, and `CurseForgeProvider::get_file`.
- `settings.rs` `curseforge_api_key` field feeds `cf_api_key_from` at the command layer.

## Conventions worth knowing

- Injectable HTTP seam pattern: `ProviderHttpClient` mirrors `AuthHttpClient` in `auth.rs`. No live HTTP in any test.
- CF key resolved at command layer via `cf_api_key_from(env_val, settings_val, baked_val)`. Precedence: env → settings → baked → `None`. Key never reaches the frontend.
- All IPC-crossing structs carry `#[serde(rename_all = "camelCase")]`. CF numeric ids stringified to `String`.
- `ProjectType::Mod` → MR facet `project_type:mod`, CF `classId=6`. `ProjectType::Modpack` → facet `project_type:modpack`, CF `classId=4471`.
- `ModProvider` is object-safe: `Box<dyn ModProvider>` compiles (asserted in `providers_tests.rs`).
- `mod_install.rs` has no filesystem I/O and no Tauri commands — pure resolver/planner.
