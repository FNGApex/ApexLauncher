# Design: FTB (Feed The Beast) modpack integration

> Branch: TBD (`feat/ftb-integration`)
> Status: design — approved-ready, not implemented
> Companion spec: `docs/spec/ftb-integration.md`

## Problem

FTB is one of the three modpack ecosystems users expect, alongside CurseForge and
Modrinth. Today FTB (and ATLauncher) appear in the Browse per-provider sidebar as **static
"coming soon" stubs** (`src/components/Sidebar.tsx:286-306`); the `BrowseProvider` component
renders a placeholder for any provider that is not `curseforge`/`modrinth`
(`src/routes/Browse.tsx:104-114`). There is no `FtbProvider`, no FTB install path, and the
frontend's provider unions are hardcoded to two values in ~21 places.

This design makes FTB **real**: browse the FTB catalog (featured / popular / search), open a
pack detail page, and install it one-click — at parity with the existing CF/Modrinth flows,
with pack update-checking where feasible.

## Goals

1. **Browse** — featured / popular / term-search the FTB catalog; a pack detail page with
   description, icon, author, version selector; "Installed" pills.
2. **One-click install** — fetch the FTB version manifest, download every file (FTB-CDN +
   CurseForge-referenced), stage-and-promote into a launchable instance with the correct
   Minecraft version + modloader.
3. **Reuse, not reinvent** — ride the existing `ModProvider` trait, `DownloadPlan`/
   `execute_plan_cancellable`, staging/promote, `CfManualFile`/`PendingManual` manual-download
   UX, and the TaskJob queue. Add the minimum new surface.
4. **Update-check (if in scope)** — surface "update available" for installed FTB packs via the
   existing `refresh_pack_meta` throttled path.

## Non-goals

- No FTB server-pack install (`/server/{os}` endpoints) — client only.
- No ATLauncher (the other "coming soon" stub stays a stub).
- No FTB private/unlisted packs (those need an FTB account token; out of scope).
- No re-implementation of CF resolution — FTB's CF-referenced files **reuse**
  `CurseForgeProvider::get_file` and the manual-download machinery verbatim.

---

## Evidence trail

### FTB API facts (verified against the live keyless API, 2026-06-26)

All endpoints probed live with `curl -A "ApexLauncher/..."`, **no API key**, base
`https://api.modpacks.ch`. Response headers showed `Access-Control-Allow-Origin: *`,
`Cache-Control: max-age=900, public`, `KeyDB-Cache: HIT` — keyless, CORS-open, server-cached.

