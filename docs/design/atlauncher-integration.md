# Design: ATLauncher modpack integration

> Branch: TBD (`feat/atlauncher-integration`)
> Status: design — approved-ready, not implemented
> Companion spec: `docs/spec/atlauncher-integration.md`
> Template: this mirrors the just-shipped FTB integration (`docs/design/ftb-integration.md`),
> diverging where the ATLauncher API dictates (see **Decision 3 — the decisive finding**).

## Problem

ATLauncher is the last "coming soon" static stub in the Browse sidebar
(`src/components/Sidebar.tsx:300-309`); CurseForge, Modrinth, and FTB are real providers.
There is no `AtlProvider`, no ATLauncher install path, and the frontend provider unions are
hardcoded to three values across ~25 sites.

This design makes ATLauncher **real**: browse the ATLauncher catalog, open a pack detail page,
and one-click install — at parity with the CF/Modrinth/FTB flows, with pack update-checking.

## Goals

1. **Browse** — list / term-search the ATLauncher public catalog; a pack detail page with
   description, icon, version selector; "Installed" pills.
2. **One-click install** — fetch the pack version manifest, download every mod jar + the
   config overrides archive, install the loader, stage-and-promote into a launchable instance.
3. **Reuse, not reinvent** — ride the existing `ModProvider` trait, `DownloadPlan` /
   `execute_plan_cancellable`, staging/promote, overrides-extraction, and the TaskJob queue.
4. **Update-check** — surface "update available" for installed ATLauncher packs via the
   existing `refresh_pack_meta` throttled path.

## Non-goals

- No ATLauncher server-pack install (client only).
- No ATLauncher's own "browse CurseForge/Modrinth/Technic" passthrough (we have native CF/MR).
- No legacy/exotic mod types (`jar`/`forge`/`mcpc` jarmods, `decomp`, `millenaire`,
  `*extract`) — these only appear on pre-1.7 packs. Modern packs (1.16+) are ~all `mods` +
  occasional `resourcepack`/`shaderpack`/`datapack` (see Decision 5).
- No `loader.choose == true` interactive loader-version selection (rare; modern packs pin it).
- No "apply update" for installed ATLauncher packs in v1 — **check-only** (Open question O-3).

---

## Evidence trail

### ATLauncher API facts (verified live, 2026-06-26)

All probed live with `curl -A "ATLauncher/3.4.36.0"` (a **non-empty User-Agent is mandatory** —
the default `curl/*` UA is Cloudflare-blocked with a 1020). **No API key.** Two hosts:
public API `https://api.atlauncher.com/v1`, and the content CDN
`https://download.nodecdn.net/containers/atl` (from ATLauncher's `Constants.java`
`BASE_CDN_PATH = "/containers/atl"`, `DOWNLOAD_HOST = download.nodecdn.net`).

