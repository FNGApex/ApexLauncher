# Spec: ATLauncher modpack integration

> Branch: TBD (`feat/atlauncher-integration`)
> Design: `docs/design/atlauncher-integration.md`
> Build/test ONLY via `scripts/build.sh` (`check`, `test [filter]`, `dev`). Tests live in
> sibling `<stem>_tests.rs` files (CLAUDE.md convention) with **canned ATLauncher JSON
> fixtures + a mock `ProviderHttpClient`** — no live network in unit tests (mirror
> `ftb_tests.rs` / `curseforge_tests.rs`). DTO/command/event changes require regenerating
> `src/lib/bindings.ts` via `scripts/build.sh dev` (wait for `[bindings] exported`, stop) —
> never hand-edit `ipc.ts`. **Bindings regen on Windows gotcha:** start the dev window, wait
> for `[bindings] exported`, stop it, then kill any stray dev/cargo processes and free TCP
> port 1420 before the next build (see the project's bindings-regen memory).

Each checkpoint ends **runnable** (`scripts/build.sh check` green, named tests pass, app
builds). Sequence: provider client → MD5 hash support → install planner → command/job wiring →
Browse UI → update-check.

## ATLauncher API ground truth (verified live, see design evidence trail)

- API base `https://api.atlauncher.com/v1`; CDN base `https://download.nodecdn.net/containers/atl`.
- **A non-empty `User-Agent` is mandatory** (Cloudflare 1020 otherwise). **Keyless** (no CF key).
- Standard envelope `{ error, code, message, data }` on every API response.
- `GET /packs/full/public` → whole catalog (145 packs) in ONE call; each pack has
  `id, name, safeName, type, versions[{version, minecraft, published, __LINK}], description`.
- Provider id = **`safeName`** (string); version id = **free-form version string** (URL-encode).
- `GET /pack/{safeName}` (full versions + description); `GET /pack/{safeName}/{version}` and
  `/pack/{safeName}/latest` → version summary `{version, minecraftVersion, recommended, …}`.
- **Mod list is NOT in the API** — Configs.json manifest at
  `{CDN}/packs/{safeName}/versions/{version}/Configs.json`:
  `{ version, minecraft, loader{type,metadata}, mods[], configs{filesize,sha1}, memory? }`.
- **Mods are SELF-HOSTED** — `download ∈ {server, browser, direct}`; **keyless install, no CF**.
  `server` ⇒ `{CDN}/{url}`; `direct` ⇒ absolute `url`; `browser` ⇒ manual (rare).
- **Mods carry only `md5`** → needs `ExpectedHash::Md5`. `Configs.zip` uses `sha1` (engine-native).
- Config overrides = one `Configs.zip` at `{CDN}/packs/{safeName}/versions/{version}/Configs.zip`,
  extracted **root-relative** into the instance mc dir.
- Icon = `{CDN}/launcher/images/{safeName-lowercase}.png` (deterministic; no fetch).
- `manifest.memory` (int MB, optional) → `Source.recommended`. Version `recommended` is a bool.

---

## Checkpoint table

| CP | Goal | Files touched | Tests to add (sibling `<stem>_tests.rs` + fixtures) | bindings regen? | Runnable gate |
|----|------|---------------|------------------------------------------------------|-----------------|---------------|
| **CP-1** | `AtlProvider` client + `ProviderKind::Atlauncher` | `src-tauri/src/core/providers.rs:209-213` (`ProviderKind::Atlauncher`, serde `"atlauncher"`); **new** `src-tauri/src/core/atl.rs` (`AtlProvider` unit struct; raw deser `AtlEnvelope<T>`/`AtlPack`/`AtlPackVersion`/`AtlConfigsManifest`/`AtlLoader`/`AtlMod`; builders `api_packs_public_url`/`api_pack_url`/`api_pack_version_url`/`cdn_configs_url`/`cdn_configs_zip_url`/`cdn_file_url`/`cdn_image_url`; `impl ModProvider`; `newest_version`/`get_configs_manifest` pub helpers for CP-3/CP-4); `src-tauri/src/core/mod.rs:27` (`pub mod atl;`); **new** `src-tauri/src/core/atl_tests.rs` + `#[path]` stub | **new** `atl_tests.rs`: `search` (one `/packs/full/public` call via mock; client-side substring filter; offset/limit window; envelope unwrap; maps to `ProjectSummary` with `id=safeName`, deterministic `icon_url`); `get_versions`→`ProjectVersion[]` from `versions[]` with `game_versions=[minecraft]`, **empty loaders+files**; `get_project`→`PackInfo{body_is_html:false}`; `get_pack_summary`→name/icon/first-line-summary; `get_projects_brief`→`Ok(vec![])`; `cdn_image_url` lowercases safeName; envelope `error:true` → `ProviderError`. `providers_tests.rs`: `ProviderKind::Atlauncher` serializes `"atlauncher"`; `Box<dyn ModProvider>` object-safe with AtlProvider | **Yes** — `ProviderKind` is `specta::Type`. Regen `bindings.ts`; confirm `type ProviderKind = "modrinth" \| "curseForge" \| "ftb" \| "atlauncher"` | `build.sh check` + `build.sh test core::atl` + `build.sh test core::providers` green; `bindings.ts` regenerated |
| **CP-2** | MD5 hash support in the download engine | `src-tauri/src/core/download.rs:27-33` (`ExpectedHash::Md5(String)`), `:199-228` (`IncrementalHasher::Md5(md5::Md5)` + new/update/finalize arms), `:267, :480` (the `Sha1\|Sha256\|Sha512` match arms gain `\| Md5`); `src-tauri/Cargo.toml` (`md-5 = "0.10"` dep) | `download_tests.rs`: an `ExpectedHash::Md5` item verifies against a known-good md5 and **fails** on a wrong md5 (mirror the existing sha1 verify/mismatch tests); `.part` resume + TOCTOU path unaffected | **Yes** — CORRECTION: `ExpectedHash` IS surfaced in `bindings.ts` (via `DownloadItem`/`DownloadPlan` → `execute_download_plan` command). Stale bindings compile fine (no TS constructs `Md5`) but regen keeps the generated source truthful. Regen the `ExpectedHash` union to gain `{type:"md5"}` | `build.sh check` + `build.sh test core::download` green; `bindings.ts` `ExpectedHash` has `md5` |
| **CP-3** | ATL install planner (pure) + configs extraction | `src-tauri/src/core/modpack.rs`: `AtlPackPlan { items, mods, manual, skipped }`; `atl_dest_path(&AtlMod) -> Result<String,_>` (honor `path`, else `type`→folder map, append `file`; `validate_relative_path`-guarded; legacy/exotic types → `Err`/skip); `build_atl_pack_plan(manifest, cdn_base, mc_dir) -> Result<AtlPackPlan,_>` (pure: `server`→`{cdn}/{url}`+`Md5`; `direct`→abs url+`Md5`; `browser`→`CfManualFile`-shaped manual with `page_url=url`, ids `0`; `mods` type→`ModEntry{provider:"atlauncher"}`; skip `client==false`; include `optional`); `extract_atl_configs(archive, mc_dir)` (root-relative, zip-slip-safe; reuse `extract_prefix(_, _, "")` or thin wrapper over `validate_relative_path`+`is_safe_dest`) | `modpack_tests.rs`: `server` mod→`DownloadItem{url=cdn+url, Md5}` at correct folder + `ModEntry{provider:"atlauncher"}`; `direct` mod→abs-url item; `browser` mod→manual entry; `resourcepack`→`resourcepacks/`, `shaderpack`→`shaderpacks/`; `path` override honored; `client==false`→skipped; `optional` included; legacy type (`decomp`)→skipped; path traversal (`../`, absolute) rejected; `extract_atl_configs` extracts root-relative + rejects zip-slip. Fixture: `atl_configs.json` (mixed server/direct/browser, mods+resourcepack, optional, client-false, legacy) | **No** (internal types only) | `build.sh check` + `build.sh test core::modpack` green |
| **CP-4** | Command + `ImportAtlJob` wiring | `src-tauri/src/lib.rs`: **new** `ImportAtlJob` (TaskJob — holds manifest + meta; `build_atl_pack_plan`→`remap_to_staging`→`execute_plan_cancellable` (mods + `Configs.zip`)→`extract_atl_configs`→`promote_staging`; writes `ModEntry[]` + any `browser` files to `instance.pending_manual`; returns `CfImportResult`); `enqueue_import_atl`; `install_atl_modpack` (resolve version via `/latest` or selected→fetch Configs.json→build `Source{provider:"atlauncher", project_id:safeName, file_id:version, pack_version:version, recommended: memory>0 ? RecommendedJava{memory_mb} : None, icon_url, name}`→`enqueue_import_atl`); `install_modpack` `"atlauncher"` arm (`lib.rs:3543` neighbor, **bypasses** archive path); `"atlauncher"` arms in `search_mods` (1377), `get_mod_versions` (1418), `get_pack_info` (1461), `refresh_pack_meta` (1557); `update_modpack` `"atlauncher"` → "not supported yet" (3959 neighbor); loader_kind/version from `manifest.loader` (forge/neoforge→`metadata.version`, fabric/quilt→`metadata.loader`) | `lib_tests.rs` (where extractable): `install_atl_modpack` selects latest when `version_id=None`; `Source` provenance carries `provider:"atlauncher"` + `recommended` from `memory`; loader mapping (fabric→`metadata.loader`, neoforge→`metadata.version`); `browser` files land in `pending_manual`. (Heavy job I/O integration-style; keep helpers pure.) | **No** new DTO/command/event — signatures unchanged, `CfImportResult` reused, `ProviderKind::Atlauncher` already regenerated at CP-1. (Verify no helper DTO slipped in; if so, regen.) | `build.sh check` + `build.sh test` (full suite) green; manual smoke (`build.sh dev`): `installModpack("atlauncher","AllTheForge10")` enqueues a task that installs a launchable instance **with no CF key set** |
| **CP-5** | Browse UI enablement (frontend union-widening) | Widen `\| "atlauncher"` at every site in the design's ripple table: `src/lib/store.ts:118-119`; `src/lib/ipc.ts:234,263,272,284`; `src/routes/Browse.tsx:55,61,107,114-117,244` (drop ATL from coming-soon guard; `supportsFilters` excludes it; client-side-searched single grid); `src/routes/BrowsePackInfo.tsx:51,65-66,122,215`; `src/routes/InstanceDetail.tsx:387-392,616-621`; `src/routes/instance-tabs/InfoTab.tsx:113-120`; `src/components/Sidebar.tsx:300-309` (stub→`<NavLink to="/browse/atlauncher">`); `src/components/ProviderBadge.tsx:15,27,33` (arm+label+color); `src/components/BrowseCard.tsx:22,27`; `src/components/FiltersPopover.tsx:41`; `src/lib/categoryMap.ts:70`. **Leave** the ModlistTab per-mod add dropdown 3-valued (ATL is pack-only). No change: `installedIndex.ts`, `router.tsx` | No frontend test harness yet (planned Phase 7) — visual/manual verification | **No** (frontend only; `ProviderKind` already in `bindings.ts` from CP-1) | `build.sh check` (tsc) green; sidebar ATLauncher → `/browse/atlauncher`; grid loads catalog (one call); pack detail + version modal install; "Installed" pills via `installedIndex` |
| **CP-6** | Pack update-**check** | `src-tauri/src/lib.rs` `refresh_pack_meta` `"atlauncher"` arm: `GET {API}/pack/{safeName}/latest`→latest version string→write `latest_version`/`latest_version_id`/`last_update_check`; throttled by existing `needs_update_check` (24h). Frontend update banner already reads these fields — no UI change beyond CP-5 | `lib_tests.rs`/`atl_tests.rs`: latest-version selection; update-available when stored `file_id != latest version`; throttle respected | **No** (reuses `PackMetaRefresh` DTO + `refresh_pack_meta` command) | `build.sh check` + `build.sh test` green; an installed ATL instance shows "update available" when a newer version exists |

---

## Per-checkpoint detail

### CP-1 — `AtlProvider` (bindings regen)
- `AtlProvider` is a **unit struct** — no `api_key` (ATL public API is keyless). Send
  `("User-Agent", "<descriptive>")` on **every** request — Cloudflare blocks empty/default UAs.
- Two base consts: `API = "https://api.atlauncher.com/v1"`,
  `CDN = "https://download.nodecdn.net/containers/atl"`.
- All responses are wrapped in `{ error, code, message, data }`; deser into `AtlEnvelope<T>` and
  fail with `ProviderError::BadResponse`/`HttpStatus` when `error == true` or `code != 200`.
- `search`: ONE `GET {API}/packs/full/public`; unwrap `data: Vec<AtlPack>`; filter
  case-insensitively by `params.query` on `name` (empty query → all); window by
  `offset`/`limit`. Map each → `ProjectSummary { id: safeName, slug: safeName, name,
  summary: description.lines().next(), downloads: 0, icon_url: Some(cdn_image_url(safeName)),
  categories: [], page_url: Some("https://atlauncher.com/pack/{safeName}") }`.
- `get_versions`: `GET {API}/pack/{safeName}` → `versions[]` → `ProjectVersion { id: version,
  name: version, version_number: version, game_versions: vec![minecraft], loaders: vec![],
  files: vec![], dependencies: vec![] }`. Loaders empty (only known from Configs.json).
- `cdn_image_url(safe) = format!("{CDN}/launcher/images/{}.png", safe.to_lowercase())` — pure.
- `newest_version(client, safeName) -> Option<String>` (pub, for CP-4/CP-6): `GET
  {API}/pack/{safeName}/latest` → `data.version`.
- `get_configs_manifest(client, safeName, version) -> AtlConfigsManifest` (pub, for CP-4):
  `GET {cdn_configs_url}` (raw CDN, **not** API-enveloped — Configs.json is a bare object).
- Test wiring: end `atl.rs` with `#[cfg(test)] #[path = "atl_tests.rs"] mod tests;`.
- **Wire-string note:** `ProviderKind::Atlauncher` under `#[serde(rename_all="camelCase")]`
  serializes to `"atlauncher"` (single word). Use `"atlauncher"` as the wire string everywhere
  (frontend route, command dispatch arms, `ProviderKind`) — do NOT introduce a separate
  `"atl"` string. (Rust module/type names stay short: `atl.rs`, `AtlProvider`.)

### CP-2 — MD5 hash support (no regen)
- Add `Md5(String)` to `ExpectedHash` and an `IncrementalHasher::Md5(md5::Md5)` arm; mirror the
  existing sha1 arms in `new_from`/`update`/`finalize_hex` and the two `Sha1|Sha256|Sha512`
  accessor matches (add `| ExpectedHash::Md5(h)`).
- `md-5` crate (`use md5::{Md5, Digest}`) — RustCrypto, same `Digest` API as `sha1`/`sha2`.
- ATL mod jars verify by MD5; the `Configs.zip` verifies by SHA-1 (already supported).
- Keep the existing `download_tests.rs` flake note in mind (`cp4_concurrency_bound_not_exceeded`
  is pre-existing/timing-sensitive — not introduced here).

### CP-3 — Install planner (no regen)
- `atl_dest_path(m: &AtlMod) -> Result<String, ModpackError>`:
  - If `m.path` non-empty → `validate_relative_path(path.join(file))`.
  - Else folder by `m.type`: `mods|dependency|depandency|coremods|ic2lib|denlib|flan|plugins`
    → `mods/`; `resourcepack|texturepack` → `resourcepacks/`; `shaderpack` → `shaderpacks/`;
    `datapack` → `datapacks/`; `jar|forge|mcpc` → `jarmods/`. Append `m.file`.
  - Legacy/exotic (`extract|decomp|texturepackextract|resourcepackextract|millenaire`) →
    signal skip (planner records in `skipped`; not an error). Modern packs never hit this.
- `build_atl_pack_plan(manifest, cdn_base, mc_dir)` (pure, mirrors `build_ftb_pack_plan`):
  for each mod, skip when `client == false`; compute dest via `atl_dest_path`; then by
  `download`:
  - `server` → `DownloadItem { url: format!("{cdn_base}/{}", m.url), dest, expected_hash:
    m.md5 → Some(ExpectedHash::Md5), size: m.filesize }`.
  - `direct` → same but `url: m.url` (absolute).
  - `browser` → push a `CfManualFile { project_id: 0, file_id: 0, file_name: m.file,
    page_url: m.url, expected_sha1: None, size: Some(m.filesize) }` (reuses the pending UX).
  - Record a `ModEntry { provider:"atlauncher", project_id: m.curse_id?.to_string() or "",
    version_id: "", file_name: m.file, hashes: {"md5":…}, from_pack:true, … }` only for
    `type ∈ {mods,dependency,coremods,…}` (the `mods/` family).
  - `optional` → included (v1; O-4).
- `extract_atl_configs(archive, mc_dir) -> Result<u32, ModpackError>`: extract every entry
  **root-relative** (no prefix) into `mc_dir` with the same zip-slip guard as `extract_prefix`.
  Implement as `extract_prefix(archive, mc_dir, "")` if that helper handles an empty prefix
  cleanly, else a thin dedicated copy. Skip when the manifest has `noConfigs == true` or no
  `configs` block.

### CP-4 — Command + job (no regen expected)
- `ImportAtlJob` mirrors `ImportFtbJob`: same staging dir, `remap_to_staging`/`promote_staging`,
  `pending_manual` write, returns `CfImportResult`. Its plan comes from `build_atl_pack_plan`
  over a held `AtlConfigsManifest`; additionally it downloads + `extract_atl_configs` the
  `Configs.zip` (added as a `DownloadItem` with `ExpectedHash::Sha1(configs.sha1)`).
- `install_atl_modpack` (mirrors `install_ftb_modpack`, **no CF key resolution**): resolve the
  version (selected, or `AtlProvider::newest_version` when `None`), fetch the Configs.json via
  `get_configs_manifest`, build the `Source` (incl. `recommended` from `manifest.memory` when
  `> 0`), `enqueue_import_atl`.
- Loader: from `manifest.loader.type` + version (`metadata.version` for forge/neoforge,
  `metadata.loader` for fabric/quilt) → instance `loader_kind`/`loader_version`. Existing
  launch-time resolver installs the loader (no new loader code).
- Construct `AtlProvider::new()` (no key) in the search/versions/packinfo/refresh arms.

### CP-5 — Browse UI (no regen)
- Widen the union at every site in the design's ripple table. ATL Browse: no server-side facets
  → hide `FiltersPopover` for `atlauncher` (like FTB). Single grid, client-side query filter,
  no `useInfiniteQuery` (whole catalog in one call).
- Sidebar: replace the static ATLauncher "coming soon" stub (`Sidebar.tsx:300-309`) with a
  `<NavLink to="/browse/atlauncher">` mirroring the FTB item (`Sidebar.tsx:287-298`).
- `ProviderBadge`: ATLauncher label + a distinct color (indigo suggested; FTB took sky-500).

### CP-6 — Update-check (no regen)
- `refresh_pack_meta` `"atlauncher"` arm reuses `needs_update_check` throttle + `PackMetaRefresh`
  return. Update-**apply** is out of v1 scope (O-3) — do not wire ATL into `update_modpack`
  beyond the "not supported yet" arm added at CP-4.

---

## Fixtures to add (`src-tauri/src/core/fixtures/`)
- `atl_packs_public.json` — trimmed `{ error, code, message, data:[AtlPack] }` with ≥3 packs
  (varied `safeName`, `versions[]`, `description`) to exercise the envelope + client-side filter.
- `atl_pack_detail.json` — `GET /pack/{safeName}` single pack (full `versions[]` + description).
- `atl_pack_latest.json` — `/pack/{safeName}/latest` version summary (`recommended`, version).
- `atl_configs.json` — a trimmed Configs.json with: ≥1 `server` mod, ≥1 `direct` mod, ≥1
  `browser` mod, ≥1 `resourcepack`, ≥1 `optional`, ≥1 `client:false`, ≥1 legacy type, a
  `loader` block (neoforge **and** a fabric variant fixture), `configs{filesize,sha1}`, and an
  optional `memory`. (Trim a real live response to keep it small.)

## Test inventory delta (expected)
- `atl_tests.rs` (new): +~10 (search one-call+filter+window, get_versions, get_project,
  get_pack_summary, get_projects_brief no-op, icon-url lowercase, envelope-error, newest_version).
- `providers_tests.rs`: +~2 (`ProviderKind::Atlauncher` serde, object-safety with AtlProvider).
- `download_tests.rs`: +~2 (Md5 verify ok / mismatch).
- `modpack_tests.rs`: +~9 (server/direct/browser routing, folder mapping, path override,
  client-false skip, optional include, legacy skip, traversal reject, configs root extraction).
- `lib_tests.rs`: +~3 (version selection, Source provenance + recommended + loader mapping).
- No frontend tests (none exist yet; planned Phase 7).
- Total Rust lib tests: 701 → ~728 expected.

## Regeneration checklist (bindings.ts)
Regen required at **CP-1** (`ProviderKind` gains `"atlauncher"`) **and CP-2** (`ExpectedHash`
gains `{type:"md5"}` — it IS surfaced via `DownloadItem`/`DownloadPlan`; the original spec was
wrong to say CP-2 needed none). CP-3…CP-6 touch no generated DTOs/commands/events (verify at
CP-4 that no helper DTO slipped in). Regen:
`scripts/build.sh dev` → wait for `[bindings] exported` → stop → **kill stray dev/cargo
processes + free port 1420** → commit the regenerated `src/lib/bindings.ts` alongside the CP-1
Rust change. Confirm `type ProviderKind = "modrinth" | "curseForge" | "ftb" | "atlauncher"`.

## Resolved decisions (human, 2026-06-26 — locked before execution)
- **O-1** ✅ Add `ExpectedHash::Md5` (+ `md-5` crate) — ATL jars are MD5-verified. (CP-2.)
- **O-2** ✅ Ship the defensive `browser → pending_manual` arm (no current pack uses it, but it's
  in the schema — cheap insurance). (CP-3.)
- **O-3** ✅ Check-only for v1 (mirror FTB); `update_modpack` returns "not supported yet". (CP-6.)
- **O-4** ✅ Install `optional` mods by default; user disables later in the modlist. (CP-3.)
- **O-5** ✅ Populate `Source.recommended` from `manifest.memory` when `> 0`. (CP-4.)
- **O-6** ✅ Browse default ordering = by name (stable for a bounded catalog). (CP-5.)

## Change log
- 2026-06-26 — Initial spec authored (design `docs/design/atlauncher-integration.md`). Not
  implemented. ATLauncher API verified live: keyless `api.atlauncher.com` (UA-gated) + content
  CDN `download.nodecdn.net/containers/atl`; whole catalog in one `/packs/full/public` call
  (no N+1); mod list in a CDN `Configs.json`; **mods SELF-HOSTED (`download: server/direct`),
  install is KEYLESS — no CF dependency** (decisive finding, opposite of FTB); mods carry only
  `md5` (needs `ExpectedHash::Md5`); configs delivered as one root-relative `Configs.zip`;
  icon via deterministic CDN URL. 6 checkpoints; bindings regen at CP-1 only
  (`ProviderKind::Atlauncher`). Open questions O-1…O-6 pending human decision.
- 2026-06-26 — Decisions locked (human): O-1 add MD5 verify; O-2 defensive `browser →
  pending_manual`; O-3 check-only v1; O-4 install optional by default; O-5 populate
  `Source.recommended` from `manifest.memory>0`; O-6 browse order by name. Cleared for
  implementation via `/ax-implement`.
- 2026-06-26 — CP-1 done (commit `ebdf862`): keyless `AtlProvider` + `ProviderKind::Atlauncher`
  + bindings regen. CP-2 done: `ExpectedHash::Md5` + `md-5` crate. **Spec correction:**
  `ExpectedHash` IS surfaced in `bindings.ts`, so CP-2 DID require a regen (original spec said
  none). Browse-`ProviderBadge` atlauncher label/color pulled forward at CP-1 (regen-forced).
