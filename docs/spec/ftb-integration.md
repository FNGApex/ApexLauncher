# Spec: FTB (Feed The Beast) modpack integration

> Branch: TBD (`feat/ftb-integration`)
> Design: `docs/design/ftb-integration.md`
> Build/test ONLY via `scripts/build.sh` (`check`, `test [filter]`, `dev`). Tests live in
> sibling `<stem>_tests.rs` files (CLAUDE.md convention) with **canned FTB JSON fixtures + a
> mock `ProviderHttpClient`** — no live network in unit tests (mirror `curseforge_tests.rs`).
> DTO/command/event changes require regenerating `src/lib/bindings.ts` via `scripts/build.sh dev`
> (wait for `[bindings] exported`, stop) — never hand-edit `ipc.ts`.

Each checkpoint ends **runnable** (`scripts/build.sh check` green, named tests pass, app builds).
Sequence: provider client → install planner → command/job wiring → Browse UI enablement →
pack update-check.

FTB API ground truth (verified live, see design evidence trail): base `https://api.modpacks.ch`,
**keyless**; list endpoints return id-arrays (N+1 detail fetch); version manifest `files[]` are
FTB-CDN (`url`) **XOR** CF-referenced (`curseforge:{project,file}`, no url); `targets[]` give
loader/mc/java; `specs.recommended` gives RAM (MB). **Install requires the CF API key** (jars are
CF-hosted) and reuses the CF manual-download (`pending_manual`) pipeline.

---

## Checkpoint table

