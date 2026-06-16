# providers

## What it does

Normalized mod-search and mod-install backend for Modrinth and CurseForge. Exposes a `ModProvider` async_trait with `search` + `get_versions` methods; both providers share unified types (`ProjectSummary`, `ProjectVersion`, `VersionFile`, `Dependency`, `SearchResult`). `mod_install.rs` adds a BFS dependency resolver (`resolve_install`) that partitions results into downloads / manual / unresolved / suggestions / warnings and drives the `add_mod` and `update_mod` Tauri commands. CF key resolution reads `MODLOADER_CF_API_KEY` env var first, then `settings.curseforge_api_key`, via pure `cf_api_key_from`. `ProviderHttpClient` is an injectable HTTP seam — production uses `ReqwestProviderClient`; tests inject a mock backed by a `VecDeque<MockResp>`.

## CLI code

- `src-tauri/src/core/providers.rs` (351 lines) — normalized types (`ProjectSummary`, `ProjectVersion`, `VersionFile`, `Dependency`, `SearchParams`, `SearchResult`, `ProviderKind`), `ModProvider` trait, `ProviderHttpClient` trait + `ReqwestProviderClient`, `ProviderError` enum (KeyMissing / Network / HttpStatus / BadResponse), raw MR + CF serde deserialization types, `cf_api_key_from`; tests moved to sibling `providers_tests.rs` (322 lines, 21 tests), wired via `#[cfg(test)] #[path = "providers_tests.rs"] mod tests;` stub at file end
- `src-tauri/src/core/modrinth.rs` (284 lines) — `ModrinthProvider` impl; facets JSON-encoded as `[["project_type:mod"],["versions:X"],["categories:Y"]]`; percent-encoding helper; `GET /v2/search` + `GET /v2/project/{id}/version`; client-side mc+loader filter after server-side filtering; sends `User-Agent` header matching `meta.rs` UA string; tests moved to sibling `modrinth_tests.rs` (419 lines, 15 tests), wired via `#[path]` stub
- `src-tauri/src/core/curseforge.rs` (427 lines) — `CurseForgeProvider` impl; `gameId=432`, `classId=6`; `modLoaderType` numeric mapping (Forge=1, Fabric=4, Quilt=5, NeoForge=6); `gameVersions` split heuristic (`split_game_versions`): digit-dot rule for MC versions, known-loader-name match for loader tags, unknown entries discarded; default search sort is Popularity (`SORT_FIELD_POPULARITY = 2`); `GET /v1/mods/search` + `GET /v1/mods/{id}/files` + `get_file(client, project_id, file_id)` (single-file resolver, `GET /v1/mods/{projectId}/files/{fileId}`, added for modpack-import slice B); `downloadUrl: null` maps to `VersionFile.url = None`; tests moved to sibling `curseforge_tests.rs` (690 lines, 34 tests — 26 pre-existing + 8 `get_file_*`), wired via `#[path]` stub
- `src-tauri/src/core/mod_install.rs` (599 lines) — BFS dependency resolver (`resolve_install`); output types `PlannedMod`, `ManualMod`, `UnresolvedDep`, `Suggestion`, `IncompatibleWarning`, `InstallPlan`, `AddModResult`, `UpdateModResult`, `FailedMod`; helpers `build_download_items`, `merge_mod_entries`, `partition_by_file_name`, `attribute_outcomes`, `decide_update`, `apply_swap`, `fetch_newest_compatible`, `page_url_for`; no filesystem I/O, no Tauri commands; tests moved to sibling `mod_install_tests.rs` (1164 lines, 40 tests), wired via `#[path]` stub
- `src-tauri/src/lib.rs` — `search_mods`, `get_mod_versions`, `add_mod`, `set_mod_enabled`, `remove_mod`, `update_mod` Tauri commands; `ProviderCommandError { kind, message }` IPC error type; `unknown_provider_err` helper; CF key resolved at command layer via `cf_api_key_from(env_val, settings_val)`; `add_mod` runs BFS resolve then download engine + manifest merge; `update_mod` calls `fetch_newest_compatible` + `apply_swap`; `import_curseforge_zip` (modpack domain) also resolves the CF key the same way and constructs a `CurseForgeProvider` directly to call `get_file`

## Artifacts