| Fact | Endpoint / evidence | Verdict |
|------|---------------------|---------|
| Base URL is `https://api.modpacks.ch`, all browse under `/public/modpack/...` | apiary docs + live probes | supported |
| **Keyless** — no `x-api-key`, no auth header needed for public packs | live `GET /public/modpack/100` returned 200 with no auth | supported |
| **Featured** list: `GET /public/modpack/featured/{limit}` → `{ packs: [id,…], total, limit }` | live → `{packs:[124,117,126,123,125]}` | supported |
| **Popular** list: `GET /public/modpack/popular/installs/{limit}` (also `/popular/plays/{limit}`) → `{ packs:[id,…] }` | live → `{packs:[100,35,91,4,103]}` | supported |
| **Search**: `GET /public/modpack/search/{limit}?term=<q>` → `{ packs:[ftb ids], curseforge:[cf ids], total }` | live `?term=dawn` → `packs:[]`, `curseforge:[10 ids]`; `?term=fabric` → `packs:[105,109,86,102]` | supported |
| **Listing endpoints return ONLY integer pack ids** — no names/art inline → an N+1 detail fetch per visible pack is required | every list endpoint above | supported |
| **Pack detail**: `GET /public/modpack/{id}` → `{ id,name,synopsis,description (markdown),art[],authors[],tags[],versions[],installs,plays,released,updated,type,private,provider }` | live `GET /public/modpack/100` | supported |
| `art[]` entries: `{ url, type:"square", width,height,sha1,size }` → icon = `type=="square"` url | live pack 100 | supported |
| `authors[]`: `{ name, type:"team", website, id }` | live pack 100 → `[{name:"FTB Team",type:"team"}]` | supported |
| `versions[]` entries: `{ id, name, type, updated, targets[], specs{} }` — **`type` casing varies**: `"release"`, `"Release"`, `"beta"` | live packs 100/117/124/126 | supported — handle case-insensitively |
| **Version manifest**: `GET /public/modpack/{id}/{versionId}` → `{ targets[], specs{}, files[], changelog, name, type }` | live `GET /public/modpack/100/2275` | supported |
| `targets[]` encodes loader+MC+java: `{name:"forge"|"neoforge"|"fabric"|"quilt", type:"modloader", version}`, `{name:"minecraft", type:"game", version}`, `{name:"java", type:"runtime", version}` | live: forge `40.1.84`/mc `1.18.2`; neoforge `21.1.74`/mc `1.21.1` | supported |
| `specs`: `{ minimum, recommended }` — **recommended RAM in MB** | live pack 100 → `{minimum:4096,recommended:6144}` | supported |
| `files[]` entry: `{ name, path, url, sha1, size, type:"mod"\|"config"\|"script"\|"resource", clientonly, serveronly, optional, curseforge?:{project,file} }` | live pack 100/2275 | supported |
| **Two file kinds, clean XOR invariant**: a file either has a direct FTB-CDN `url` (host `dist.modpacks.ch`), OR has **empty `url` + a `curseforge:{project,file}` block** (resolve via CF API). Verified `empty-url set == curseforge set` exactly. | live packs 100/2275, 124/12630, 126/12599, 117/12425 | supported |
| **Mod jars are overwhelmingly CF-referenced**: FTB Unstable 1.21 (124) = **245/246 mods have no FTB url**, only a `curseforge{}` block. FTB CDN hosts configs/scripts/resources + a tiny handful of jars. | live | supported — *decisive: see Decision 3* |
| `path` is the dest dir relative to the instance mc dir (`"./mods/"`, `"./config/Mekanism/"`); `name` is the filename → dest = `path`+`name` (strip leading `./`) | live pack 100/2275 | supported |
| `serveronly` files exist on some packs (filter out for client); `optional` files exist | live (0 on probed modern packs; present historically) | supported |
| **"Latest version" notion**: no explicit pointer; `versions[]` is the list — newest = max by `id` (or `updated`). Pick newest entry whose `type` ~= `release`. | live | supported (derive) |
| **FTB catalog is small + curated** (~130 first-party packs); search of "dawn"/etc returns 0 native FTB packs (only the `curseforge[]` echo) | live | supported — shapes Browse UX (no true pagination) |

Sources:
- FTB modpacks.ch API portal — <https://modpacksch.docs.apiary.io/> (production base
  `https://api.modpacks.ch`).
- Endpoint catalogue (search/popular/featured/detail/version/changelog/server) — community
  reference gist <https://gist.github.com/LXGaming/a45d20213b23ce7a83ec9cf21b3dbbc3>.
- All response shapes above **verified live** on 2026-06-26 (see the per-row evidence).

### Codebase integration points (file:line)

