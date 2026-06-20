# Browse: per-provider split + sidebar sub-nav (spec)

Status: APPROVED (human, 2026-06-20). Branch: `ui-overhaul`. Frontend only (search backend already
single-provider). Decouples the merged Browse feed into per-provider browse pages, surfaced as sidebar
sub-items, to scale to FTB + ATLauncher later.

## Locked decisions
- Provider sub-categories live **nested in the left sidebar** under "Browse": CurseForge, Modrinth
  (active), FTB, ATLauncher (**grayed "coming soon"**, not clickable).
- **Remember last-used** provider (persisted; default CurseForge on first run). Clicking "Browse" or
  landing on `/browse` goes to the last-used provider.
- Each provider page is a **single-provider feed** (no merge/dedupe/compatibility) — simplifies the
  current `MergedFeed`. Filters are scoped to that provider.

## Checkpoints

### PS-1 — last-used provider state
| Deliverable | Files |
|---|---|
| Add `browseProvider: "curseforge" \| "modrinth"` (default `"curseforge"`) + `setBrowseProvider` to the persisted `useUiStore` (`src/lib/store.ts`). | `src/lib/store.ts` |

### PS-2 — routing
| Deliverable | Files |
|---|---|
| `router.tsx`: `/browse` → a tiny redirect element that reads `useUiStore.browseProvider` and `<Navigate to={/browse/${p}} replace/>`. `/browse/:provider` → `<BrowseProvider/>` (the per-provider feed; handles curseforge/modrinth/ftb/atlauncher). KEEP `/browse/:provider/:id` → `<BrowsePackInfo/>` (pack info — unchanged; the 2-segment vs 3-segment routes don't collide). | `src/router.tsx` |

### PS-3 — sidebar nested Browse
| Deliverable | Files |
|---|---|
| `Sidebar.tsx`: "Browse" stays a nav item (NavLink to `/browse/<lastUsed>`); when the sidebar is EXPANDED, render indented sub-items under it: **CurseForge** → `/browse/curseforge`, **Modrinth** → `/browse/modrinth` (NavLinks, active-highlighted), **FTB** + **ATLauncher** as disabled/greyed rows with a "Soon" tag (not links, `cursor-not-allowed`, title="Coming soon"). When COLLAPSED (icon-only), show only the Browse icon (→ last-used); no sub-items. | `src/components/Sidebar.tsx` |

### PS-4 — BrowseProvider (single-provider feed)
| Deliverable | Files |
|---|---|
| Refactor `Browse.tsx` into `BrowseProvider` parameterized by `useParams().provider`. For `curseforge`/`modrinth`: ONE `useInfiniteQuery` (drop the second provider + the whole merge/dedupe/`mrCompatible`/`cfCompatible` block); reuse the search field, FiltersPopover, filter chips, and the responsive `BrowseCard` grid. On mount/param-change, call `useUiStore.setBrowseProvider(provider)` (remember-last-used) for the two real providers. For `ftb`/`atlauncher`: render a centered **"Coming soon"** placeholder. Keep `searchMods` single-provider with the route provider; keep the installed-index pill. | `src/routes/Browse.tsx` |

### PS-5 — provider-scoped filters
| Deliverable | Files |
|---|---|
| `FiltersPopover` gains a `provider: "curseforge" \| "modrinth"` prop and shows only the categories valid for that provider (rows where that provider's value is non-null) — no single-provider tags needed anymore (it's one provider). Loaders: keep the checkboxes; for CurseForge apply the singular-or-Any rule when querying (>1 → omit); Modrinth ORs. | `src/components/FiltersPopover.tsx`, `src/routes/Browse.tsx` |

## Notes
- No backend/bindings change. Gate: `scripts\apex-build.bat check` (tsc) + dev-window smoke.
- The merged-feed model (Q8 from the prior rework) is intentionally REPLACED by the per-provider split —
  cleaner and removes the CF/Modrinth category-AND/OR + compatibility juggling.
- Collapsed-sidebar sub-items are omitted (icon-only); the Browse icon → last-used provider.
