# Browse page rework (design)

Status: **PROPOSED — not yet built.** Planning artifact; the orchestrator hands the
companion spec (`docs/spec/browse-rework.md`) to a builder after human approval.

Contract: a LARGE rework of the modpack **Browse** page (`src/routes/Browse.tsx` +
`src/routes/BrowsePackInfo.tsx`) and the `search_mods` backend. Four human requirements:

1. **Bigger = Better** — larger, more prominent cards + page.
2. **Rework the top nav/search** — a more prominent search/nav region.
3. **MOST IMPORTANT — "installed" indicator** — when a modpack the user already
   installed appears in search results AND on its pack-info page, clearly show it is
   already installed.
4. **Categories & Filters** — REMOVE the inline "Any MC version" + "Any loader"
   dropdowns; move filtering into an anchored **Filters** popup offering loader
   multi-select (checkboxes), a game-version dropdown, and categories.

This decomposes into a small backend change (search params: categories + multi-loader)
and a larger frontend change (cards, nav/search, filters popup, installed indicator).

---

## 1. Subsystems touched

| Subsystem | Files | Change |
|---|---|---|
| providers (backend) | `src-tauri/src/core/providers.rs` (`SearchParams`), `modrinth.rs` (`build_search_url`), `curseforge.rs` (`build_search_url`), sibling `_tests.rs` | Add `categories: Vec<String>` + `loaders: Vec<String>` to `SearchParams`; build per-provider facets/params. |
| providers (command) | `src-tauri/src/lib.rs` (`search_mods`) | Accept `categories`/`loaders` args; keep back-compat for `mc_version`. |
| IPC | `src/lib/bindings.ts` (regen), `src/lib/ipc.ts` (`searchMods` wrapper) | New args flow through; **regen required**. |
| frontend-shell | `src/routes/Browse.tsx`, `src/routes/BrowsePackInfo.tsx`, new `src/components/FiltersPopover.tsx`, new `src/components/BrowseCard.tsx` (optional), new `src/lib/installedIndex.ts` helper | Bigger cards/nav, filters popup, installed badge. |
| metadata (read-only reuse) | `listInstances()` (`src/lib/ipc.ts`), `["mc-versions"]` + `["loaders", v]` queries | Cross-reference installs; reuse the startup-warmed caches (api-frugality). |

No new Tauri commands. No `SCHEMA_VERSION` bump. No on-disk change.

---

## 2. The installed indicator (requirement 3 — most important)

### Approach: pure frontend, zero backend

`listInstances()` already returns `Instance[]`, each with
`source: Source | null` where `Source` carries `provider: string` + `projectId: string`
(`instances.rs:63-66`, camelCase over IPC). Browse cards carry `pack.provider`
(`ProviderKind` wire value) + `pack.id`. So the cross-reference is purely client-side:

> A Browse card is "installed" iff some instance has
> `source != null && norm(source.provider) === norm(card.provider) && source.projectId === card.id`.

Build an **installed index** once per Browse mount from the `["instances"]` query
(already cached for the Home grid — no extra IPC call):

```ts
// src/lib/installedIndex.ts
type Key = string; // `${normProvider}:${projectId}`
function normProvider(p: string): string { return p.toLowerCase(); } // see §2.1
function installedKeysFromInstances(instances: Instance[]): Map<Key, string /*slug*/> { … }
function isInstalled(index: Map<Key,string>, provider: string, id: string): string | null { … }
```

The index returns the **slug** of the installed instance so the badge can deep-link to
`/instances/<slug>` ("Open instance"). On both the card and the pack-info header, render
an "Installed" pill (distinct color, check icon) when present; the card's primary action
flips from "view/install" emphasis to a secondary "Open instance" affordance.

### 2.1 CRITICAL gotcha — provider-string casing is inconsistent on disk

`Source.provider` is written **three different ways** depending on install path:

| Install path | Code | `Source.provider` value |
|---|---|---|
| Browse one-click (`install_modpack`) | `lib.rs:3029-3030` | `"modrinth"` / **`"curseForge"`** (camelCase) |
| `.mrpack` import | `modpack.rs` mrpack branch | `"modrinth"` |
| CF `.zip` import (`ModEntry`) | `modpack.rs:479` | **`"curseforge"`** (lowercase) |

Meanwhile the card's `pack.provider` is the `ProviderKind` wire value
(`"modrinth"` / `"curseForge"`, camelCase — providers signals §coupling). Therefore the
cross-reference **MUST normalize case** (`.toLowerCase()`) before comparing provider
strings, or CF `.zip`-imported packs will silently fail to match. This is the single
highest-risk detail in the whole rework; it is called out as a checkpoint gate.

(We do not "fix" the on-disk inconsistency in this rework — that is a separate concern
with migration risk. We normalize at the comparison point.)

### 2.2 Why not a backend command

A backend "is this installed?" command would (a) add an IPC round-trip per render, (b)
duplicate data the frontend already holds, and (c) re-read every manifest. The frontend
already has `listInstances()` cached. Pure-frontend is the api-frugal and simplest answer.

---

## 3. Filters popup (requirement 4)

Remove `FacetRow` (the two inline `<select>`s, `Browse.tsx:95-152`). Replace the facet
row with a single **Filters** button (badge showing active-filter count) that opens an
**anchored popover** (`src/components/FiltersPopover.tsx`). The popover holds three groups:

1. **Loaders** — checkbox multi-select: Fabric, Quilt, Forge, NeoForge (the four modpack
   loaders; vanilla excluded — `instances.rs:27`). State: `Set<string>`.
2. **Game version** — single `<select>` dropdown, sourced from the existing
   `["mc-versions"]` query (reuses the startup-warmed cache — api-frugality). `null` = any.
3. **Categories** — multi-select (checkbox chips) from the **unified category list** (§4).
   State: `Set<string>` of unified category keys.

The popover has Apply/Clear; applied filters drive the two infinite queries. Active filter
count surfaces on the Filters button. (Anchored popover, not a modal: closes on
outside-click / Esc; positioned under the button.)

**Loader removed-on-version-change behavior** (current `handleMcVersionChange` resets the
loader) is dropped — loaders are now provider-level facets, not version-scoped builds, so
the reset coupling no longer applies. (Open question Q5.)

---

## 4. Category overlap + unified mapping (the research deliverable)

### 4.1 Provider facts (verified against live APIs — June 2026)

**Modrinth** (`GET /v2/search`, `facets` = JSON array-of-arrays):
- Inner array = **OR**; outer arrays = **AND** (docs, confirmed). e.g.
  `[["categories:fabric","categories:forge"],["versions:1.20.1"]]` = (fabric OR forge) AND 1.20.1.
- **Loaders are filtered via the `categories:` facet** (not a separate `loaders:` facet) —
  docs: "loaders are lumped in with categories in search."
- Modpack categories (live `GET /v2/tag/category`, `project_type==modpack`, `header==categories`),
  **exactly 10**: `adventure`, `challenging`, `combat`, `kitchen-sink`, `lightweight`,
  `magic`, `multiplayer`, `optimization`, `quests`, `technology`. (`kitchen-sink` hyphenated.)
- Source: docs.modrinth.com `/api/operations/searchprojects`, `/categorylist`; live `/v2/tag/category`.

**CurseForge** (`GET /v1/mods/search`):
- `modLoaderType` is a **SINGLE** `ModLoaderType` enum (Any=0, Forge=1, Cauldron=2,
  LiteLoader=3, Fabric=4, Quilt=5, NeoForge=6). A separate plural `modLoaderTypes` (list,
  max 5, comma/array string) **overrides** the singular — so >1 loader is possible only via
  `modLoaderTypes`. Both "Must be coupled with gameVersion" to take effect.