| Fact | Source |
|------|--------|
| `ProviderKind { Modrinth, CurseForge }` (closed enum, serde camelCase → `"modrinth"`/`"curseForge"`) | `src-tauri/src/core/providers.rs:206-212` |
| `ModProvider` trait — `search`/`get_versions`/`get_project`/`get_projects_brief`/`get_pack_summary` (object-safe, `&dyn ProviderHttpClient` injected) | `src-tauri/src/core/providers.rs:374-433` |
| `ProviderHttpClient` (get/post) + `ReqwestProviderClient` | `src-tauri/src/core/providers.rs:221-280` |
| `ProviderError { KeyMissing, Network, HttpStatus, BadResponse }` | `src-tauri/src/core/providers.rs:288-304` |
| `ProjectSummary { provider, id, slug, name, summary, downloads, icon_url, categories, page_url }` | `src-tauri/src/core/providers.rs:63-88` |
| `ProjectVersion { provider, id, name, version_number, game_versions, loaders, files, dependencies }` | `src-tauri/src/core/providers.rs:91-110` |
| `PackInfo { title, description, icon_url, body_is_html }` | `src-tauri/src/core/providers.rs:336-347` |
| `PackSummary { name, icon_url, author, summary, categories }` (internal) | `src-tauri/src/core/providers.rs:356-368` |
| `SearchParams`/`SearchResult` | `src-tauri/src/core/providers.rs:160-204` |
| `CurseForgeProvider { api_key }` + `new()` (template) | `src-tauri/src/core/curseforge.rs:318-330` |
| `impl ModProvider for CurseForgeProvider` (all 5 methods) | `src-tauri/src/core/curseforge.rs:514-735` |
| `get_file(client, project_id, file_id) -> VersionFile` (the seam FTB CF-files reuse) | `src-tauri/src/core/curseforge.rs:439-461` |
| CF test wiring + mock-client/fixture pattern | `src-tauri/src/core/curseforge.rs:739-741`; `curseforge_tests.rs` |
| `build_cf_pack_plan` (pure router: url+sha1→items, else→manual) | `src-tauri/src/core/modpack.rs:489-566` |
| `CfManualFile { project_id, file_id, file_name, page_url, expected_sha1, size }` | `src-tauri/src/core/modpack.rs:397-414` |
| `impl From<&CfManualFile> for PendingManual` + pending-manual UX | `modpack.rs`; `docs/spec/cf-manual-download-ux.md` |
| `resolve_pack_file` (single-archive resolver — **NOT** usable for FTB, see Decision 2) | `src-tauri/src/core/modpack.rs:729-770` |
| `ResolvedPackFile { url, file_name, provider, version_id, version_name }` | `src-tauri/src/core/modpack.rs:692-705` |
| `remap_to_staging` / `promote_staging` (reused) | `src-tauri/src/core/modpack.rs:1199-1260` |
| `install_modpack` — fetch single archive → enqueue Import job (provider match) | `src-tauri/src/lib.rs:3197-3316` |
| `ImportCfZipJob` writes `instance.pending_manual` from `CfManualFile[]` (FTB reuses) | `src-tauri/src/lib.rs` (ImportCfZipJob) |
| `search_mods` provider dispatch (`"modrinth"`/`"curseforge"`) | `src-tauri/src/lib.rs:1338-1379` |
| `get_mod_versions` dispatch | `src-tauri/src/lib.rs:1386-1416` |
| `get_pack_info` dispatch | `src-tauri/src/lib.rs:1425-1453` |
| `refresh_pack_meta` dispatch (`"curseforge"\|"curseForge"`) | `src-tauri/src/lib.rs:1482-1583` |
| `collect_commands![…]` registration + event list | `src-tauri/src/lib.rs:3714-3777` |
| `Source { provider, project_id, file_id, pack_version, recommended, … }` (`recommended` always `None` today) | `src-tauri/src/lib.rs:3284-3300` (write site) |
| `java_resolve` tier-1 precedence reads `inst.source.recommended` (FTB can populate it) | `.claude/project/signals/java.md` |
| **Frontend ripple — `"curseforge"\|"modrinth"` union (≈21 sites)** | see table below |

### Frontend ripple surface (every provider-string site)

| Site | Source | Change |
|------|--------|--------|
| `useUiStore.browseProvider` union + default | `src/lib/store.ts:118-129` | widen to `"curseforge"\|"modrinth"\|"ftb"` |
| `/browse/:provider` routes (accept any param already) | `src/router.tsx:36-38` | no change (param is open) |
| `BrowseProvider` real-provider guard + "coming soon" placeholder | `src/routes/Browse.tsx:55-57, 104-114` | add `ftb` as a real provider |
| `BrowsePackInfo` provider cast + routing→wire remap | `src/routes/BrowsePackInfo.tsx:50, 64` | add `ftb` arm |
| `InstanceDetail` source.provider→routing remaps | `src/routes/InstanceDetail.tsx:387-392, 614-617` | add `ftb` arm |
| ModlistTab `ModProvider` type + provider dropdown (mods only) | `src/routes/InstanceDetail.tsx:956, 1037-1048` | **leave 2-valued** — FTB is pack-only, not a per-mod add source |
| InfoTab provider routing | `src/routes/instance-tabs/InfoTab.tsx:113-117` | add `ftb` arm |
| `ProviderBadge` normalization | `src/components/ProviderBadge.tsx:14-15` | add FTB label/color |
| `FiltersPopover` provider type | `src/components/FiltersPopover.tsx:41` | widen (FTB filters limited — see UX) |
| `BrowseCard` wire→routing | `src/components/BrowseCard.tsx:22-27` | add `ftb` arm |
| Sidebar FTB static "coming soon" item | `src/components/Sidebar.tsx:286-295` | convert to `<NavLink to="/browse/ftb">` |
| `searchMods`/`getModVersions`/`getPackInfo` param types | `src/lib/ipc.ts:234, 263, 272-282` | widen unions |
| `installedIndex` provider keying | `src/lib/installedIndex.ts:7-10` | add `ftb` lowercase key |
| `categoryMap` provider branches | `src/lib/categoryMap.ts:70-79` | FTB tag→category (limited; can no-op) |
| `bindings.ts` `ProviderKind` type | `src/lib/bindings.ts:1052` | regenerated → gains `"ftb"` |