| CP | Goal | Files touched | Tests to add (sibling `<stem>_tests.rs` + fixtures) | bindings regen? | Runnable gate |
|----|------|---------------|------------------------------------------------------|-----------------|---------------|
| **CP-1** | `FtbProvider` client + `ProviderKind::Ftb` | `src-tauri/src/core/providers.rs` (`ProviderKind::Ftb`, serde `"ftb"`); **new** `src-tauri/src/core/ftb.rs` (`FtbProvider` unit struct; raw deser types `FtbListResponse`/`FtbModpackDetail`/`FtbVersionEntry`/`FtbVersionManifest`/`FtbFile`/`FtbTarget`/`FtbArt`/`FtbAuthor`/`FtbSpecs`; URL builders `build_featured_url`/`build_popular_url`/`build_search_url`/`build_detail_url`/`build_version_url`; `impl ModProvider`); `src-tauri/src/core/mod.rs` (register `pub mod ftb;`); **new** `src-tauri/src/core/ftb_tests.rs` + `#[path]` stub | **new** `ftb_tests.rs`: `search` (empty query→featured ids→N detail→`ProjectSummary[]`; term query→`/search` arm; ignores `curseforge[]` echo); `get_project`→`PackInfo{body_is_html:false}`; `get_pack_summary`→name/icon(square art)/author(authors[0])/tags; `get_versions`→`ProjectVersion[]` with mc+loader from `targets[]`, **empty `files`**; `get_projects_brief`→`Ok(vec![])`; loader-name mapping (`forge`/`neoforge`/`fabric`/`quilt`); version `type` case-insensitive (`"release"`/`"Release"`); art square-pick; all via mock client + fixtures (NO key header sent). `providers_tests.rs`: `ProviderKind::Ftb` serializes `"ftb"`; `Box<dyn ModProvider>` still object-safe with FtbProvider | **Yes** — `ProviderKind` is a generated DTO (`specta::Type`). Regen `bindings.ts`; confirm `type ProviderKind = "modrinth" \| "curseForge" \| "ftb"` | `build.sh check` + `build.sh test core::ftb` + `build.sh test core::providers` green; `bindings.ts` regenerated |
| **CP-2** | FTB install planner (pure + async seam) | `src-tauri/src/core/modpack.rs` (`FtbPackPlan { items, mods, manual, skipped, failed }`; `build_ftb_pack_plan(manifest, resolved_cf, mc_dir)` pure; `resolve_and_build_ftb_plan(http, cf_provider, manifest, mc_dir)` async seam calling `CurseForgeProvider::get_file` for the CF-referenced subset; `ftb_dest_path` helper = strip `./` from `path` + join `name`, `validate_relative_path`-guarded) | `modpack_tests.rs`: FTB-hosted file (url+sha1)→`DownloadItem` at `path`+`name`, `type=="mod"`→also `ModEntry{provider:"ftb"}`, non-mod (config/script)→`DownloadItem` only; CF-referenced (no url, `curseforge{}`) resolved with url+sha1→`DownloadItem`+`ModEntry{provider:"curseforge"}`; CF-referenced resolving to url=None→`CfManualFile` (reuses pending pipeline); `serveronly==true`→skipped; `optional` included; `path` traversal (`../`, absolute, leading `\`) rejected via `validate_relative_path`; `resolve_and_build_ftb_plan` calls `get_file` once per CF entry (mock client) and dedups by (project,file). Fixture: `ftb_version_manifest.json` (mixed FTB-hosted + CF-referenced + serveronly + manual) | **No** (internal types only — `FtbPackPlan`/`DownloadItem`/`CfManualFile`/`ModEntry` not new IPC DTOs) | `build.sh check` + `build.sh test core::modpack` green |
| **CP-3** | Command + `ImportFtbJob` wiring | `src-tauri/src/lib.rs`: **new** `ImportFtbJob` (TaskJob — fetches/holds manifest, runs `resolve_and_build_ftb_plan`, `remap_to_staging`→`execute_plan_cancellable`→`promote_staging`, writes `ModEntry[]` + `instance.pending_manual` like `ImportCfZipJob`, returns a `CfImportResult`); `install_modpack` gains `"ftb"` arm (fetch detail→resolve latest/selected version→fetch version manifest→build `Source{provider:"ftb",…,recommended:specs.recommended}`→enqueue `ImportFtbJob`, **bypassing** the archive-download path at `lib.rs:3256-3277`); `"ftb"` arms in `search_mods` (1338), `get_mod_versions` (1386), `get_pack_info` (1425), `refresh_pack_meta` (1482, also accept stored provider `"ftb"`); `enqueue_import_ftb` helper | `lib_tests.rs` (where logic is extractable): `install_modpack` `"ftb"` arm selects newest release version when `version_id=None`; `Source` provenance carries `provider:"ftb"` + `recommended` from `specs`; manual entries land in `pending_manual`. Provider-dispatch arms: unknown→error unchanged. (Heavy job I/O stays integration-style; keep unit-testable helpers pure.) | **No** new DTO/command/event — `install_modpack`/`search_mods`/etc. signatures unchanged, `CfImportResult` reused, `ProviderKind::Ftb` already regenerated at CP-1. (Verify no new generated type slipped in; if a helper DTO is added, regen.) | `build.sh check` + `build.sh test` (full Rust suite) green; manual smoke: `installModpack("ftb", "<id>")` enqueues a task that installs a launchable FTB instance (with a CF key set) |
| **CP-4** | Browse UI enablement (frontend union-widening) | `src/lib/store.ts:118-129` (`browseProvider` union +`"ftb"`); `src/routes/Browse.tsx:55-57,104-114` (drop `ftb` from "coming soon" guard → real provider; reduced/empty `FiltersPopover` for FTB; featured+popular default grid, term search, no infinite scroll); `src/components/Sidebar.tsx:286-295` (FTB item → `<NavLink to="/browse/ftb">`); `src/components/BrowseCard.tsx:22-27` (`ftb` wire→routing arm); `src/components/ProviderBadge.tsx:14-15` (FTB label+color); `src/routes/BrowsePackInfo.tsx:50,64` (`ftb` arm); `src/routes/InstanceDetail.tsx:387-392,614-617` + `InfoTab.tsx:113-117` (`ftb` source→routing arms); `src/lib/ipc.ts:234,263,272-282` (widen param unions); `src/lib/installedIndex.ts:7-10` (`ftb` lowercase key); `src/lib/categoryMap.ts:70-79` + `FiltersPopover.tsx:41` (widen; FTB category branch may no-op); **leave** `InstanceDetail.tsx:956,1037-1048` ModlistTab per-mod dropdown 2-valued (FTB pack-only) | No frontend test harness yet (planned Phase 7) — visual/manual verification | **No** (frontend only; `ProviderKind` already in `bindings.ts` from CP-1) | `build.sh check` (tsc) green; sidebar FTB navigates to `/browse/ftb`; grid loads featured/popular packs; pack detail + version modal install; "Installed" pills via `installedIndex` |
| **CP-5** | Pack update-**check** (in scope) | `src-tauri/src/lib.rs` `refresh_pack_meta` `"ftb"` arm: `GET /public/modpack/{id}`→newest release version id+name→write `latest_version`/`latest_version_id`/`last_update_check`; throttled by existing `needs_update_check` (24h). Frontend: existing update banner already reads these manifest fields — no UI change beyond CP-4 | `lib_tests.rs` / `ftb_tests.rs`: newest-release-version selection from `versions[]` (case-insensitive `type`, max by id); update-available when stored `file_id != latest_version_id`; throttle respected | **No** (reuses existing `PackMetaRefresh` DTO + `refresh_pack_meta` command) | `build.sh check` + `build.sh test` green; an installed FTB instance shows "update available" when a newer version exists |

---

## Per-checkpoint detail

### CP-1 — `FtbProvider` (bindings regen)
- `FtbProvider` is a **unit struct** — no `api_key` field (FTB public API is keyless). Send a
  descriptive `User-Agent` header on every `client.get(url, &[("User-Agent", "ApexLauncher/<ver> (contact)")])`
  per the Modrinth etiquette convention; no `x-api-key`.
- URL builders are static `fn`s over `const BASE: &str = "https://api.modpacks.ch"` (mirror
  `curseforge.rs:31-46, 361-431`). Endpoints: `/public/modpack/featured/{limit}`,
  `/public/modpack/popular/installs/{limit}`, `/public/modpack/search/{limit}?term=`,
  `/public/modpack/{id}`, `/public/modpack/{id}/{versionId}`.
- `search`: empty `params.query` → featured (and/or popular) id list; non-empty → `/search`.
  Take `packs[]`, fetch each `/public/modpack/{id}` → `ProjectSummary`. Ignore `curseforge[]`.
  Honor `params.limit`/`offset` against the bounded id list (no real server pagination).
- `get_versions` returns `ProjectVersion` per `versions[]` with `files: vec![]` and
  `dependencies: vec![]` — install does not use these files (Decision 2); the version-select
  modal uses `id`/`name`/`game_versions`/`loaders`.
- Loader mapping: `targets[]` entry with `type=="modloader"` → `.name` lowercased
  (`forge`/`neoforge`/`fabric`/`quilt`). MC = `type=="game"` entry `.version`.
- Test wiring: end `ftb.rs` with `#[cfg(test)] #[path = "ftb_tests.rs"] mod tests;`.

### CP-2 — Install planner (no regen)
- `ftb_dest_path(path: &str, name: &str) -> String`: trim a leading `"./"` (and any `"/"`)
  from `path`, join `name`, return a forward-slash relative path; feed `validate_relative_path`
  (already rejects `..`, absolute, drive-letter, `\`-prefix — `modpack.rs`).
- `build_ftb_pack_plan` mirrors `build_cf_pack_plan` (`modpack.rs:489-566`) but:
  - dest comes from `ftb_dest_path(file.path, file.name)`, not a hardcoded `mods/`.
  - a `ModEntry` is recorded only for `file.type == "mod"`; other types are pure
    `DownloadItem`s (the FTB analogue of mrpack `overrides/`).
  - FTB-hosted files use `file.sha1` directly; CF-referenced files use the resolved
    `VersionFile` (from `get_file`) for url+sha1, falling back to `CfManualFile` when url=None
    (reuse `cf_file_page_url` + `expected_sha1`/`size`).
  - `serveronly == true` → skip. `optional` → include (v1; O-4).
- `resolve_and_build_ftb_plan` is the async seam (mirror `resolve_and_build_cf_plan`): collect
  the CF-referenced `(project,file)` set, `get_file` each (dedup), pass `(FtbFile, VersionFile)`
  pairs + the FTB-hosted files to the pure builder. Injectable `ProviderHttpClient` → mock-tested.

### CP-3 — Command + job (no regen expected)
- `ImportFtbJob` mirrors `ImportCfZipJob`: same staging dir (`staging_dir_for`), same
  `remap_to_staging`/`promote_staging`, same `pending_manual` write, returns `CfImportResult`.
  Difference: its plan comes from `resolve_and_build_ftb_plan` over a held manifest instead of
  a parsed zip.
- `install_modpack` `"ftb"` arm (diverges from `lib.rs:3239-3315`): no archive fetch. Resolve
  the version (newest release when `version_id` is `None`), fetch the version manifest, build
  `Source { provider:"ftb", project_id, file_id:versionId, pack_version, recommended:specs.recommended, icon_url, author, … }`, then `enqueue_import_ftb(...)`.
- Construct `FtbProvider` (no key) in the `search_mods`/`get_mod_versions`/`get_pack_info`/
  `refresh_pack_meta` `"ftb"` arms — no `cf_api_key_from`. (The CF key is only needed **inside**
  `ImportFtbJob` to resolve CF-referenced files; resolve it there via the existing
  `cf_api_key_from(env, settings, baked)`.)

### CP-4 — Browse UI (no regen)
- Widen the union at every site in the design's ripple table. FTB Browse: no server-side loader/
  category facets → render `FiltersPopover` in a reduced/empty mode (or hide it) for `ftb`.
- Featured+popular default grid (dedup ids), term search filters that set. No `useInfiniteQuery`
  fan-out for FTB (bounded catalog).
- Sidebar: replace the static FTB "coming soon" div (`Sidebar.tsx:286-295`) with a `<NavLink>`
  mirroring the CurseForge/Modrinth items (`Sidebar.tsx:263-284`). ATLauncher stays static.

### CP-5 — Update-check (no regen)
- `refresh_pack_meta` `"ftb"` arm reuses `needs_update_check` throttle + `PackMetaRefresh`
  return. Update-**apply** is **out of v1 scope** (O-3) — do not wire FTB into `update_modpack`
  in this pass.

---

## Fixtures to add (`src-tauri/src/core/fixtures/`)
- `ftb_featured.json` — `{ packs:[…ids], total, limit }` (list-response shape).
- `ftb_search.json` — `{ packs:[…], curseforge:[…], total }` (term-search; exercise the
  `curseforge[]`-ignored path).
- `ftb_modpack_detail.json` — full `/public/modpack/{id}` (name, synopsis, description,
  art[square], authors[], tags[], versions[] with mixed-case `type`).
- `ftb_version_manifest.json` — `/public/modpack/{id}/{versionId}` with: ≥1 FTB-hosted mod
  (url+sha1), ≥1 FTB-hosted config (non-mod), ≥1 CF-referenced normal file, ≥1 CF-referenced
  manual (resolves to url=None), ≥1 `serveronly` file, `targets[]` (forge or neoforge + mc +
  java), `specs{recommended}`. (Trim a real live response to keep it small.)

## Test inventory delta (expected)
- `ftb_tests.rs` (new): +~10 (search empty/term, get_project, get_pack_summary, get_versions
  mc+loader+empty-files, get_projects_brief no-op, loader/art/type-case mapping).
- `providers_tests.rs`: +~2 (`ProviderKind::Ftb` serde, object-safety with FtbProvider).
- `modpack_tests.rs`: +~7 (FTB-hosted mod/config, CF-referenced normal/manual, serveronly skip,
  optional include, path-traversal reject, single-get_file-per-CF-entry).
- `lib_tests.rs`: +~3 (version selection, Source provenance + recommended, pending write) where
  extractable.
- No frontend tests (none exist yet; planned Phase 7).

## Regeneration checklist (bindings.ts)
Regen required at **CP-1 only** (`ProviderKind` gains `"ftb"`). CP-2/CP-3/CP-4/CP-5 touch no
generated DTOs/commands/events (verify at CP-3 that no helper DTO slipped in). Regen:
`scripts/build.sh dev` → wait for `[bindings] exported` → stop → commit the regenerated
`src/lib/bindings.ts` alongside the CP-1 Rust change.

## Open questions (carried from design — resolve before execution)
- **O-1** Browse default/pagination (featured+popular top-N, no infinite scroll?).
- **O-2** CF-key gating UX for FTB install (fail-on-hit vs proactive disable+tooltip).
- **O-3** Update scope (check-only v1 vs check+apply).
- **O-4** `optional` files included by default? (v1: yes.)
- **O-5** Populate `Source.recommended` from `specs.recommended`? (v1: yes.)

## Change log
- 2026-06-26 — Initial spec authored (design `docs/design/ftb-integration.md`). Not implemented.
  FTB API verified live (keyless `api.modpacks.ch`; id-array lists + N+1 detail; FTB-CDN XOR
  CF-referenced files; install requires CF key + reuses `pending_manual`). 5 checkpoints;
  bindings regen at CP-1 only (`ProviderKind::Ftb`).
- 2026-06-26 — Open questions resolved (all v1 defaults adopted): **O-1** featured+popular +
  term search, no infinite scroll; **O-2** fail-on-hit (reuse the existing CF key-missing
  prompt; no FTB-specific gating); **O-3** update **check-only** (defer apply; do not wire FTB
  into `update_modpack`); **O-4** include `optional` files; **O-5** populate `Source.recommended`
  from `specs.recommended`. Ready for execution.
- 2026-06-26 — **Implemented CP-1…CP-5.** `core/ftb.rs` (FtbProvider, manifest types,
  newest-release selection); `ProviderKind::Ftb` (bindings regenerated at CP-1); `modpack.rs`
  FTB planner (`build_ftb_pack_plan`/`resolve_and_build_ftb_plan`, reuses CF resolution +
  `pending_manual`); `install_modpack` ftb arm + `ImportFtbJob`; `"ftb"` arms in
  search/get_versions/get_pack_info/refresh_pack_meta; Browse UI enabled (Sidebar NavLink,
  union widening across ~14 sites, filters hidden for FTB, instance source routing);
  `update_modpack` returns a clear unsupported message for ftb (check-only). `build.sh check`
  + full suite green (701 lib tests; +16 FTB: 11 ftb + 5 modpack planner). Single bindings
  regen at CP-1. Runtime-only paths (live FTB browse/install, asset-less manifest download with
  a real CF key) not smoke-tested headlessly — flagged for a manual/`dev` check.
