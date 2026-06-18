# Providers browse (Phase 5, slice A)

## Goal

Expose a unified modpack discovery feed from both Modrinth and CurseForge on a single
Browse page. Results from both providers are merged client-side and sorted by downloads
descending; each card is provider-badged. Clicking a card opens the project page in the
system browser — there is no add-to-instance affordance on Browse. Provider discovery
uses `project_type = "modpack"` for both `searchMods` calls.

## Non-goals

- Add-to-instance from Browse (moved to per-instance slide-over; `docs/spec/mod-install.md`).
- Mod (non-modpack) search on Browse.
- Modpack import (`.mrpack`, CF zip) — Phase 6.
- Resource packs, shaders, datapacks (other `classId` / `project_type` values).
- Jar fingerprint reconciliation of manually added mods (Murmur2 / sha512 lookup).
- Provider account auth (CF/Modrinth user accounts).
- Disk-caching of search results (TanStack Query's 30 s `staleTime` is sufficient; rate-limit
  TTL on the version list is a follow-up risk, not built now).

## Success criteria

- [x] `ModProvider` trait compiles; both Modrinth and CurseForge implementations satisfy it
      with a mock-injectable HTTP seam (same async_trait pattern as `AuthHttpClient`,
      `auth.rs:226`).
- [x] `search_mods` Tauri command accepts an optional `project_type` selector (`"mod"` |
      `"modpack"`, default `"mod"`); Modrinth uses `project_type:<value>` facet, CurseForge
      uses `classId` `6` (mod) or `4471` (modpack). Existing mod searches unchanged when the
      selector is omitted or set to `"mod"`.
- [x] `ProjectSummary` carries `page_url` (IPC camelCase `pageUrl`): Modrinth =
      `https://modrinth.com/{project_type}/{slug}` (derived from the response hit's
      `project_type` field, not the selector); CurseForge = `links.websiteUrl` verbatim;
      `Option` — `null` when absent.
- [x] `get_mod_versions` Tauri command returns `Vec<ProjectVersion>` filtered to versions
      compatible with the supplied MC version + loader.
- [x] CF `downloadUrl: null` maps to `VersionFile.url = None` (not an error); the field is
      serialized to the frontend as `null`.
- [x] CF key absent: `search_mods` for the `curseforge` provider returns a
      `ProviderCommandError { kind: "key_missing", … }` rather than a panic or an
      untyped string error.
- [x] All new Rust tests pass under `cargo test`; no live HTTP calls in any test (all
      responses fixture-backed via the injectable seam).
- [x] `cargo check` produces 0 errors.
- [x] `ipc.ts` mirrors new param (`projectType: ProjectType = "mod"` on `searchMods`) and
      new field (`pageUrl: string | null` on `ProjectSummary`); existing callers still
      typecheck (new param has a default).
- [x] Browse page: typing in the search box fires a debounced (≥ 300 ms; 400 ms target per
      design) query; results render as cards; infinite scroll loads the next page on
      scroll-to-bottom for both providers.
- [x] Browse page: MC version + loader facet selectors are visible and wired to the query
      parameters.
- [x] Browse page: shows unified modpack feed (downloads-desc merge of both providers,
      provider-badged cards, click opens `pageUrl` in system browser; no add-to-instance
      affordance). `project_type = "modpack"` passed to both `searchMods` calls.
- [x] Browse page: when CF key is absent, CF results are hidden with an inline dismissible
      notice; Modrinth results still render.

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
| 5 | **Browse UI — unified modpack feed.** Replace the `Browse.tsx` stub with a live unified modpack discovery feed: debounced search input (≥ 300 ms; 400 ms target), MC version + loader facet selectors (populated from `listMinecraftVersions` / `getLoaders`), two independent `useInfiniteQuery` calls (`project_type = "modpack"`), client-side merge sorted by downloads desc, provider-badged cards (icon, name, summary, download count). Click opens `pageUrl` in the system browser; no add-to-instance affordance. CF key absent: inline dismissible notice; Modrinth results still render. CF non-key error: inline error notice. | `src/routes/Browse.tsx`, `src/lib/ipc.ts` (additions only if needed) | atomic-builder | ~1–2 | `npm run build` 0 type errors; manual verify: debounce fires, merged results render downloads-desc, scroll loads next page on both providers, CF key absent shows inline notice without blanking Modrinth |

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

### 2026-06-16 — Browse unified modpack feed

**What changed:**
- Browse is now a single ordered modpack discovery feed: both providers searched with
  `project_type = "modpack"`, results merged client-side and sorted by downloads descending,
  each card provider-badged.
- Clicking a card opens the provider page in the system browser (`openUrl`). There is no
  add-to-instance affordance on Browse — that interaction lives in the per-instance slide-over.