---

## Decision 1 — `FtbProvider implements ModProvider` (browse parity, FTB semantics)

Add `src-tauri/src/core/ftb.rs` with `FtbProvider` (unit struct — **no API key field**, FTB
is keyless) implementing `ModProvider`, mirroring `CurseForgeProvider`'s structure and the
injectable-HTTP-seam test pattern. Add `ProviderKind::Ftb` (serde → `"ftb"`).

Method mapping (each builds a URL, calls `client.get(url, &[])` with a descriptive
`User-Agent` header per the Modrinth etiquette convention):

| Trait method | FTB implementation |
|--------------|--------------------|
| `search` | empty query → `GET /public/modpack/{limit}` via `featured/{limit}` (or `popular/installs/{limit}`); non-empty → `GET /public/modpack/search/{limit}?term=`. Take `packs[]` ids, then **N detail fetches** `GET /public/modpack/{id}` → map each to `ProjectSummary` (id, name, synopsis→summary, art square→icon_url, installs→downloads, tags→categories, page_url = ftbapp/web pack URL). Ignore the `curseforge[]` echo. |
| `get_project` | `GET /public/modpack/{id}` → `PackInfo { title:name, description (markdown), icon_url, body_is_html:false }` |
| `get_pack_summary` | `GET /public/modpack/{id}` → `PackSummary { name, icon_url, author(authors[0].name), summary(synopsis), categories(tags) }` |
| `get_versions` | `GET /public/modpack/{id}` → map `versions[]` → `ProjectVersion` (id=versionId, name, version_number=name, game_versions=[mc target], loaders=[modloader target name], `files: vec![]`, `dependencies: vec![]`). **Files intentionally empty** — FTB has no single primary jar; the install path resolves the manifest itself (Decision 2). Used only to populate the version-select modal. |
| `get_projects_brief` | returns `Ok(vec![])` (no-op). FTB has no batch mod-metadata endpoint; FTB packs' CF-referenced mods carry `provider:"curseforge"` and are enriched by the existing CF path. The few FTB-hosted jars stay un-enriched (acceptable). |

**Frugality note (api-frugality standing rule).** FTB list endpoints return only ids, so the
browse grid costs **1 list call + N detail calls** per page. Mitigations: (a) server-side
KeyDB cache (`max-age=900`); (b) TanStack Query client cache (staleTime); (c) fetch details
**only for the visible page**, with a modest default `limit` (e.g. 30–50) — FTB's catalog is
~130 packs so there is no deep pagination to fan out. Pack metadata captured at install-time
into `Source` (name/icon/author/version) is **never re-fetched to display**, exactly like CF/MR.

## Decision 2 — Dedicated FTB install planner (NOT `resolve_pack_file`)

`resolve_pack_file` (`modpack.rs:729-770`) resolves a pack to a **single downloadable archive**
(`.mrpack`/`.zip`) which `install_modpack` then fetches and hands to `ImportMrpackJob`/
`ImportCfZipJob`. **FTB has no archive** — a pack version *is* a JSON manifest listing N files.
So FTB needs its own planner, structurally analogous to the CF one:

- `build_ftb_pack_plan(manifest: &FtbVersionManifest, resolved_cf: &[(FtbFile, VersionFile)], mc_dir) -> FtbPackPlan` — **pure**, unit-tested. For each `files[]` entry:
  - **FTB-hosted** (`url` non-empty): → `DownloadItem { url, dest, expected_hash: Sha1, size }` where `dest = mc_dir.join(strip "./" from path).join(name)`, guarded by `validate_relative_path`. Record a `ModEntry` only when `type=="mod"` (provider `"ftb"`); config/script/resource files are plain `DownloadItem`s (the FTB analogue of mrpack `overrides/`).
  - **CF-referenced** (`curseforge:{project,file}`, no url): resolved out-of-band via `CurseForgeProvider::get_file` → if the resolved `VersionFile` has url+sha1 → `DownloadItem` + `ModEntry{provider:"curseforge"}`; else → **`CfManualFile`** (reuses the existing pending-manual UX verbatim).
  - Skip `serveronly == true`. Include `optional` (FTB-app default behaviour).
- `resolve_and_build_ftb_plan(provider_http, cf_provider, manifest, mc_dir) -> FtbPackPlan` — the async seam (mirrors `resolve_and_build_cf_plan`): batch-resolve the CF-referenced subset via `get_file`, then call the pure builder. Injectable client → mock-tested, no live network.

`FtbPackPlan { items: Vec<DownloadItem>, mods: Vec<ModEntry>, manual: Vec<CfManualFile>, skipped, failed }` — same shape family as `CfPackPlan`, so it feeds the existing
`remap_to_staging`/`promote_staging`/`execute_plan_cancellable` pipeline unchanged.

## Decision 3 — FTB install **requires the CurseForge API key** (gate, don't fight)

The decisive live finding: **~all mod jars in FTB packs are CF-referenced** (245/246 in FTB
Unstable 1.21). FTB's CDN hosts configs/scripts/overrides, not the mod jars. Therefore an FTB
install **cannot be keyless** — resolving the `curseforge:{project,file}` entries needs
`x-api-key`, and those files hit the same `allowModDistribution:false` manual-download cases as
native CF packs.

Consequences (all reuse existing machinery, no new policy):
- If the CF key is missing, FTB install fails with the **existing** `KeyMissing` →
  `ProviderCommandError{kind:"key_missing"}` surface that Browse/InstanceDetail already render.
  Browse FTB **listing** still works keyless (detail/version are public) — only **install**
  needs the key. So a keyless user can browse FTB but is prompted for a CF key on install.
- Manual (`allowModDistribution:false`) FTB files flow into `instance.pending_manual` exactly
  like CF-zip imports — the pending panel, badge, pre-launch warning, watcher, and drag-drop
  drop-in all work with zero new code (`docs/spec/cf-manual-download-ux.md`).

This is reframed as **reuse**, not a blocker: FTB rides the entire CF resolution + manual
pipeline. (Open question O-2 asks whether to additionally warn the user up-front that FTB
install depends on a CF key.)

## Decision 4 — Command/job wiring: extend `install_modpack`, add `ImportFtbJob`

Keep the frontend's single `installModpack(provider, id, …)` entry point — add an `"ftb"` arm
to `install_modpack` (`lib.rs:3197`) that **diverges from the archive path**:
1. Fetch the version manifest synchronously (`GET /public/modpack/{id}/{versionId}`; resolve
   "latest" when `version_id` is `None` by picking the newest release-type version via a
   detail call). Small JSON — fine to do pre-enqueue.
2. Build the `Source` provenance (provider `"ftb"`, `project_id`, `file_id=versionId`,
   `pack_version`, **`recommended = specs.recommended`** → populates the long-dormant
   pack-recommended Java/RAM tier), plus icon/author/name from the detail.
3. Enqueue a **new `ImportFtbJob`** carrying the manifest (or pack/version ids). The job:
   resolves CF files via `get_file` (network — belongs in the job for progress/cancel), builds
   the `FtbPackPlan`, `remap_to_staging` → `execute_plan_cancellable` → `promote_staging`,
   writes `ModEntry[]` + `pending_manual` to the manifest, returns a `CfImportResult`-shaped
   terminal result (reused — installed/skipped/failed + manual list). The pending-manual write
   is identical to `ImportCfZipJob`.

The other provider-dispatched commands gain `"ftb"` arms: `search_mods`, `get_mod_versions`,
`get_pack_info`, `refresh_pack_meta` — each constructs `FtbProvider` (no key) and calls the
trait. `enrich_instance_mods` needs no FTB arm (FTB `get_projects_brief` is a no-op; CF-tagged
mods enrich via the CF arm).

