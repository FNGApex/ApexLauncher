# providers

## What it does

Normalized mod/modpack search backend for Modrinth and CurseForge. Exposes a `ModProvider` async_trait with `search` + `get_versions` methods; both providers share unified types (`ProjectSummary`, `ProjectVersion`, `VersionFile`, `Dependency`, `SearchResult`). `SearchParams` carries a `ProjectType` enum (`Mod` | `Modpack`) that routes to Modrinth's `project_type:mod|modpack` facet and CF's `classId` 6 (mods) | 4471 (modpacks). `ProjectSummary` carries `page_url: Option<String>`: Modrinth builds it as `https://modrinth.com/{project_type}/{slug}` (from the hit's own `project_type` field, not the search selector); CurseForge takes `links.websiteUrl` verbatim. `ProviderHttpClient` is an injectable HTTP seam — production uses `ReqwestProviderClient`; tests inject a mock backed by a `VecDeque<MockResp>`. CF key resolution reads `MODLOADER_CF_API_KEY` env var first, then `settings.curseforge_api_key`, via pure `cf_api_key_from`.

## CLI code

- `src-tauri/src/core/providers.rs` — normalized types (`ProjectSummary`, `ProjectVersion`, `VersionFile`, `Dependency`, `SearchParams`, `SearchResult`, `ProviderKind`), `ProjectType` enum (`Mod`/`Modpack`), `ModProvider` trait, `ProviderHttpClient` trait + `ReqwestProviderClient`, `ProviderError` enum (KeyMissing / Network / HttpStatus / BadResponse), raw MR + CF serde deserialization types, `cf_api_key_from`; 25 tests in sibling `providers_tests.rs`, wired via `#[cfg(test)] #[path = "providers_tests.rs"] mod tests;` stub
- `src-tauri/src/core/modrinth.rs` — `ModrinthProvider` impl; `project_type` facet derived from `SearchParams.project_type` (`"mod"` or `"modpack"`); facets JSON-encoded as `[["project_type:..."],["versions:X"],["categories:Y"]]`; `page_url` built as `https://modrinth.com/{hit.project_type}/{hit.slug}` using the hit's own `project_type` field; percent-encoding helper; `GET /v2/search` + `GET /v2/project/{id}/version`; client-side mc+loader filter after server-side filtering; 6 tests in sibling `modrinth_tests.rs`, wired via `#[path]` stub
- `src-tauri/src/core/curseforge.rs` — `CurseForgeProvider` impl; `gameId=432`; `classId=6` (mods) or `classId=4471` (modpacks) selected by `SearchParams.project_type`; `MODPACKS_CLASS_ID = 4471` constant; `modLoaderType` numeric mapping (Forge=1, Fabric=4, Quilt=5, NeoForge=6); `gameVersions` split heuristic; `page_url` taken from `links.websiteUrl` verbatim; `GET /v1/mods/search` + `GET /v1/mods/{id}/files` + `get_file(client, project_id, file_id)` (single-file resolver for modpack-import slice B); `downloadUrl: null` maps to `VersionFile.url = None`; 12 tests in sibling `curseforge_tests.rs` (8 `get_file_*` + 4 others; full file has 34 tests total with pre-existing search/files tests), wired via `#[path]` stub
- `src-tauri/src/core/mod_install.rs` (599 lines) — BFS dependency resolver (`resolve_install`); output types `PlannedMod`, `ManualMod`, `UnresolvedDep`, `Suggestion`, `IncompatibleWarning`, `InstallPlan`, `AddModResult`, `UpdateModResult`, `FailedMod`; helpers `build_download_items`, `merge_mod_entries`, `partition_by_file_name`, `attribute_outcomes`, `decide_update`, `apply_swap`, `fetch_newest_compatible`, `page_url_for`; no filesystem I/O, no Tauri commands; 26 tests in sibling `mod_install_tests.rs`, wired via `#[path]` stub
- `src-tauri/src/lib.rs` — `search_mods` (now accepts `project_type` arg), `get_mod_versions`, `add_mod`, `set_mod_enabled`, `remove_mod`, `update_mod` Tauri commands; `ProviderCommandError { kind, message }` IPC error type

## Artifacts

- `src/lib/ipc.ts` — `ProjectType = "mod" | "modpack"` type alias; `ProjectSummary` interface includes `pageUrl: string | null`; `searchMods` wrapper accepts `projectType: ProjectType = "mod"` as last arg; `ProviderKind`, `ProjectVersion`, `VersionFile`, `Dependency`, `SearchResult`, `ProviderCommandError` interfaces; Phase 5 slice B types: `ManualMod`, `UnresolvedDep`, `Suggestion`, `IncompatibleWarning`, `FailedMod`, `AddModResult`, `UpdateModResult`; `addMod`, `setModEnabled`, `removeMod`, `updateMod` wrappers; `kind` routing strings (`"modrinth"`, `"curseforge"`) are lowercase and distinct from `ProviderKind` response value `"curseForge"` (camelCase)

## Docs

- `docs/spec/providers-browse.md` — slice A spec + implementation log
- `docs/spec/ui-modpack-rework.md` — CP3 spec: `ProjectType` param, `page_url` field, Browse rewrite
- `docs/design/providers.md` — design doc: problem, goals/non-goals, approach rationale, normalized pipeline diagram
- `docs/design/ui-modpack-rework.md` — design rationale for Browse-as-modpack-feed and slide-over approach

## Fixtures

- `src-tauri/src/core/fixtures/modrinth_search.json` — 2-hit Modrinth search response (sodium, fabric-api)
- `src-tauri/src/core/fixtures/modrinth_versions.json` — 3-version Modrinth version list
- `src-tauri/src/core/fixtures/cf_search.json` — 2-mod CF search response (JEI, OptiFine)
- `src-tauri/src/core/fixtures/cf_files.json` — 3-file CF files response (includes null downloadUrl case)

## Coupling

- `ipc.ts` `ProviderKind` response value `"curseForge"` (camelCase) vs routing param `"curseforge"` (lowercase): two distinct string shapes; `Browse.tsx` and `InstanceDetail.tsx` both branch on `mod.provider === "modrinth"` / `=== "curseForge"`; any serialization change breaks both files (providers + frontend-shell domains).
- `ProviderCommandError.kind` string `"key_missing"` is checked by name in both `Browse.tsx` and `InstanceDetail.tsx` `AddModTab`; any rename in `lib.rs` breaks the frontend key-missing UI state.
- `AddModResult` / `ManualMod` / `UnresolvedDep` / `FailedMod` Rust types in `mod_install.rs` are hand-mirrored in `ipc.ts`; any field rename or addition requires a matching `ipc.ts` update.
- `add_mod` command calls `instances::merge_mod_entries` and writes the instance manifest; changes to `instances.rs` `ModEntry` struct require matching updates in `mod_install.rs` `planned_to_mod_entry` (instances domain).
- `set_mod_enabled` and `remove_mod` commands delegate directly to `instances::set_mod_enabled` / `instances::remove_mod` (instances domain).
- Phase 6 modpack domain (`core/modpack.rs`) reuses `ProviderHttpClient`, `ProviderError`, `VersionFile`, and `CurseForgeProvider::get_file` directly; the providers types are the substrate.
- `settings.rs` `curseforge_api_key` field feeds `cf_api_key_from` at the command layer; if the field moves, the command layer must update (settings → providers coupling).

## Conventions worth knowing

- Injectable HTTP seam pattern: `ProviderHttpClient` mirrors `AuthHttpClient` in `auth.rs`. No live HTTP in any test — all responses pre-loaded in a `VecDeque<MockResp>`.
- CF key resolved at command layer via `cf_api_key_from(env_val, settings_val)`. Key never reaches the frontend.
- All IPC-crossing structs carry `#[serde(rename_all = "camelCase")]`. CF numeric ids are stringified to `String` in normalized types.
- `ProjectType::Mod` → Modrinth facet `project_type:mod`, CF `classId=6`. `ProjectType::Modpack` → facet `project_type:modpack`, CF `classId=4471`.
- Modrinth `page_url` uses the *hit's own* `project_type` field (from the response row), not the search selector — correctly handles edge cases where the API returns a different type.
- `VersionFile.url = None` signals `allowModDistribution: false` / `downloadUrl: null`; `mod_install.rs` routes these to `ManualMod`.
- `ModProvider` is object-safe: `Box<dyn ModProvider>` compiles (compile-time asserted in `providers_tests.rs`).
- `mod_install.rs` has no filesystem I/O and no Tauri commands — it is a pure resolver/planner; all I/O and manifest mutation happen in `lib.rs`'s `add_mod` command body.
