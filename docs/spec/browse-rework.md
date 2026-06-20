# Browse page rework (spec)

Status: **APPROVED (human, 2026-06-20) — ready to build.** Companion to `docs/design/browse-rework.md`.

**Locked decisions (Q1–Q8):** Q1 CF multi-loader = **singular-or-Any** (>1 loader → CF `Any`, not
narrowed; Modrinth ORs); Q2 installed pill = **clickable → `/instances/<slug>`**; Q3 cards = **responsive
grid**; Q4 = **anchored popover, apply-on-change**; Q5 = **disable loader filters until a game version is
chosen** (CF ignores loader without version); Q6 = **show all categories, tag single-provider ones with a
provider glyph**; Q7 = **send at most one CF `categoryId`** + a build-time live-verify of CF AND/OR before
shipping; Q8 = **keep the merged two-infinite-query feed**.

Contract: rework the modpack **Browse** page + the `search_mods` backend per four
requirements — bigger cards/nav, a Filters popup, an installed indicator, and category +
multi-loader search. Decomposed into **four independently-shippable checkpoint groups**
(BR-A backend, BR-B installed indicator, BR-C filters popup, BR-D bigger cards/nav). BR-A is a
prerequisite for the category/loader parts of BR-C; BR-B and BR-D are independent of A.

**Gate vocabulary.** Backend checkpoints: `scripts/build.sh check` (cargo check + tsc) +
`scripts/build.sh test <filter>`; new tests live in the sibling `<stem>_tests.rs`
(CLAUDE.md → "Rust test layout"). UI checkpoints: `scripts/build.sh check` (tsc) + **smoke-test
in the dev window** (`scripts/build.sh dev`) — there are no frontend unit tests yet. **Any
checkpoint that changes a Rust DTO/command/event MUST regenerate `src/lib/bindings.ts`** via
`scripts/build.sh dev` on Windows (wait for `[bindings] exported`, then stop) — called out inline.

New `SearchParams` fields use `#[serde(default)]`; **no `SCHEMA_VERSION` bump**; no on-disk change.

---

## BR-A — Backend: category + multi-loader search

Closes the gap that `SearchParams` carries only single `loader` + `mc_version` + no categories.
Prerequisite for BR-C's category/loader filters. No UI.

| CP | Deliverable | Files | Done when |
|----|-------------|-------|-----------|
| **A-1** | Extend `SearchParams`: add `#[serde(default)] loaders: Vec<String>` and `#[serde(default)] categories: Vec<String>`; KEEP existing `loader`/`mc_version`. (design §5.1) | `src-tauri/src/core/providers.rs`, `providers_tests.rs` | `scripts/build.sh check` passes; a round-trip test deserializes an old JSON (no new fields) into the struct with both vecs empty. |
| **A-2** | Modrinth `build_search_url`: emit loaders as ONE OR'd inner array `["categories:fabric",…]`; categories as ONE OR'd inner array `["categories:<val>",…]`; keep `versions:` + `project_type:` as separate outer (AND) arrays; empty vecs omitted. (design §5.2) | `src-tauri/src/core/modrinth.rs`, `modrinth_tests.rs` | `scripts/build.sh test modrinth` green; new tests assert the exact facets JSON for (a) 2 loaders → single OR array, (b) 2 categories → single OR array, (c) loaders+categories+version → 3 AND arrays. |
| **A-3** | CF `build_search_url` (design §5.3, **singular-or-Any**): 0 loaders → omit; 1 → `modLoaderType=<id>`; >1 → send `Any` (0) / omit (do NOT use the unreliable `modLoaderTypes` plural unless Q1 says so). Categories: send **at most one** CF `categoryId` (CF ANDs `categoryIds` → >1 yields ~empty); 0 → omit. **Verify CF category OR-vs-AND against a live keyed call before shipping** (Q7). | `src-tauri/src/core/curseforge.rs`, `curseforge_tests.rs` | `scripts/build.sh test curseforge` green; new tests assert URL for 0/1/>1 loaders (>1 → Any/omit) and 0/1 category; a note records the verified CF category semantics. |
| **A-4** | `search_mods` command gains `loaders: Option<Vec<String>>` + `categories: Option<Vec<String>>` args (default empty); threads them into `SearchParams`. | `src-tauri/src/lib.rs` | `scripts/build.sh check` + `test` green; existing `search_mods` callers/tests still compile. |
| **A-5** | Regenerate `bindings.ts`; update `searchMods` wrapper (`ipc.ts`) to accept `loaders: string[]` + `categories: string[]` (default `[]`). | `src/lib/bindings.ts` (generated), `src/lib/ipc.ts` | `scripts/build.sh dev` emits `[bindings] exported`; `scripts/build.sh check` (tsc) passes with new args visible. |

## BR-B — Installed indicator (requirement 3 — most important)

Pure frontend; no backend, no IPC change. Independent of BR-A. Cross-references
`listInstances()` (`source.provider` + `source.projectId`) against each card/pack.

| CP | Deliverable | Files | Done when |
|----|-------------|-------|-----------|
| **B-1** | `src/lib/installedIndex.ts`: build `Map<"<normProvider>:<projectId>", slug>` from `Instance[]`; `normProvider = p.toLowerCase()` (**handles the `"curseForge"` vs `"curseforge"` on-disk casing split — design §2.1**); `isInstalled(index, provider, id) → slug \| null`. Pure functions. | `src/lib/installedIndex.ts` | `scripts/build.sh check` (tsc) passes; functions are pure (no React/IPC import). A short manual reasoning note confirms a CF `.zip`-imported pack (provider `"curseforge"`) matches a card whose provider is `"curseForge"`. |
| **B-2** | Browse feed builds the index once from the `["instances"]` query (reuse cache — **no new IPC call**) and passes installed-slug into each card; card renders an "Installed" pill + flips primary affordance to "Open instance" (→ `/instances/<slug>`). | `src/routes/Browse.tsx` (+ card component) | dev-window: install a pack, return to Browse, its card shows the Installed pill; clicking Open navigates to the instance. `check` passes. |
| **B-3** | Pack-info header shows the same "Installed" pill (+ Open instance) when the pack matches an instance; the Download button stays available (re-install/update is allowed). | `src/routes/BrowsePackInfo.tsx` | dev-window: open the pack-info page of an installed pack → pill visible, Open works; non-installed pack → no pill. `check` passes. |