| Fact | Endpoint / evidence | Verdict |
|------|---------------------|---------|
| API needs a non-empty UA; otherwise Cloudflare 1020 ("you have been blocked") | empty-UA probe returned the CF block page; `-A "ATLauncher/..."` → 200 | supported — **send a descriptive UA** |
| **Keyless** — no `x-api-key`/auth for public packs | live, no auth header | supported |
| Standard envelope `{ error, code, message, data }` on every API response | live, all endpoints | supported |
| **Whole catalog in ONE call**: `GET /v1/packs/full/public` → `data:[Pack]` with **145** public packs, each with `id, name, safeName, type, versions[], description, …` inline | live → 145 packs, 731 KB | supported — *decisive for Browse frugality: no N+1 (unlike FTB)* |
| `GET /v1/packs/full/all` → same shape, includes non-public packs too | live → MoonQuest etc. | supported (use `/public`) |
| Pack `versions[]` entry (in the list): `{ version, minecraft, published, __LINK }` — **string** version names (e.g. `"v11.2.1hf"`, `"Pixelmon-1.21.1-9.3.16"`), NOT numeric ids | live | supported — handle arbitrary version strings (URL-encode) |
| Stable pack key is **`safeName`** (alphanumeric, case-sensitive), not the numeric `id`; all `/v1/pack/...` calls key by safeName | wiki + live (`/v1/pack/AllTheForge10`) | supported — use `safeName` as the provider `project_id` |
| `GET /v1/pack/{safeName}` → single Pack (full `versions[]` + `description`, no icon field) | live `GET /v1/pack/DigitalReality` | supported |
| `GET /v1/pack/{safeName}/{version}` → version **summary** `{ version, minecraftVersion, recommended, published, changelog }` — **NOT the mod list** | live `GET /v1/pack/DigitalReality/5.0.1` | supported |
| `GET /v1/pack/{safeName}/latest` alias → newest version summary (carries `recommended` bool) | live `GET /v1/pack/AllTheForge10/latest` | supported — resolves the default version |
| **The mod list / install manifest is NOT in the API** — it is a CDN file: `GET https://download.nodecdn.net/containers/atl/packs/{safeName}/versions/{version}/Configs.json` (also `.xml`) | live → DigitalReality 5.0.1 Configs.json (397 KB, 384 mods) | supported — *the real manifest source* |
| Configs.json shape: `{ enableCurseIntegration, enableEditingMods, version, minecraft, loader{}, mods[], configs{} }` (+ optional `memory`, `permGen`, `java`, `mainClass`, `libraries`, `noConfigs`, `extraArguments` on custom/legacy packs) | live | supported |
| `loader`: `{ type:"neoforge"\|"forge"\|"fabric"\|"quilt", choose:bool, metadata{…}, className }`. **neoforge/forge** version → `metadata.version`; **fabric** → `metadata.loader` (+ `metadata.yarn`) | live DigitalReality (neoforge `21.1.83`), AllTheFabric5 (fabric `0.16.7`) | supported — loader-version field differs by loader |
| `configs`: `{ filesize, sha1 }` → a **single overrides archive** `Configs.zip` at `.../versions/{version}/Configs.zip`, extracted **root-relative** into the instance mc dir (the `config/`, `scripts/`, … tree) | live (DigitalReality `configs.filesize 21 MB`) | supported — analogue of mrpack `overrides/` |
| `mods[]` entry: `{ name, version, url, file, md5, filesize, download, type, optional, client, server, curse_id?, curse_file_id?, … }` | live | supported |
| **Mods are SELF-HOSTED, not CF-referenced** — `download` is one of `{ server, browser, direct }` (ATLauncher `DownloadType.java`); there is **no curseforge/modrinth download type**. `server` ⇒ `CDN_BASE + "/" + url`; `direct` ⇒ absolute URL; `browser` ⇒ manual (rare) | live + `DownloadType.java` | supported — ***the decisive finding (Decision 3)*** |
| Download-type distribution across 8 modern packs (mods): DigitalReality 384× server; AllTheForge10 161× server; AllTheFabric5 154× server; KingdomsOfTheValley 162 server + 1 direct; PixelmonMod 12 server + 3 direct; 3RDLIFE 137 server; AllTheForge9 149 server; DonutCraft 133 server. **Zero `browser`. Zero CF API resolution.** | live | supported — *first-party packs are server/direct = keyless* |
| `curse_id` / `curse_file_id` on a mod are **informational metadata only** (fingerprint/dedup) — the jar is downloaded from the ATL CDN, never resolved through the CF API. The `curseForgeProject`/`modrinthProject` object fields were empty on all probed first-party packs | live (`curse_id` present, `curseForgeProject` absent) | supported |
| **Mods carry ONLY `md5`** — 384/384 in DigitalReality had `md5`, none had `sha1`/`sha512`. The download engine's `ExpectedHash` is Sha1/Sha256/Sha512 — **no MD5** | live + `download.rs:27` | supported — *requires an MD5 hash variant (Decision 6)* |
| Version-detail `recommended` is a **boolean** (is-this-version-recommended), not a RAM value. RAM recommendation lives in the Configs.json optional `memory` (int MB), often absent on modern loader-packs | live (`recommended:true`; no `memory` on DigitalReality) | supported — populate `Source.recommended` only when `memory > 0` |
| `optional` mods exist (3RDLIFE 9, AllTheForge9 6); `client`/`server` booleans gate side | live | supported |
| Mod `type` (ATLauncher `ModType.java`): `mods, dependency, coremods, resourcepack, texturepack, shaderpack, datapack, jar, forge, mcpc, extract, decomp, millenaire, …` → dest folder. Modern packs ≈ all `mods` (+ rare `resourcepack`) | live + `ModType.java` | supported |
| Pack icon: `https://download.nodecdn.net/containers/atl/launcher/images/{safeName-lowercase}.png` (deterministic; no API field) | live → 200 `image/png` for `alltheforge10.png`; confirmed by `Pack.getImage()` (`{safeName}.png` lowercase) | supported — *no fetch needed to build the icon URL* |