- `categoryId` (single) vs `categoryIds` (list, JSON-array string `[a,b]`, **max 10**,
  overrides `categoryId`). **Semantics across ids: treat as OR** (subagent live reading);
  CF docs are ambiguous ("filter by a list") — **VERIFY at build time** (Q7).
- Modpack categories (live `GET /v1/categories?gameId=432&classId=4471`), 19 rows
  (id → name): 4475 Adventure and RPG, 4483 Combat / PvP, 4476 Exploration, 9243 Expert,
  4482 Extra Large, 4487 FTB Official Pack, 4479 Hardcore, 7418 Horror, 4473 Magic,
  4480 Map Based, 4477 Mini Game, 4484 Multiplayer, 4478 Quests, 10683 RLCraft, 4474 Sci-Fi,
  **4736 Skyblock**, 4481 Small / Light, 4472 Tech, **5128 Vanilla+** (slug `vanilla`).
- Source: docs.curseforge.com/rest-api (Search Mods, Categories, ModLoaderType); live
  `/v1/categories?gameId=432&classId=4471`.

### 4.2 Unified category mapping table

Each unified label queries each provider with the right per-provider value. Modrinth uses
the facet string; CF uses the numeric `categoryId`. Rows with one side blank are
single-provider — the UI still shows them but only one provider returns results.

| Unified label | Modrinth facet (`categories:`) | CF category id (name) | Notes |
|---|---|---|---|
| Adventure & RPG | `adventure` | 4475 (Adventure and RPG) | strong overlap |
| Combat / PvP | `combat` | 4483 (Combat / PvP) | strong overlap |
| Magic | `magic` | 4473 (Magic) | strong overlap |
| Tech | `technology` | 4472 (Tech) | strong overlap (label differs) |
| Quests | `quests` | 4478 (Quests) | strong overlap |
| Multiplayer | `multiplayer` | 4484 (Multiplayer) | strong overlap |
| Kitchen Sink / Large | `kitchen-sink` | 4482 (Extra Large) | approximate map (Q6) |
| Lightweight | `lightweight` | 4481 (Small / Light) | approximate map |
| Challenging | `challenging` | 4479 (Hardcore) | approximate; CF also 9243 Expert (Q6) |
| Optimization | `optimization` | — | **Modrinth-only** |
| Exploration | — | 4476 | **CF-only** |
| Skyblock | — | 4736 | **CF-only** |
| Sci-Fi | — | 4474 | **CF-only** |
| Map Based | — | 4480 | **CF-only** |
| Mini Game | — | 4477 | **CF-only** |
| Horror | — | 7418 | **CF-only** |
| Vanilla+ | — | 5128 | **CF-only** |
| FTB Official | — | 4487 | **CF-only** |
| RLCraft | — | 10683 | **CF-only** |
| Expert | — | 9243 | **CF-only (or fold into Challenging — Q6)** |

**Finding:** only **9** unified categories meaningfully overlap (6 strong + 3 approximate);
Modrinth has 1 unique (optimization); CF has ~10 unique. The mapping lives in **one frontend
table** (`src/lib/categoryMap.ts`); the backend stays provider-agnostic and just receives a
`Vec<String>` of already-resolved per-provider values (see §5.2). Single-provider rows can
be visually tagged (e.g. a provider glyph) so users understand why a filter narrows results
to one feed. (Q6 settles the recommended initial unified set.)

---

## 5. Backend `SearchParams` changes

### 5.1 Shape

Extend `SearchParams` (`providers.rs:160-176`) — both new fields `#[serde(default)]` so old
callers/round-trips stay valid:

```rust
pub struct SearchParams {
    pub query: String,
    pub mc_version: Option<String>,
    pub loader: Option<String>,          // KEEP for back-compat (single); may be unused by Browse
    #[serde(default)] pub loaders: Vec<String>,     // NEW: multi-loader (fabric/quilt/forge/neoforge)
    #[serde(default)] pub categories: Vec<String>,  // NEW: per-provider category VALUES (already resolved by FE)
    pub offset: u32,
    pub limit: u32,
    #[serde(default)] pub project_type: ProjectType,
}
```

