# Spec — Instances tab (Home) redesign

Status: approved (plan `C:\Users\drgor\.claude\plans\agile-stargazing-wall.md`). Frontend-only;
no backend / IPC / Rust changes. Standing constraint [[api-frugality]]: the grid renders only
metadata already on each `Instance` — **no** `refreshPackMeta`/`searchMods`/etc. from Home.

## Goal

Rework `src/routes/Home.tsx` from small static cards into a Browse-style responsive card grid:
real pack icons, full stats, per-card Play/Stop, an update-available pill, and a search + sort
bar. Visual language matches `src/components/BrowseCard.tsx` / `src/routes/Browse.tsx`.

## Reusable building blocks (import, do not duplicate)

- `launchInstance(slug)` / `killInstance(slug)` — `src/lib/ipc.ts:190,195`.
- Run status: `useAppStore((s) => s.runs.get(slug))` (keyed by slug); active =
  `status === "preparing" || "running"` (`src/lib/store.ts`).
- `labelLoader`, `formatDate`, `formatPlaytime` — exported from `src/routes/InstanceDetail.tsx`
  (TechTab already imports from this route file — precedent).
- `ProviderBadge` — `src/components/ProviderBadge.tsx` (wants `ProviderKind` camelCase).
- Link-root + inner-button `preventDefault()/stopPropagation()` pattern — `BrowseCard.tsx`.

## Checkpoints

### CP1 — `toProviderKind` helper + `InstanceCard` component
- **CP1a** Export `toProviderKind(wire: string): ProviderKind` from `ProviderBadge.tsx`
  (`"curseforge" | "curseForge"` → `"curseForge"`, else `"modrinth"`). Pure mapper.
- **CP1b** NEW `src/components/InstanceCard.tsx`, props `{ instance: Instance }`. Mirror
  `BrowseCard.tsx`:
  - Root `<Link to="/instances/:slug">`; all inner buttons call `e.preventDefault();
    e.stopPropagation()`.
  - **Icon**: `instance.source?.iconUrl` → `<img referrerPolicy="no-referrer" loading="lazy"
    className="size-16 shrink-0 rounded-xl object-cover">`; fallback lucide tile (`Box`) like
    BrowseCard's `Package` fallback. Do not use `instance.icon`.
  - **Header**: name (`font-semibold`) + `<ProviderBadge provider={toProviderKind(source.provider)}>`
    when `source`; **update pill** (amber, e.g. `bg-amber-500/15 text-amber-400`) when
    `updateAvailable = !!(s?.latestVersionId && s.latestVersionId !== s.fileId)`; small lock
    glyph (lucide `Lock`) when `packLocked`.
  - **Stat line** (muted): `v{packVersion}` (if source) · `minecraft` ·
    `labelLoader(loader.kind)`(+` {version}` if present) · `{mods.length} mods`.
  - **Footer**: Play/Stop button left; `formatPlaytime(totalPlaytimeSec)` + last-played
    (`lastPlayed ? formatDate(lastPlayed) : "Never"`) right.
    - `const run = useAppStore((s) => s.runs.get(instance.slug))`. Running → green **Stop**
      (`killInstance(slug)`) + pulse dot; else **Play** (`launchInstance(slug)`) with local
      `launching` state (disable + spinner). If launching while already running, `confirm()`
      first (mirror `InstanceDetail.tsx:159`). On error: `console.error` (use a toast helper if
      one exists in `store.ts`; otherwise console only).
  - **Delete**: hover trash button + `confirm()` (port from current `Home.tsx:88-99`);
    `deleteInstance(slug)` mutation invalidating `["instances"]`.
- **Done when**: `scripts/build.sh check` passes; component renders without runtime error in the
  running dev window.

### CP2 — Home grid + search/sort
- `src/routes/Home.tsx`:
  - Add `search` state with ~300ms debounce (mirror `Browse.tsx:47,68` rawQuery/query) and
    `sort` state (`"lastPlayed" | "name" | "created" | "playtime"`, default `"lastPlayed"`).
  - **Controls row** under the existing title header: search field styled like `Browse.tsx:129`
    + a compact styled `<select>` for sort. Keep "New instance" button in the header.
  - Derive list: case-insensitive name-substring filter, then sort —
    `lastPlayed` most-recent-first (`null` last); `name` localeCompare A–Z; `created`
    newest-first; `playtime` `totalPlaytimeSec` desc.
  - **Grid**: replace `minmax(220px,1fr)` with `grid grid-cols-1 sm:grid-cols-2 xl:grid-cols-3
    gap-3` (`Browse.tsx:375`); render `<InstanceCard>`. Remove the old inline `InstanceCard`.
  - Keep the empty state; add a "no matches" message when search hides all instances.