Sources:
- ATLauncher API wiki — <https://wiki.atlauncher.com/api-docs/v1/> (endpoints `/v1/packs`, `/v1/pack`).
- ATLauncher source (GPLv3) — `DownloadType.java` (`{server, browser, direct}`), `ModType.java`
  (mod-type enum), `Mod.java` (mod fields incl. `md5`/`curse_id`), `Version.java` (`memory`,
  `loader`, `noConfigs`), `Constants.java` (CDN base) — <https://github.com/ATLauncher/ATLauncher>.
- All response shapes **verified live** on 2026-06-26 (per-row evidence above).

### Codebase integration points (file:line, post-FTB-merge)

| Fact | Source |
|------|--------|
| `ProviderKind { Modrinth, CurseForge, Ftb }` (serde camelCase) | `src-tauri/src/core/providers.rs:209-213` |
| `ModProvider` trait (5 methods, object-safe) | `src-tauri/src/core/providers.rs:375-434` |
| `ProviderHttpClient` + `ReqwestProviderClient` | `src-tauri/src/core/providers.rs:222-281` |
| `ProviderError { KeyMissing, Network, HttpStatus, BadResponse }` | `src-tauri/src/core/providers.rs:289-305` |
| `FtbProvider` (keyless template) | `src-tauri/src/core/ftb.rs` (whole file) |
| `pub mod ftb;` (where `pub mod atl;` goes) | `src-tauri/src/core/mod.rs:27` |
| `ExpectedHash { Sha1, Sha256, Sha512 }` + `IncrementalHasher` | `src-tauri/src/core/download.rs:27-33, 199-228, 267, 480` |
| `FtbPackPlan` / `build_ftb_pack_plan` / `resolve_and_build_ftb_plan` / `ftb_dest_path` | `src-tauri/src/core/modpack.rs:662-673, 677-687, 701-830, 831-897` |
| `remap_to_staging` / `promote_staging` (reused) | `src-tauri/src/core/modpack.rs:1424-1438, 1458-1460` |
| `extract_overrides` / `extract_prefix` / `is_safe_dest` (overrides machinery) | `src-tauri/src/core/modpack.rs:1235-1246, 1249-1305, 1318-…` |
| `validate_relative_path` | `src-tauri/src/core/modpack.rs:1025-1050` |
| `CfManualFile` + `impl From<&CfManualFile> for PendingManual` | `src-tauri/src/core/modpack.rs:397-414, 416-429` |
| `ImportFtbJob` struct + `impl TaskJob` | `src-tauri/src/lib.rs:3184-3193, 3196-3337` |
| `enqueue_import_ftb` / `install_ftb_modpack` | `src-tauri/src/lib.rs:3340-3376, 3381-3466` |
| `Source.recommended` population (FTB, `RecommendedJava { memory_mb, java_args }`) | `src-tauri/src/lib.rs:3425-3432` |
| `install_modpack` "ftb" arm | `src-tauri/src/lib.rs:3543-3546` |
| `search_mods` / `get_mod_versions` / `get_pack_info` / `refresh_pack_meta` "ftb" arms | `src-tauri/src/lib.rs:1377-1380, 1418-1423, 1461-1466, 1557-1579` |
| `update_modpack` "ftb" not-supported arm | `src-tauri/src/lib.rs:3959-3963` |
| `collect_commands!` / `collect_events!` (no per-provider command/event) | `src-tauri/src/lib.rs:4069-4117, 4118-4128` |