### Why not a separate `install_ftb_modpack` command?
Rejected — it would force the frontend `installModpack` call sites (`BrowseCard`,
`BrowsePackInfo`) to branch on provider before calling, and add a command + DTO. Extending the
existing command keeps the frontend ripple to **union-widening only** and reuses the result/
toast plumbing. The internal divergence (manifest vs archive) is contained in one match arm.

## Decision 5 — Pack update-check (in scope, cheap)

`refresh_pack_meta` already throttles to 24h and writes `latest_version`/`latest_version_id`/
`last_update_check` to the manifest. The FTB arm: `GET /public/modpack/{id}` → newest
release-type version's `id`+`name` → compare to `source.file_id`. One cached GET per 24h per
installed FTB instance — within the frugality budget. **Update *apply*** reuses the
`update_modpack` path only if it is taught the FTB planner; v1 may ship **update-check only**
(surface the banner) and defer apply (Open question O-3).

---

## New surface summary

### Rust
- `src-tauri/src/core/ftb.rs` (new) — `FtbProvider`, raw deser types (`FtbListResponse`,
  `FtbModpackDetail`, `FtbVersionEntry`, `FtbVersionManifest`, `FtbFile`, `FtbTarget`,
  `FtbArt`, `FtbAuthor`, `FtbSpecs`), URL builders, `impl ModProvider`. Sibling `ftb_tests.rs`.
- `src-tauri/src/core/providers.rs` — `ProviderKind::Ftb` (serde `"ftb"`).
- `src-tauri/src/core/modpack.rs` — `FtbPackPlan`, `build_ftb_pack_plan` (pure),
  `resolve_and_build_ftb_plan` (async seam). Sibling tests in `modpack_tests.rs`.
- `src-tauri/src/lib.rs` — `ImportFtbJob` (TaskJob); `"ftb"` arms in `install_modpack`,
  `search_mods`, `get_mod_versions`, `get_pack_info`, `refresh_pack_meta`; register nothing
  new beyond what already exists (no new command — `install_modpack` reused). `Source.recommended`
  populated from `specs.recommended`.

### Events / IPC / bindings
- **`ProviderKind` gains `"ftb"`** — the only generated-DTO change → **`bindings.ts` must
  regenerate** (CP-1). No new commands or events (install/search/etc. signatures unchanged).
- `CfImportResult` reused as the FTB job's terminal result (no new result DTO).

### Frontend
- Widen `"curseforge"|"modrinth"` → `…|"ftb"` at the ~14 ripple sites in the table above.
- Sidebar: FTB static item → `<NavLink to="/browse/ftb">`.
- `Browse.tsx`: treat `ftb` as a real provider (drop it from the placeholder guard); FTB has no
  loader/category facets server-side → render a reduced/empty `FiltersPopover` for FTB, and a
  single non-paginated grid (featured/popular default + term search).
- `ProviderBadge`: FTB label + color.
- ModlistTab per-mod add dropdown stays **2-valued** (FTB is pack-only; you can't add an
  individual FTB "mod").

---

## UX wireframes (ASCII)

Browse → FTB feed (`/browse/ftb`):

```
┌ Sidebar ────────┐   ┌ FTB ─────────────────── [ search FTB packs… ]  (no facets) ┐
│ Browse          │   │                                                            │
│  • CurseForge   │   │  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐        │
│  • Modrinth     │   │  │ [art] FTB    │ │ [art] FTB    │ │ [art] StoneB │        │
│  • FTB      ◀── │   │  │ Unstable 1.21│ │ Skies Expert │ │ -lock 3      │        │
│  • ATLauncher   │   │  │ NeoForge·1.21│ │ Forge·1.19.2 │ │ Forge·1.18.2 │        │
│    (soon)       │   │  │ 1.1M installs│ │ 240k installs│ │ 1.1M installs│        │
│                 │   │  │ [Install][↗]│ │ [Install][↗] │ │ [Installed]  │        │
└─────────────────┘   │  └──────────────┘ └──────────────┘ └──────────────┘        │
                      │  (featured + popular; ~130 packs, no infinite scroll)      │
                      └────────────────────────────────────────────────────────────┘
```

Pack detail (`/browse/ftb/:id`) + version-select on Install:

```
┌ FTB Unstable 1.21 ───────────────────────────── by FTB Team ─┐
│ [art]  In an ever-changing world… (synopsis)                 │
│        NeoForge 21.1 · MC 1.21.1 · recommended 6 GB RAM      │
│        [ Install ▾ ]   [ Open on FTB ↗ ]                     │
│  ── description (markdown) ──────────────────────────────────│
│  …                                                           │
└──────────────────────────────────────────────────────────────┘
   Install ▾  → version modal:  ( • 1.3.0 release   ○ 1.2.0   ○ 1.1.0 )
                                [ Cancel ]            [ Install ]
```

If the CF key is missing on Install → the existing key-missing prompt (no new UI):

```
┌ CurseForge API key required ─────────────────────────────────┐
│ FTB packs download most mods from CurseForge, which needs a  │
│ free API key. Add one under Settings → Advanced → API Keys.  │
│                                  [ Open Settings ]  [ Close ] │
└──────────────────────────────────────────────────────────────┘
```

---

## Tradeoffs / rejected alternatives

- **FtbProvider implements ModProvider vs a bespoke FTB module.** Chose the trait — browse
  commands (`search_mods`/`get_pack_info`/`get_mod_versions`/`refresh_pack_meta`) dispatch on
  `Box<dyn ModProvider>`, so implementing the trait gets FTB into all four with one `"ftb"`
  match arm each. The trait's `files`-bearing `get_versions` is a slight impedance mismatch
  (FTB has no single jar) — resolved by returning empty `files` and routing install through the
  dedicated planner (Decision 2). Clean enough; a bespoke module would duplicate dispatch.
- **Dedicated planner vs forcing FTB through `resolve_pack_file`.** Rejected the latter —
  `resolve_pack_file` assumes one archive URL; FTB is a manifest of N files. Shoehorning would
  mean faking an archive. The dedicated `build_ftb_pack_plan` mirrors `build_cf_pack_plan` and
  reuses the same plan→stage→promote pipeline.
- **Separate `install_ftb_modpack` command vs extending `install_modpack`.** Chose extend —
  minimizes frontend ripple (union-widening only) and reuses result/toast plumbing. The
  archive-vs-manifest divergence is one contained match arm.
- **Keyless FTB install (avoid CF dependency).** Impossible — verified the jars are
  CF-hosted. Reframed as reuse of the CF resolution + manual pipeline rather than a blocker.
  (We do NOT proxy or work around CF distribution flags — same policy as native CF packs.)
- **N+1 browse fetches vs a bulk endpoint.** FTB exposes no bulk-detail endpoint; the N+1 is
  unavoidable. Bounded by a small catalog + a modest page `limit` + server/client caching, and
  no metadata is re-fetched to display (frugality). Accepted.
- **Update *apply* in v1.** Deferred (O-3) — update-*check* is cheap and high-value; apply
  needs the FTB planner wired into `update_modpack` and careful `from_pack` diffing. Ship check
  first.
- **ATLauncher in the same pass.** Rejected — out of scope; different API, different format.

---

## Open questions (need input before execution)

- **O-1 — Browse default & pagination.** FTB has ~130 packs and no deep pagination. v1: show
  **featured + popular** (e.g. top 50) merged/deduped, with term-search filtering that set —
  no infinite scroll. Acceptable, or do you want a specific default ordering / a larger fetch?
- **O-2 — CF-key gating UX for FTB install.** Browse works keyless; install needs the CF key.
  Should we (a) let install fail into the existing key-missing prompt only when hit, or (b)
  proactively badge/disable Install on FTB cards when no CF key is configured, with an
  explanatory tooltip? (b) is friendlier but adds a key-presence check to the FTB Browse path.
- **O-3 — Pack-update scope.** Ship **update-check only** (banner: "vX available") for v1 and
  defer **update-apply** to a follow-up, or wire FTB into `update_modpack` now (more work,
  more `from_pack` diff testing)?
- **O-4 — Optional/clientonly files.** Confirm v1 installs **all non-`serveronly`** files
  (including `optional`), matching the FTB app's default. Or should `optional` files be
  opt-in?
- **O-5 — Recommended RAM.** OK to populate `Source.recommended` from FTB `specs.recommended`
  (activates the tier-1 Java/RAM precedence that is `None` for CF/MR today)? It's a net UX win
  but means FTB instances get a non-default memory suggestion CF/MR instances don't.