**Key design decision: the frontend resolves the unified→per-provider category value** (via
`categoryMap.ts`) and sends the provider's *own* value(s) in `categories`. So when calling the
Modrinth provider the frontend sends `["adventure","magic"]` (Modrinth ORs them); when calling
CF — because CF categories AND, not OR (§5.3) — the frontend sends **at most one** CF value,
e.g. `["4475"]`. The backend providers stay dumb string-passers — no mapping table in Rust, no
Rust knowledge of the taxonomy, and **no AND/OR policy in Rust** (the FE decides how many values
to send per provider). This keeps the per-provider value + policy coupling in one place (the FE
table) and avoids a second source of truth.

`loaders` is provider-neutral loader names (`"fabric"` …); each provider maps them to its own
form (Modrinth → `categories:fabric` facet; CF → single `modLoaderType`, per the §5.3
singular-or-Any recommendation).

### 5.2 Modrinth `build_search_url` (`modrinth.rs:201-225`)

- For each loader in `loaders`, add to a **single inner OR array**:
  `["categories:fabric","categories:forge",…]` (one outer entry, OR'd).
- For each category value in `categories`, add `["categories:<val>",…]` — also OR within one
  inner array (so categories OR among themselves), AND against loaders/version (separate outer
  arrays). This matches "Adventure OR Magic" intent within the category group.
- Keep `versions:` and `project_type:` as today. Empty vecs → omit the array.

### 5.3 CurseForge `build_search_url` (`curseforge.rs:324-350`) — multi-loader handling

CF's single `modLoaderType` is the hard constraint. Options (Q1, recommendation below):

- **(A) `modLoaderTypes` plural** — pass all selected loaders as a comma/array string (max 5,
  covers our 4). Requires `gameVersion` also set to take effect (CF rule). The plural form is
  **less battle-tested** than the singular (a second independent research pass flagged it as
  unreliable in practice).
- **(B) query-per-loader + merge** — N CF calls, union results. Violates api-frugality (N×).
- **(C) singular-or-Any** — exactly 1 loader → `modLoaderType=<id>`; >1 loaders → send **`Any`
  (0) / omit** and do NOT loader-filter the CF feed (Modrinth still ORs precisely, so the
  merged feed stays loader-narrowed on the Modrinth side). 0 → omit. One CF call, no unreliable
  plural param, no extra `ProjectSummary` field. Cost: CF results in a >1-loader selection are
  not loader-filtered — acceptable, since CF search hits carry no loader tag to filter on and
  the user's intent ("any of these loaders") is approximated by Any.
- **(D) client-side filter post-fetch** — `ProjectSummary` has no loader field today → would
  need a backend add + per-hit `/files` calls (expensive). Rejected.

**Recommendation: (C) singular-or-Any.** Avoids the unreliable plural param and needs no new
fields, at the cost of not narrowing the CF feed when >1 loader is picked. (A) stays a fallback
**only if** a live keyed test shows `modLoaderTypes` works reliably (Q1).

**CF categories use AND, not OR** (opposite of Modrinth) — flagged by the second research pass;
CF docs are ambiguous. Passing multiple `categoryIds` therefore risks a near-empty CF feed (a
pack must match ALL selected ids). **Recommendation: send at most ONE CF `categoryId`** — when
the user selects >1 category, use the first that maps to CF (or omit `categoryIds` entirely)
rather than AND-ing them. Modrinth still ORs all selected categories. This keeps the merged feed
sensible. **Verify the AND-vs-OR semantics live before locking this in (Q7).**

**Caveat (all options):** CF requires a `gameVersion` for loader filters to apply — when the
user selects loaders but no version, CF loader filtering is silently ignored. Document this in
the popover (a subtle hint), or disable loader filters until a version is chosen (Q1/Q5).

### 5.4 Command + IPC

`search_mods` (`lib.rs:1133-1170`) gains `loaders: Option<Vec<String>>` +
`categories: Option<Vec<String>>` args (Option for back-compat; default to empty). The
`searchMods` wrapper (`ipc.ts:232-244`) gains the two arrays. **`bindings.ts` regen required**
(`scripts/build.sh dev` on Windows → wait for `[bindings] exported` → stop).

---

## 6. Bigger UI direction (requirements 1 + 2)

- **Cards:** today a 1-column compact row list (`Browse.tsx:371-418`, `size-10` icon,
  `text-sm`). New direction: a **responsive grid of large cards** (e.g.
  `grid-cols-1 sm:grid-cols-2 xl:grid-cols-3`), each card ~`size-16/20` icon, larger title,
  2–3 line summary, download count, category chips, provider badge, and the installed pill
  (§2). Keep navigation-to-pack-info on card click; keep the external-link secondary action.
- **Nav/search:** promote the search to a tall, centered, prominent search field (larger
  input, bigger icon, maybe a subtle container) at the top, with the **Filters** button to its
  right and the active-filter chips below. The page header ("Browse" / subtitle) can shrink or
  merge into the search region to give the search visual primacy (requirement 2).
- All visual; no data-shape change. Uses existing theme tokens (`styles.css`) + `cn` helper.

---

## 7. Rejected approaches

- **Backend category mapping table** — rejected: would create a second source of truth (FE
  needs labels anyway). FE resolves unified→per-provider; backend stays a string-passer (§5.1).
- **Backend "installed?" command** — rejected: extra IPC + manifest reads for data the FE
  already caches (§2.2).
- **CF query-per-loader merge (option B)** — rejected: N× CF calls violates api-frugality.
- **CF client-side loader filter post-fetch (option D)** — rejected: `ProjectSummary` carries
  no loader list, so it'd require a backend field add + per-hit `/files` calls. The chosen
  singular-or-Any (option C, §5.3) sidesteps this.
- **CF multi-category AND (`categoryIds`)** — rejected: CF ANDs ids, so >1 category yields a
  near-empty feed; send at most one CF `categoryId` instead (§5.3).
- **Normalizing `Source.provider` on disk** — rejected for this rework: migration risk;
  normalize at the comparison point instead (§2.1).

---

## 8. Open questions (for human approval)

See the spec header for the same list; consolidated here:

1. **CF multi-loader:** confirm the **singular-or-Any** recommendation (option C, §5.3) — when
   >1 loader is selected, CF gets `Any` and is not loader-filtered (Modrinth still ORs). Or do
   we want to risk the less-reliable `modLoaderTypes` plural (option A) to also narrow CF? Note
   CF ignores loader filters unless a `gameVersion` is set regardless.
2. **Installed-indicator action:** should the installed pill deep-link to `/instances/<slug>`
   ("Open instance"), or just show a passive badge? (Recommend: clickable → Open instance.)
3. **Bigger cards layout:** grid (recommended) vs a wider single-column list? Target columns
   at common widths?
4. **Filters popover vs inline:** confirm an anchored popover (not a slide-over / full modal)
   is the intended "anchored popup". Apply-on-change or explicit Apply button?
5. **Loader×version coupling:** drop the current "reset loader when version changes" behavior
   (loaders are now provider facets)? And do we disable loader filters until a version is set
   (because CF ignores them otherwise)?
6. **Unified category set:** approve the 9-overlap + CF-unique list in §4.2? Specifically:
   keep `optimization` (Modrinth-only) and the CF-only rows visible with a provider tag, or
   hide single-provider categories? Fold CF "Expert" into "Challenging"?
7. **CF category semantics:** a second research pass reports CF ANDs `categoryIds` (vs
   Modrinth OR) — so we plan to send **at most one** CF `categoryId` (§5.3). Docs are
   ambiguous; the builder must verify against a live keyed call before shipping. Accept
   "send-one-CF-category" + a build-time verify gate?
8. **Pagination with merged feed + filters:** current merged-feed dedupe+sort-by-downloads
   stays; confirm no change to the two-independent-infinite-queries model.