### Frontend ripple surface (every provider-string site, post-FTB-merge)

The current union is `"curseforge" \| "modrinth" \| "ftb"` (lowercase wire) and the generated
`ProviderKind = "modrinth" \| "curseForge" \| "ftb"` (camelCase). Add **`"atlauncher"`**.

| Site | Source | Change |
|------|--------|--------|
| `useUiStore.browseProvider` type | `src/lib/store.ts:118` | +`"atlauncher"` |
| `setBrowseProvider` param | `src/lib/store.ts:119` | +`"atlauncher"` |
| `searchMods` provider param | `src/lib/ipc.ts:234` | +`"atlauncher"` |
| `getModVersions` provider param | `src/lib/ipc.ts:263` | +`"atlauncher"` |
| `getPackInfo` provider param | `src/lib/ipc.ts:272` | +`"atlauncher"` |
| `addMod` provider param | `src/lib/ipc.ts:284` | +`"atlauncher"` (harmless; ATL is pack-only) |
| Browse "remember" gate | `src/routes/Browse.tsx:55` | add `\|\| provider === "atlauncher"` |
| Browse `supportsFilters` | `src/routes/Browse.tsx:61` | exclude `"atlauncher"` (no facets, like FTB) |
| Browse "coming soon" gate | `src/routes/Browse.tsx:107` | drop `"atlauncher"` (now real) |
| Browse placeholder copy | `src/routes/Browse.tsx:114-117` | delete the ATLauncher placeholder branch |
| `SingleProviderFeedProps.provider` | `src/routes/Browse.tsx:244` | +`"atlauncher"` |
| `toWireProvider` | `src/routes/BrowsePackInfo.tsx:51` | add `atlauncher` arm |
| route comment + `providerParam` cast | `src/routes/BrowsePackInfo.tsx:65-66` | +`"atlauncher"` |
| `ProviderBadge` cast (camelCase) | `src/routes/BrowsePackInfo.tsx:122` | +`"atlauncher"` |
| `getModVersions` cast | `src/routes/BrowsePackInfo.tsx:215` | +`"atlauncher"` |
| ModlistTab `providerRoute` type + arm | `src/routes/InstanceDetail.tsx:387-392` | +`"atlauncher"` type & arm |
| InstanceDetail provider routing (later) | `src/routes/InstanceDetail.tsx:616-621` | +`"atlauncher"` type & arm |
| InfoTab `providerRoute` type + arm | `src/routes/instance-tabs/InfoTab.tsx:113-120` | +`"atlauncher"` type & arm |
| Sidebar ATLauncher static stub | `src/components/Sidebar.tsx:300-309` | convert to `<NavLink to="/browse/atlauncher">` |
| `ProviderBadge` `toProviderKind` | `src/components/ProviderBadge.tsx:15` | add `atlauncher` arm |
| `ProviderBadge` LABELS | `src/components/ProviderBadge.tsx:27` | `atlauncher: "ATLauncher"` |
| `ProviderBadge` COLORS | `src/components/ProviderBadge.tsx:33` | pick a distinct hue (e.g. indigo) |
| `BrowseCard` `providerRoute` type + arm | `src/components/BrowseCard.tsx:22, 27` | +`"atlauncher"` |
| `FiltersPopover` provider prop | `src/components/FiltersPopover.tsx:41` | +`"atlauncher"` |
| `categoryMap` provider param | `src/lib/categoryMap.ts:70` | +`"atlauncher"` (body may no-op) |
| `bindings.ts` `ProviderKind` | `src/lib/bindings.ts:1052` | **regenerated** → gains `"atlauncher"` |
| `installedIndex.ts` | `src/lib/installedIndex.ts` | **no change** — already lowercases provider keys |
| `router.tsx` `:provider` param | `src/router.tsx:36-38` | **no change** — param is open |
| ModlistTab per-mod add dropdown | `src/routes/InstanceDetail.tsx` (~956, 1037) | **leave 3-valued** — ATL is pack-only, not a per-mod add source |