- CF key absent: inline dismissible notice is shown; Modrinth results continue to render
  (no blank-out of the feed).
- CF non-key error: separate inline error notice.
- Goal, Non-goals, CP5 checkpoint description, and success criteria updated to reflect this
  contract.

**Why:** CP3 deliverable of the ui-modpack-rework spec. Browse's purpose shifted from general
mod browsing with per-tab columns to focused modpack discovery with a single merged feed.
The add-to-instance affordance was removed from Browse to separate discovery (Browse) from
installation (per-instance slide-over).

**Superseded:** Browse had provider tabs (All / Modrinth / CurseForge) showing side-by-side
columns of mod (not modpack) results, with an add-to-instance modal accessible from each card.

### 2026-06-16 — project-type selector + `page_url` on `ProjectSummary`

**What changed:**
- `search_mods` Tauri command gains an optional `project_type: Option<ProjectType>` parameter
  (default `"mod"`). Modrinth maps it to the `project_type` facet; CurseForge maps it to
  `classId` (`6` for mod, `4471` for modpack).
- `ProjectSummary` gains a `page_url: Option<String>` field (IPC `pageUrl`). Modrinth
  populates it from `https://modrinth.com/{response_hit.project_type}/{slug}`. CurseForge
  populates it from `links.websiteUrl` in the search row; `None` when absent/null.
- `ProjectType` enum (`Mod | Modpack`) added to `core/providers.rs`; `#[derive(Default)]`
  defaults to `Mod`.
- `MrHit` deserialization struct gains `project_type: String`; `CfMod` gains `links:
  Option<CfLinks>` / `CfLinks.website_url`.
- `ipc.ts`: `ProjectType` type alias added; `searchMods` signature adds
  `projectType: ProjectType = "mod"` (optional, back-compat for existing callers); `pageUrl:
  string | null` added to `ProjectSummary` interface.
- Success criteria updated to reflect the new contract and mark shipped items.

**Why:** CP1 of the UI/modpack-rework spec (ui-modpack-rework.md). Browse becomes a unified
modpack discovery feed; the provider layer needs to switch facets/classId per project type,
and cards need a `page_url` to open in the system browser.

**Superseded:** `classId` and Modrinth `project_type` facet were hardcoded to mods only.
`ProjectSummary` had no `page_url` field.

### 2026-06-11 — search_mods returns SearchResult, not bare Vec

**What changed**: success criterion for `search_mods` now specifies `SearchResult`
(hits + offset + total) as the return shape.

**Why**: the spec's own pagination contract ("normalize to offset/limit/total",
Recommendation table) requires `total` to reach the UI for infinite scroll; a bare
`Vec<ProjectSummary>` drops it. CP1 already shipped `SearchResult` as the normalized
shape. Caught at CP4 brief-writing.

**Superseded**: "`search_mods` returns `Vec<ProjectSummary>`".

## Implementation log

### shipped (code-complete; manual UI verify pending) — 2026-06-11

Built across 7 iterations of /subagent-implementation. Commits (chronological):

- `97ec5d9` — CP1 normalized types, ModProvider trait, HTTP seam (21 tests)
- `d05b8f4` — CP2 Modrinth client: faceted search + filtered versions (15 tests)
- `0a21572` — CP3 CurseForge client: x-api-key, gameVersions split, url:None (23 tests)
- `1bea547` — CP4 Tauri commands + ipc.ts mirrors (2 rounds: kind-string tests were missing; 4 tests)
- `8ef2fa6` — CP5 live Browse page (2 rounds: `as`-cast removal per repo TS rule)
- `d2479e4` — polish pass closing review ledger F-2..F-9 (user fix-now disposition; 4 tests)

Suite: 226 → 293 Rust tests; `npm run build` clean throughout.

**Out-of-scope work performed during this build:**
- none

**Unforeseens — surprises that emerged during implementation:**
- Spec self-contradiction caught at CP4: success criterion said `Vec<ProjectSummary>` while the pagination contract needs `total` — amended to `SearchResult` (see Change log).
- No frontend test infrastructure exists; CP5 verified by tsc + build + line review instead of TDD.
- CP4 round 1 shipped zero tests for the IPC kind strings; reviewer blocked, round 2 added them and a distinct `unknown_provider` kind.

**Deferred items still open:**
- none deferred. Dropped at triage (reason: premature at current scale, revisit if Browse shows latency in manual verify): F-7 reqwest client per command call, F-10 IntersectionObserver re-attach churn, F-11 error/loading branch order.
- Verification: manual UI verify in `npm run tauri dev` (debounce, infinite scroll, All-tab columns, CF key-missing state). The CurseForge API key is available (env / `settings.curseforge_api_key` / baked tier), so live CF testing is unblocked — see the curseforge-live-testing notes.