- **Done when**: `scripts/build.sh check` passes; Home renders the new grid; search + each sort
  order behave; Play/Stop/delete/update-pill verified in the dev window.

## Out of scope
Backend/IPC/Rust changes; custom `instance.icon`; keepLauncherOpen minimize on card-launch;
removing the duplicate non-exported `formatDownloads` in `InstanceDetail.tsx` (optional cleanup).

## Verification
`scripts/build.sh check` (tsc, no Rust change). Manual in the running dev window: icons +
fallback, stat line, provider badge, Play→Running indicator→Stop, update pill on a stored
`latestVersionId != fileId`, search filter, all sort orders, delete, empty + no-match states.
No frontend test suite (project convention).

## Iteration 2 — card enrichment + Play-on-right

User feedback after iteration 1: bigger picture (done, `size-28`), plus add **category chips**
and a **short description** to instance cards (like BrowseCard), and move the **Play button to
the right as a big purple icon** that changes color on hover.

### CP3 — Backend: capture pack categories + short summary (API-frugal)
`instances::Source` has no `summary`/`categories`. Capture them via the **existing**
`get_pack_summary` fetch in `refresh_pack_meta` — zero extra API calls.
- `src-tauri/src/core/instances.rs`: add `#[serde(default)] pub summary: Option<String>` and
  `#[serde(default)] pub categories: Vec<String>` to `Source` (lines 63–92). Old manifests load
  as `None`/`[]`.
- `src-tauri/src/core/providers.rs`: extend `PackSummary` (line 357) with
  `summary: Option<String>` + `categories: Vec<String>`.
- Both `get_pack_summary` impls (`modrinth.rs`, `curseforge.rs`): populate the two new fields
  from the project response already being parsed (CF project has `summary`+`categories`;
  Modrinth has short description + `categories`). No new HTTP request.
- `src-tauri/src/lib.rs` `refresh_pack_meta` (~line 1345): write `source.summary` +
  `source.categories` alongside `icon_url`/`author`. **Also** force a refresh when the data is
  missing even within the 24h throttle so existing instances backfill once on next open: change
  the early-return guard (~line 1297) to also fall through when
  `source.categories.is_empty() && source.summary.is_none()`.
- `src-tauri/src/lib.rs` `install_modpack` `Source { … }` (~line 3041): set `summary: None,
  categories: vec![]` (refresh backfills; no frontend signature change required).
- Tests: extend the `refresh_pack_meta` / `PackSummary` unit tests to assert the new fields
  populate. Follow the sibling `<stem>_tests.rs` convention.
- Regenerate `src/lib/bindings.ts` (dev window re-exports on Rust rebuild).
- Done when: `scripts/build.sh check` + `scripts/build.sh test` pass.

### CP4 — Frontend: chips + description + Play-on-right (after CP3 bindings exist)
`src/components/InstanceCard.tsx`:
- **Layout**: top row becomes `icon | content (flex-1) | Play button (right, centered)`.
- **Play/Stop**: move out of the footer to the right side as a **big purple icon button** —
  `bg-accent` (the `#7c5cff` purple), large `Play` glyph (~size-7/8), rounded, hover lightens
  (`hover:bg-accent/80` or a brighter shade). Running → red `Square`/Stop. Keep
  `preventDefault`/`stopPropagation`, `launching` spinner, already-running confirm.
- **Description**: `instance.source?.summary` → `line-clamp-2 text-sm text-muted` (like
  BrowseCard's summary).
- **Category chips**: `instance.source?.categories.slice(0,5)` → chip row (copy BrowseCard chip
  markup at `BrowseCard.tsx:120`).
- Playtime + last-played: keep, as a small muted footer line.
- Done when: `scripts/build.sh check` passes; chips/description/purple-Play render in dev window.

## Change log
- 2026-06-20: initial spec from approved plan.
- 2026-06-20: iteration 2 — CP3 backend category/summary capture (API-frugal via existing
  get_pack_summary), CP4 frontend chips + description + Play-on-right purple icon.