---

## Decision 1 — `AtlProvider implements ModProvider` (keyless; one-call browse)

Add `src-tauri/src/core/atl.rs` with `AtlProvider` (unit struct — **no API key**), mirroring
`FtbProvider`. Add `ProviderKind::Atlauncher` (serde `"atlauncher"` — `rename_all="camelCase"`
of a single-word `Atlauncher` already yields `"atlauncher"`; the wire string is `"atlauncher"`
everywhere — frontend route, command dispatch, and `ProviderKind`).

Two base constants: `API = "https://api.atlauncher.com/v1"`,
`CDN = "https://download.nodecdn.net/containers/atl"`. Every request sends a descriptive
`User-Agent` (Cloudflare-mandatory) and no key.

| Trait method | ATLauncher implementation |
|--------------|---------------------------|
| `search` | `GET {API}/packs/full/public` → **ONE call** returns the whole catalog. Filter `data[]` **client-side** by `params.query` (case-insensitive substring on `name`); window by `offset`/`limit`. Map each `Pack` → `ProjectSummary { provider: Atlauncher, id: safeName, slug: safeName, name, summary: description-first-line, downloads: 0, icon_url: cdn_image_url(safeName), categories: [], page_url: https://atlauncher.com/pack/{safeName} }`. The list is cacheable (TanStack `staleTime`) so search is effectively free after first load. |
| `get_versions` | `GET {API}/pack/{safeName}` → map `versions[]` → `ProjectVersion { id: version-string, name: version, version_number: version, game_versions: [minecraft], loaders: [], files: vec![], dependencies: vec![] }`. **Loaders empty** here — the loader is only known from the Configs.json manifest (fetched at install). Used to populate the version-select modal. |
| `get_project` | `GET {API}/pack/{safeName}` → `PackInfo { title: name, description, icon_url, body_is_html: false }` (ATL descriptions are plain text/markdown-ish). |
| `get_pack_summary` | `GET {API}/pack/{safeName}` → `PackSummary { name, icon_url, author: None, summary: description-first-line, categories: [] }` (ATL has no author field in the public API). |
| `get_projects_brief` | `Ok(vec![])` — no-op (ATL has no batch mod-metadata endpoint; ATL-hosted jars stay un-enriched). |

`cdn_image_url(safeName) = "{CDN}/launcher/images/{safeName.to_lowercase()}.png"` — a pure
string builder, **no fetch** (api-frugality). Browse cards render it directly; a 404 falls back
to the placeholder in the existing `<img onError>` path.

**Frugality win vs FTB.** FTB needed `1 list + N detail` calls per browse page. ATL needs
**exactly one** `/packs/full/public` call for the entire grid (catalog inline), client-side
search/filter, and a deterministic icon URL. Detail/version are fetched only when the pack
page or install modal opens. Metadata captured into `Source` at install is never re-fetched.

## Decision 2 — Provider id is `safeName` (a string), versions are arbitrary strings

Unlike FTB's numeric ids, ATLauncher keys everything by `safeName` and uses free-form version
strings (`"v11.2.1hf"`, `"Pixelmon-1.21.1-9.3.16"`). So the provider `project_id` is the
`safeName` and the version `id` is the version string (URL-encoded in CDN/API paths). No
numeric-id assumptions; "latest" resolves via `GET {API}/pack/{safeName}/latest`.

## Decision 3 — **ATLauncher install is KEYLESS** (the decisive finding; opposite of FTB)

The decisive question — *are pack mod jars self-hosted or CurseForge-referenced?* — resolves
**self-hosted**. Live evidence:

- ATLauncher's `DownloadType` enum is `{ server, browser, direct }` — there is **no
  curseforge/modrinth download type** for first-party pack mods.
- Across 8 modern packs (1.18–1.21, 10–384 mods each), **every** mod is `download:"server"`
  (ATL CDN) or `download:"direct"` (absolute URL); **zero `browser`**, **zero CF API
  resolution**. `curse_id`/`curse_file_id` are informational only — the jar is fetched from
  ATL's CDN, not the CF API.

**Therefore ATLauncher install needs NO CurseForge API key and NO CF resolution loop.** This is
strictly simpler than FTB:

- `server` file → `DownloadItem { url: "{CDN}/{mod.url}", dest, expected_hash: Md5, size }`.
- `direct` file → `DownloadItem { url: mod.url (absolute), dest, expected_hash: Md5, size }`.
- `browser` file (redistribution-restricted; not seen on current first-party packs but in the
  schema) → route to the existing `pending_manual` UX with `page_url = mod.url`, `project_id`/
  `file_id = 0`. Defensive arm; flagged O-2.

There is **no `resolve_and_build_*` async seam** (FTB needed one only for CF resolution). The
ATL planner is **fully pure** over the parsed manifest.

## Decision 4 — Config overrides via a single `Configs.zip` (mrpack-`overrides`-shaped)

ATLauncher delivers all non-jar content (`config/`, `scripts/`, `defaultconfigs/`, …) as one
archive `{CDN}/packs/{safeName}/versions/{version}/Configs.zip`, described by the manifest's
`configs: { filesize, sha1 }`. Unlike mrpack's `overrides/`-prefixed entries, the ATL zip is
**root-relative** (its top level is `config/`, `scripts/`, …). The job:

1. Downloads `Configs.zip` (verified by `configs.sha1` — a SHA-1, so the existing engine
   verifies it directly) into the staging dir.
2. Extracts it **root-relative** into the staging mc dir via a new `extract_atl_configs`
   wrapper around the existing zip-slip-safe machinery (`extract_prefix` with `prefix == ""`,
   or a thin dedicated helper reusing `validate_relative_path` + `is_safe_dest`).

When `version.noConfigs == true` or the `configs` block is absent → skip the archive.

## Decision 5 — Mod-type → dest folder; client-side filtering

`atl_dest_path(mod)`:
- Honor `mod.path` if present (rare; gives an explicit relative dir).
- Else map `mod.type` → folder: `mods`/`dependency`/`coremods`/`ic2lib`/`denlib`/`flan`/
  `plugins` → `mods/`; `resourcepack`/`texturepack` → `resourcepacks/`; `shaderpack` →
  `shaderpacks/`; `datapack` → `datapacks/` (path-specified in practice); `jar`/`forge`/`mcpc`
  → `jarmods/`. Append `mod.file`.
- **Legacy/exotic** types (`extract`, `decomp`, `*extract`, `millenaire`) → **skip + record in
  `skipped`** (only on pre-1.7 packs; out of scope per Non-goals). Modern packs never hit this.

Filter: include a mod when `mod.client != false`; skip server-only mods. `optional` mods are
included by default (v1; O-4) — they ship in the instance, user can disable later via the
existing enable/disable UX.

## Decision 6 — Add `ExpectedHash::Md5` to the download engine (the one cross-domain change)

ATL mods carry **only `md5`** (verified 384/384). The engine's `ExpectedHash` is
Sha1/Sha256/Sha512. To preserve the project's verify-everything posture, add an `Md5(String)`
variant + an `IncrementalHasher::Md5` arm, backed by the `md-5` crate (RustCrypto, same family
as the existing `sha1`/`sha2`). This is the only change outside the FTB-shaped template. The
`Configs.zip` uses SHA-1 (already supported), so only mod jars need MD5.

Rejected alternative: download ATL jars unverified (size-only). Cheaper (no engine change) but
abandons integrity verification that every other provider enforces — rejected. (Open as O-1 if
the human prefers to defer MD5 and ship size-only first.)