- `src/lib/ipc.ts` — provider section: `ProviderKind`, `ProjectSummary`, `ProjectVersion`, `VersionFile`, `Dependency`, `SearchResult`, `ProviderCommandError` interfaces; `searchMods` + `getModVersions` wrappers; Phase 5 slice B section: `ManualMod`, `UnresolvedDep`, `Suggestion`, `IncompatibleWarning`, `FailedMod`, `AddModResult`, `UpdateModResult` interfaces; `addMod`, `setModEnabled`, `removeMod`, `updateMod` wrappers; `kind` routing strings (`"modrinth"`, `"curseforge"`) are lowercase and distinct from the `ProviderKind` response value `"curseForge"` (camelCase)
- `src/routes/Browse.tsx` — live Browse page: debounced search (400 ms), MC version + loader facet selectors, All/Modrinth/CurseForge tabs; All tab renders two independent `ProviderColumn` components side-by-side; each `ProviderColumn` uses `useInfiniteQuery` with `IntersectionObserver` sentinel for infinite scroll; CF key-absent error surfaces as `KeyMissingState` (navigate-to-Settings CTA); `ModCard` shows a `+` button that opens `AddToInstanceModal`; `AddToInstanceModal` calls `listInstances`, then `getModVersions` for the selected instance's MC+loader, then `addMod`; result shown via `AddResultSummary` which surfaces `added`, `manual` (with `openUrl` CTA per entry), `unresolved`, and `failed` lists

## Docs

- `docs/spec/providers-browse.md` — slice A spec + implementation log
- `docs/design/providers.md` — design doc: problem, goals/non-goals, approach rationale, normalized pipeline diagram

## Fixtures

- `src-tauri/src/core/fixtures/modrinth_search.json` — 2-hit Modrinth search response (sodium, fabric-api)
- `src-tauri/src/core/fixtures/modrinth_versions.json` — 3-version Modrinth version list (AABBCC11/12/33 with mc+loader combos for filter tests)
- `src-tauri/src/core/fixtures/cf_search.json` — 2-mod CF search response (JEI, OptiFine with logo)
- `src-tauri/src/core/fixtures/cf_files.json` — 3-file CF files response (5034058: forge+1.20.1 with url; 5034059: neoforge+1.21; 5034060: fabric+quilt+1.20.1 with null downloadUrl)

## Coupling

- `ipc.ts` `ProviderKind` response value `"curseForge"` (camelCase) vs routing param `"curseforge"` (lowercase): two distinct string shapes in the same file; `Browse.tsx` checks `mod.provider === "modrinth"` to branch between them; any serialization change in `ProviderKind` breaks both `ipc.ts` and `Browse.tsx` (providers + frontend-shell domains).
- `ProviderCommandError.kind` string `"key_missing"` is checked by name in `Browse.tsx:isProviderCommandError`; any rename in `lib.rs` breaks the frontend key-missing UI state.
- `AddModResult` / `ManualMod` / `UnresolvedDep` / `FailedMod` Rust types in `mod_install.rs` are hand-mirrored in `ipc.ts`; any field rename or addition requires a matching `ipc.ts` update (providers + frontend-shell domains).
- `add_mod` command calls `instances::merge_mod_entries` and writes the instance manifest; changes to `instances.rs` `ModEntry` struct require matching updates in `mod_install.rs` `planned_to_mod_entry` (instances domain).
- `set_mod_enabled` and `remove_mod` commands delegate directly to `instances::set_mod_enabled` / `instances::remove_mod` (instances domain).
- Phase 6 modpack domain (`core/modpack.rs`) reuses `ProviderHttpClient`, `ProviderError`, `VersionFile`, and `CurseForgeProvider::get_file` directly for CF `.zip` import file resolution (`resolve_and_build_cf_plan` in `modpack.rs`); the providers types were designed as this substrate.
- `settings.rs` `curseforge_api_key` field feeds `cf_api_key_from` at the command layer; if the field moves or renames, the command layer must update (settings → providers coupling).
- `meta.rs` User-Agent string convention is mirrored by `modrinth.rs`; they must stay in sync.

## Conventions worth knowing

- Injectable HTTP seam pattern: `ProviderHttpClient` mirrors `AuthHttpClient` in `auth.rs`. No live HTTP in any test — all responses pre-loaded in a `VecDeque<MockResp>` or `CapturingMockClient`.
- CF key resolved at command layer (not inside provider structs for Modrinth; inside `CurseForgeProvider::require_key()` for CF). Key never reaches the frontend.
- All IPC-crossing structs carry `#[serde(rename_all = "camelCase")]`. CF numeric ids are stringified to `String` in normalized types.
- CF `gameVersions` split rule: `^\d+\.\d+` → MC version; known loader names (case-insensitive: Forge/NeoForge/Fabric/Quilt) → loader tag; anything else silently discarded.
- `VersionFile.url = None` signals `allowModDistribution: false` / `downloadUrl: null`; `mod_install.rs` routes these to `ManualMod`; `AddToInstanceModal` shows them in `AddResultSummary` with an `openUrl` CTA.
- `ModProvider` is object-safe: `Box<dyn ModProvider>` compiles (compile-time asserted in `providers_tests.rs`).
- `mod_install.rs` has no filesystem I/O and no Tauri commands — it is a pure resolver/planner; all I/O and manifest mutation happen in `lib.rs`'s `add_mod` command body.
- `AddToInstanceModal` picks the first compatible version (`versionsQuery.data?.[0]`) without user selection; a version picker is not present.
