# Providers browse (Phase 5, slice A)

## Goal

Expose Modrinth and CurseForge mod search to the user from a single Browse page. A
`ModProvider` trait + normalized types let the UI and the future pack resolver
(Phase 6) remain provider-agnostic. Slice A is read-only discovery: search, filter,
scroll, inspect versions. Installation is slice B.

## Non-goals

- Mod install, dependency resolution, enable/disable/update (slice B / `docs/spec/mod-install.md`).
- Modpack import (`.mrpack`, CF zip) — Phase 6.
- Resource packs, shaders, datapacks (`classId` / `project_type` locked to mods).
- Jar fingerprint reconciliation of manually added mods (Murmur2 / sha512 lookup).
- Provider account auth (CF/Modrinth user accounts).
- Disk-caching of search results (TanStack Query's 30 s `staleTime` is sufficient; rate-limit
  TTL on the version list is a follow-up risk, not built now).

## Success criteria

- [ ] `ModProvider` trait compiles; both Modrinth and CurseForge implementations satisfy it
      with a mock-injectable HTTP seam (same async_trait pattern as `AuthHttpClient`,
      `auth.rs:226`).
- [ ] `search_mods` Tauri command returns `Vec<ProjectSummary>` for Modrinth (no key) and
      CurseForge (key present), passing the query, MC version, loader, offset, and limit
      parameters through to the respective APIs.
- [ ] `get_mod_versions` Tauri command returns `Vec<ProjectVersion>` filtered to versions
      compatible with the supplied MC version + loader.
- [ ] CF `downloadUrl: null` maps to `VersionFile.url = None` (not an error); the field is
      serialized to the frontend as `null`.
- [ ] CF key absent: `search_mods` for the `curseforge` provider returns a
      `ProviderCommandError { kind: "key_missing", … }` rather than a panic or an
      untyped string error.
- [ ] All new Rust tests pass under `cargo test`; no live HTTP calls in any test (all
      responses fixture-backed via the injectable seam).
- [ ] `cargo check` produces 0 errors.
- [ ] `npm run build` (tsc + vite) passes with 0 type errors after `ipc.ts` is updated.
- [ ] Browse page: typing in the search box fires a debounced (≥ 300 ms; 400 ms target per
      design) query; results
      render as cards; infinite scroll loads the next page on scroll-to-bottom.
- [ ] Browse page: MC version + loader facet selectors are visible and wired to the query
      parameters.
- [ ] Browse page: "All" tab renders two side-by-side columns (one per provider) with
      independent pagination; "Modrinth" and "CurseForge" tabs show a single provider column.
- [ ] Browse page: when CF key is absent, the CurseForge column/tab shows a "API key
      required" state (not a crash, not an empty list with no explanation).

## Approaches

| # | Approach | Pros | Cons |
|---|----------|------|------|
| A | Rust-side `ModProvider` trait + normalized domain types; unified thin Tauri commands; frontend provider-agnostic | CF key never leaves backend; Phase 6 pack resolver reuses clients + types in Rust; testable with mock-HTTP convention (same as auth + download + launch) | Normalization quirks (CF `gameVersions` mixing loaders and MC versions) are solved in Rust |
| B | Thin per-provider Rust passthrough; normalize in TypeScript | Faster UI iteration | Duplicates normalization for Phase 6 pack resolver (Rust); two normalization sources of truth; more `ipc.ts` drift surface |
| C | Frontend fetches provider APIs directly from the webview | No IPC layer | CF key in the bundle (forbidden); CORS; UA etiquette uncontrollable |

## Recommendation

**Approach A.** `docs/PROVIDERS.md:52` prescribes "the UI and pack resolver don't branch on
provider." The roadmap requires the CF key to stay backend-side. Phase 6 pack resolution
("parse index → DownloadPlan") will consume `Vec<VersionFile>` in Rust — normalization must
live there. B forks normalization; C leaks the key. Full rationale and sub-decisions:
`docs/design/providers.md` §Recommendation.

| Decision | Choice | Evidence |
|----------|--------|----------|
| HTTP seam | New injectable `ProviderHttpClient` trait (async_trait), **not** `meta::cached_text` | `meta.rs:33` `cached_text` is GET-text + 6 h disk cache; CF needs `x-api-key` header and search queries are parameterized (disk-caching explodes keys + staleness); seam follows `AuthHttpClient` (`auth.rs:226`) |
| Search result caching | None in Rust; TanStack Query 30 s staleTime (`query.ts`) | Interactive search; double-caching adds invalidation bugs |
| CF key resolution | `settings.curseforge_api_key` (`settings.rs:31`) + `MODLOADER_CF_API_KEY` env override | Mirrors `ms_client_id()` pattern (`auth.rs:28`): env for dev/CI, settings for users |
| Pagination contract | Normalize to `offset` / `limit` / `total` | Modrinth is offset/limit natively; CF `index`/`pageSize` maps 1:1 |
| "All providers" tab | Two independent paginated queries side-by-side, not an interleaved merged stream | No stable cross-provider sort key; merging two cursors has no win for v1 |
| Command errors | `ProviderCommandError { kind, message }` | Mirrors `AuthCommandError` (`lib.rs:29`) |

## Checkpoints