## Decision 7 — Command/job wiring: extend `install_modpack`, add `ImportAtlJob`

Mirror FTB exactly. Keep the single `installModpack(provider, id, …)` frontend entry point; add
an `"atlauncher"` arm to `install_modpack` (`lib.rs:3543`) → `install_atl_modpack`:
1. Resolve the version (selected, or `/pack/{safeName}/latest` when `version_id` is `None`).
2. Fetch the Configs.json manifest (`{CDN}/packs/{safeName}/versions/{version}/Configs.json`).
3. Build `Source { provider:"atlauncher", project_id: safeName, file_id: version, pack_version:
   version, recommended: manifest.memory (when > 0 → `RecommendedJava{memory_mb}`), icon_url,
   name, … }`.
4. `enqueue_import_atl(ImportAtlJob { manifest, instance_name, minecraft, loader_kind,
   loader_version, pack_source })`. The job: `build_atl_pack_plan` (pure) → `remap_to_staging`
   → download mods + `Configs.zip` via `execute_plan_cancellable` → `extract_atl_configs` →
   `promote_staging` → write `ModEntry[]` (+ any `browser` files into `pending_manual`) →
   return a `CfImportResult`-shaped terminal result (reused).

`search_mods` / `get_mod_versions` / `get_pack_info` / `refresh_pack_meta` gain `"atlauncher"`
arms constructing `AtlProvider::new()` (no key). `update_modpack` returns the "not supported
yet" string for ATL (check-only, like FTB). `enrich_instance_mods` needs no ATL arm
(`get_projects_brief` is a no-op).

Loader: `loader_kind`/`loader_version` come from `manifest.loader` — `metadata.version` for
forge/neoforge, `metadata.loader` for fabric/quilt. Mapped onto the instance's existing loader
fields; the existing launch-time resolver installs the loader (no new loader code).

## Decision 8 — Pack update-**check** (in scope, cheap)

`refresh_pack_meta` `"atlauncher"` arm: `GET {API}/pack/{safeName}/latest` → latest version
string → write `latest_version`/`latest_version_id`/`last_update_check`; throttled by the
existing `needs_update_check` (24h). One cached GET per 24h per installed ATL instance.
**Update-apply** is out of v1 scope (O-3).

---

## New surface summary

### Rust
- `src-tauri/src/core/atl.rs` (new) — `AtlProvider`, raw deser types (`AtlEnvelope<T>`,
  `AtlPack`, `AtlPackVersion`, `AtlConfigsManifest`, `AtlLoader`, `AtlMod`), URL/icon builders,
  `impl ModProvider`. Sibling `atl_tests.rs` + fixtures.
- `src-tauri/src/core/providers.rs` — `ProviderKind::Atlauncher` (serde `"atlauncher"`).
- `src-tauri/src/core/mod.rs` — `pub mod atl;`.
- `src-tauri/src/core/download.rs` — `ExpectedHash::Md5` + `IncrementalHasher::Md5` (md-5 crate).
- `src-tauri/Cargo.toml` — `md-5` dependency.
- `src-tauri/src/core/modpack.rs` — `AtlPackPlan`, `build_atl_pack_plan` (pure), `atl_dest_path`,
  `extract_atl_configs`. Sibling tests in `modpack_tests.rs`.
- `src-tauri/src/lib.rs` — `ImportAtlJob`, `enqueue_import_atl`, `install_atl_modpack`;
  `"atlauncher"` arms in `install_modpack`/`search_mods`/`get_mod_versions`/`get_pack_info`/
  `refresh_pack_meta`/`update_modpack`; `Source.recommended` from `manifest.memory`.

### Events / IPC / bindings
- **`ProviderKind` gains `"atlauncher"`** — the only generated-DTO change → `bindings.ts`
  regenerates (CP-1). No new commands/events; `install_modpack` reused; `CfImportResult` reused.