## BR-C — Filters popup (requirement 4)

Removes the inline `FacetRow`; adds an anchored **Filters** popover (loaders multi-select,
game-version dropdown, categories multi-select). Category/loader filters depend on **BR-A**.

| CP | Deliverable | Files | Done when |
|----|-------------|-------|-----------|
| **C-1** | `src/lib/categoryMap.ts`: the unified-category table from design §4.2 — each row `{ label, modrinth: string\|null, cfId: string\|null }`; a `resolveCategoriesFor(provider, selected) → string[]` helper returning that provider's values (drops rows with no value for the provider). | `src/lib/categoryMap.ts` | `scripts/build.sh check` passes; a manual check confirms `resolveCategoriesFor("modrinth", ["Tech"]) === ["technology"]` and `("curseforge", ["Tech"]) === ["4472"]`. |
| **C-2** | `src/components/FiltersPopover.tsx`: anchored popover (outside-click/Esc close) with Loaders checkboxes (fabric/quilt/forge/neoforge), a game-version `<select>` (from `["mc-versions"]` cache — no new fetch), and category checkbox chips (labels from `categoryMap.ts`). Lifts `Set<loader>`, `mcVersion`, `Set<category>` to Browse. | `src/components/FiltersPopover.tsx`, `src/routes/Browse.tsx` | dev-window: Filters button opens the popover anchored under it; selecting filters updates state; outside-click closes. `check` passes. |
| **C-3** | Remove `FacetRow` (`Browse.tsx:95-152`); add a **Filters** button with an active-filter count badge; render applied filters as chips. Drop the "reset loader on version change" coupling (design §3, Q5). | `src/routes/Browse.tsx` | dev-window: old two dropdowns gone; Filters button + count badge present. `check` passes. |
| **C-4** | Wire filters into both infinite queries: pass `loaders` (as `string[]`) and `resolveCategoriesFor(provider, …)` per provider into `searchMods`; include filter state in the TanStack query keys so changes refetch. | `src/routes/Browse.tsx` | dev-window: selecting Fabric+Forge and a category narrows the merged feed; both providers receive the right per-provider values (verify via network/log). `check` passes. **Depends on BR-A.** |

## BR-D — Bigger cards + nav/search (requirements 1 + 2)

Pure frontend visual rework. Independent of BR-A/BR-B (composes with B's pill + C's filters).

| CP | Deliverable | Files | Done when |
|----|-------------|-------|-----------|
| **D-1** | Card rework: larger responsive grid card (icon ~`size-16/20`, larger title, 2–3 line summary, download count, category chips, provider badge; slot for the BR-B pill). Keep click→pack-info + external-link secondary action. | `src/routes/Browse.tsx` (+ optional `src/components/BrowseCard.tsx`) | dev-window: cards are visibly larger in a grid; layout holds at 800px min width; `check` passes. |
| **D-2** | Nav/search rework: prominent, larger centered search field with the Filters button beside it and applied-filter chips below; header de-emphasized/merged (design §6). | `src/routes/Browse.tsx` | dev-window: search field is the visual focus; Filters button adjacent; debounce still works; `check` passes. |

---

## Sequencing

- **BR-A** first (unblocks BR-C's category/loader wiring at C-4).
- **BR-B** and **BR-D** are independent — can land in parallel / any order.
- **BR-C** C-1..C-3 only need the frontend; **C-4 depends on BR-A** (the new `searchMods` args).
- Recommended single integration branch `browse-rework` with the above order; each CP gated as
  stated. The `bindings.ts` regen (A-5) is the only Windows-dev-window-required step.

## Risk callouts (carry into the build)

- **Provider-string casing (B-1):** the on-disk `Source.provider` is `"curseForge"` (Browse
  install) OR `"curseforge"` (CF `.zip` import) OR `"modrinth"`. Compare case-insensitively or
  CF `.zip` packs won't show as installed. Highest-risk detail.
- **CF multi-loader (A-3):** singular-or-Any — >1 loader sends CF `Any` (not loader-narrowed);
  Modrinth still ORs. The `modLoaderTypes` plural is unreliable; use only if Q1 confirms live.
- **CF loader+version coupling (A-3/C-4):** CF ignores `modLoaderType` unless `gameVersion` is
  also set. Decide (Q5) whether to disable loader filters until a version is chosen.
- **CF categories AND (A-3):** CF ANDs `categoryIds` (vs Modrinth OR) → send at most one CF
  `categoryId`; verify the AND/OR semantics live before shipping (Q7).

## Open questions

Blocking human decisions live in `docs/design/browse-rework.md` §8 (Q1–Q8). Summary: CF
multi-loader strategy (Q1), installed-pill action (Q2), card layout (Q3), popover behavior
(Q4), loader×version coupling (Q5), the unified category set (Q6), CF `categoryIds` semantics
(Q7), merged-feed pagination (Q8).

## Change log

- 2026-06-20 — Initial spec drafted (design + research complete; provider APIs verified live).
  Awaiting human approval of §8 open questions before build.