| # | Checkpoint | Files/areas | Agent | Est. files | Verifies |
|---|------------|-------------|-------|------------|----------|
| 1 | **Providers module: types + trait + HTTP seam.** New `core/providers.rs`: normalized structs (`ProjectSummary`, `ProjectVersion`, `VersionFile`, `Dependency`, `SearchParams`, `SearchResult`), `ModProvider` async_trait, `ProviderHttpClient` async_trait (injectable seam), and the CF-key-resolution helper (settings field + env override). Register in `core/mod.rs`. Add two fixture JSON files under `core/fixtures/` (one Modrinth search response, one CF search response). | `src-tauri/src/core/providers.rs`, `core/mod.rs`, `core/fixtures/modrinth_search.json`, `core/fixtures/cf_search.json` | atomic-builder | ~4 | Fixture JSON deserializes into normalized types; `ModProvider` trait is object-safe; `ProviderHttpClient` seam compiles; `cargo check` 0 errors |
| 2 | **Modrinth client.** `core/modrinth.rs` implementing `ModProvider`: `search` (GET `/search`, facets for loader + MC version, UA header per `docs/PROVIDERS.md:8`) and `get_versions` (GET `/project/{id}/version`, filtered by `game_versions` + `loaders`). HTTP injected via the seam from CP1. | `src-tauri/src/core/modrinth.rs`, `core/mod.rs` | atomic-builder | ~2 | Unit tests: fixture-backed search returns correct `ProjectSummary` count + field mapping; fixture-backed versions returns only compatible entries; SHA-1 + SHA-512 hash fields present in `VersionFile.hashes`; `cargo test` green |
| 3 | **CurseForge client.** `core/curseforge.rs` implementing `ModProvider`: `search` (GET `/v1/mods/search?gameId=432`, `x-api-key` header, `modLoaderType`, `classId` fixed to mods) and `get_versions` (GET `/v1/mods/{id}/files`, `gameVersions[]` splitting to extract MC versions vs loader names). `downloadUrl: null` → `VersionFile.url = None`. Key-absent path returns an `Err` with `kind = "key_missing"` before any HTTP call. | `src-tauri/src/core/curseforge.rs`, `core/mod.rs`, `core/fixtures/cf_files.json` | atomic-builder | ~3 | Unit tests: fixture search maps to `ProjectSummary`; `downloadUrl: null` deserializes to `url: None`; key-absent returns `key_missing` error without HTTP; `gameVersions` mixing is split correctly; `cargo test` green |
| 4 | **Tauri commands + `ipc.ts` mirrors.** `search_mods(provider, query, mc_version, loader, offset, limit)` and `get_mod_versions(provider, project_id, mc_version, loader)` commands in `lib.rs`; `ProviderCommandError { kind, message }` serializable struct (mirrors `AuthCommandError`); both commands registered in `invoke_handler`. Mirror types in `src/lib/ipc.ts` (`ProjectSummary`, `ProjectVersion`, `VersionFile`, `ProviderCommandError`, `searchMods`, `getModVersions` typed wrappers). | `src-tauri/src/lib.rs`, `src/lib/ipc.ts` | atomic-builder | ~2 | `cargo check` 0 errors; `npm run build` 0 type errors; commands appear in `invoke_handler` registry (`lib.rs:708-730` block) |
| 5 | **Browse UI.** Replace the `Browse.tsx` stub with a live implementation: debounced search input (≥ 300 ms; 400 ms target), MC version + loader facet selectors (populated from `listMinecraftVersions` / `getLoaders`), provider tabs (All / Modrinth / CurseForge), `useInfiniteQuery` per-provider, mod result cards (icon, name, summary, download count), "All" tab side-by-side columns, CF-key-missing empty state. No install affordances (slice B). | `src/routes/Browse.tsx`, `src/lib/ipc.ts` (additions only if needed) | atomic-builder | ~1–2 | `npm run build` 0 type errors; manual verify: debounce fires, results render, scroll loads next page, All tab shows two columns, CF key absent shows "API key required" copy |

## Risks

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| CF `gameVersions[]` mixes MC versions and loader names with no flag — split heuristic can misclassify | med | Parse rule: entries matching `/^\d+\.\d+/` are MC versions; known loader name strings (Forge, NeoForge, Fabric, Quilt, etc.) are loader tags; unknown entries discarded. Unit-tested against the CF fixture in CP3. |
| Modrinth rate limit under fast typing | med | ≥ 300 ms debounce; note in CP5 manual verify. Backend rate-limit layer deferred (open question in `docs/design/providers.md`). |
| CF Quilt coverage is thin — empty result indistinguishable from broken | low | UI empty state copy is provider-generic; CF Quilt empty list renders as expected "no results" (not an error). Flagged in design open questions. |
| `ProviderHttpClient` seam vs `reqwest` ergonomics — response streaming not needed for browse, but shape must not preclude it for Phase 6 | low | Seam returns `bytes::Bytes` (or text) per request; no streaming in slice A. Phase 6 can extend if needed. |
| `ipc.ts` drift from Rust normalized types | ongoing | Manual mirror, same as existing structs. `specta`/`ts-rs` generation deferred to cross-cutting roadmap item; drift is the known risk. |

## Change log

<!-- new entries prepended below this line: ### YYYY-MM-DD — <title> -->