### Frontend
- Widen `… \| "atlauncher"` at the ~25 ripple sites in the table above.
- Sidebar: ATLauncher static stub → `<NavLink to="/browse/atlauncher">`.
- `Browse.tsx`: treat `atlauncher` as a real provider; hide facets (no server-side filters);
  single non-paginated client-side-searched grid.
- `ProviderBadge`: ATLauncher label + a distinct color (indigo suggested).

---

## How ATLauncher differs from FTB (at a glance)

| Aspect | FTB | ATLauncher |
|--------|-----|------------|
| Mod hosting | **CF-referenced** (jars on CF) | **Self-hosted** (ATL CDN / direct) |
| Install needs CF key | **Yes** | **No (keyless)** |
| CF resolution loop | `resolve_and_build_ftb_plan` (async, `get_file` per mod) | none — pure planner |
| Manual/`pending_manual` | common (`allowModDistribution:false`) | rare (`download:"browser"`, defensive only) |
| Browse cost | `1 list + N detail` per page (N+1) | **1 call** for whole catalog (client-side filter) |
| Config files | per-file entries in the manifest | one `Configs.zip` (root-relative overrides) |
| Mod hash | `sha1` (engine-native) | **`md5`** (needs `ExpectedHash::Md5`) |
| Pack id | numeric | `safeName` (string) |
| Version id | numeric | free-form string |
| Recommended RAM | `specs.recommended` | `manifest.memory` (optional, often absent) |
| Icon | `art[]` square url (from detail) | deterministic CDN URL (no fetch) |

---

## Tradeoffs / rejected alternatives

- **AtlProvider implements ModProvider vs a bespoke module.** Chose the trait — the four browse
  commands dispatch on `Box<dyn ModProvider>`, so one `"atlauncher"` arm each wires it all.
- **Pure planner vs an async resolve seam.** ATL needs no out-of-band resolution (self-hosted),
  so `build_atl_pack_plan` is pure — simpler than FTB's `resolve_and_build_ftb_plan`. No async
  seam added.
- **Add `ExpectedHash::Md5` vs download unverified.** Chose MD5 — preserves the verify-always
  posture; small, well-isolated engine change (one enum arm + `md-5` crate). (O-1 if deferred.)
- **One `/packs/full/public` call vs paginated search.** ATL exposes the whole catalog in one
  response; client-side search is cheaper and strictly more frugal than FTB's N+1. Accepted.
- **Configs.zip root-relative extraction vs mrpack `overrides/` reuse.** `extract_overrides`
  assumes a prefix; ATL's zip is root-relative — a thin `extract_atl_configs` reuses the same
  zip-slip safety with an empty prefix. Minimal new code.
- **`safeName` string id vs numeric.** Forced by the API; threaded through as the `project_id`.
- **Legacy mod types.** Skipped (pre-1.7 only); modern packs are `mods` + occasional
  `resourcepack`. Documented as a Non-goal.

---

## Open questions (need input before execution)

- **O-1 — MD5 verification.** Add `ExpectedHash::Md5` (+ `md-5` crate) so ATL jars are
  hash-verified (recommended), or ship size-only-unverified first and add MD5 later?
- **O-2 — `browser` (manual) mods.** Not present on current first-party packs but in the
  schema. Ship the defensive `browser → pending_manual` arm (reuses the existing UX with
  `page_url = mod.url`), or hard-skip+warn until a real pack needs it?
- **O-3 — Update scope.** Ship **update-check only** (banner) for v1 and defer update-apply
  (matches FTB), or wire ATL into `update_modpack` now?
- **O-4 — `optional` files.** Install all `optional` mods by default (user disables later via
  the modlist), or make them opt-in at install?
- **O-5 — `Source.recommended` from `manifest.memory`.** Populate the tier-1 Java/RAM
  recommendation when `memory > 0` (often absent on modern packs), like FTB? Net UX win.
- **O-6 — Browse default ordering.** `/packs/full/public` order is API-defined. Sort by
  name, by most-recent `published`, or keep API order for the default (empty-query) grid?
