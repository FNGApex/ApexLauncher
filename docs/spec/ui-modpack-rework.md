# UI / modpack-flow rework

## Goal

Browse becomes a unified, ordered **modpack** discovery feed (both providers, platform-badged, click opens project page). Modpack import moves into the New Instance modal. Mod add/manage consolidates into a per-instance "Manage installs" slide-over. Window opens maximized.

## Non-goals

- Installing a modpack directly from Browse (discovery-only — click opens the provider page).
- New provider, auth change, or generated IPC types.
- Changes to importer internals (`core/modpack.rs`) or installer internals (`core/mod_install.rs`) beyond surfacing their entry points.
- Server-side merged search; exact global ordering across providers.

## Success criteria

- [ ] `search_mods` accepts a project-type selector; Modrinth uses `project_type:mod|modpack`, CurseForge uses `classId` `6|4471` accordingly. Existing mod searches unchanged when the selector is "mod".
- [ ] `ProjectSummary` carries `page_url` (camelCase `pageUrl` over IPC): Modrinth = `https://modrinth.com/{project_type}/{slug}`, CurseForge = `links.websiteUrl` verbatim. Populated for both providers.
- [ ] Browse shows **modpacks**: a single ordered list (downloads desc) merging both providers, each card badged with its platform. No side-by-side columns.
- [ ] Clicking a Browse card opens its `pageUrl` in the system browser. No add-to-instance modal on Browse.
- [ ] Missing CF key hides only CF results (inline notice) and still renders Modrinth results.
- [ ] Instances list header has **no** import buttons. The New Instance modal can create a blank instance OR import a `.mrpack` / CurseForge `.zip`.
- [ ] Instance page shows summary + a "Manage installs" control. The slide-over performs mod **search + add** (source toggle CF/Modrinth, project-type "mod") AND **enable/disable/update/remove** of installed mods.
- [ ] Window opens maximized with a sensible min-size floor.
- [ ] `cargo test` green (new provider tests for modpack class + `page_url`); `npm run build` (tsc + vite) green.
- [ ] Each touched domain spec amended to current truth (see Checkpoints).

## Approaches

| # | Approach | Sketch | Cost | Risk |
|---|----------|--------|------|------|
| A | Per-buffer client merge-sort of two independent paginated queries, deduped by `provider:id` | reuse `searchMods` untouched; one merged list UI | low | ordering exact only within loaded buffer |
| B | New server-side merged-search command | one ordered stream | high | over-scoped for discovery-only |

## Recommendation

**A.** Discovery-only Browse needs "roughly popularity-ordered," not globally exact. Reuses existing pagination; smallest backend surface (project-type param + `page_url` only). Evidence: `Browse.tsx` already runs one `useInfiniteQuery` per provider — keep two, merge client-side. Full rationale in `docs/design/ui-modpack-rework.md`.

## Checkpoints

| # | Checkpoint | Files/areas | Agent | Est. files | Verifies |
|---|------------|-------------|-------|------------|----------|
| 1 | Search project-type selector + `page_url` on `ProjectSummary` (both providers, command, ipc mirror) | `core/providers.rs` (SearchParams, ProjectSummary), `core/modrinth.rs`, `core/curseforge.rs`, `lib.rs` (`search_mods`), `src/lib/ipc.ts`, sibling `*_tests.rs`; amend `docs/spec/providers-browse.md` | atomic-builder | ~7 | `cargo test` — modpack-class facet/classId switch + `page_url` normalization tests; existing mod tests still green |
| 2 | Window opens maximized + min-size floor | `src-tauri/tauri.conf.json` | atomic-surgeon | 1 | config parses; window opens maximized in `tauri dev` (manual note) |
| 3 | Browse → unified modpack feed | `src/routes/Browse.tsx` (rewrite: merged ordered list, `ProviderBadge`, modpack project-type, click→`openUrl(pageUrl)`, CF-key inline notice; remove columns + `AddToInstanceModal`); amend `docs/spec/providers-browse.md` | atomic-builder | ~2 | `npm run build` green; Browse renders one badged modpack list |
| 4 | Import moves into New Instance; strip Home import buttons | `src/components/NewInstanceModal.tsx` (Create/Import tabs), `src/routes/Home.tsx` (remove import buttons + result toasts, keep New-instance), reuse import IPC; amend `docs/spec/modpack-import.md` | atomic-builder | ~2 | `npm run build` green; Home header import-button-free; modal imports `.mrpack`/CF `.zip` |
| 5 | Manage installs slide-over | `src/routes/InstanceDetail.tsx` (summary + button; slide-over with mod search+add + source toggle + relocate enable/disable/update/remove), shared `SlideOver`/`ProviderBadge` component(s); amend `docs/spec/mod-install.md` | atomic-builder | ~3 | `npm run build` green; instance page opens a slide-over that adds and manages mods |

## Risks

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Modrinth modpack `page_url` segment wrong for some project types | low | derive from response `project_type`, not a hardcoded `modpack`; test both `mod` and `modpack` cases |
| CF `links.websiteUrl` absent/null in some search rows | med | `page_url` is `Option`; card falls back to disabling the click when `None` |
| Merged-feed dedupe/key collisions across providers | low | key by `provider:id` (already the card-key convention in `Browse.tsx:319`) |
| Manage-installs slide-over regresses existing mod-row ops | med | relocate the existing `ModRow` logic intact; gate on `npm run build` + manual add/remove |
| IPC drift — `ipc.ts` hand-mirrors Rust | med | CP1 updates `ipc.ts` in the same slice as the Rust struct change |

## Change log

<!-- Populated on first amendment after approval. -->

## Implementation log

### shipped — 2026-06-16

Built across 5 checkpoints + 1 polish pass via /subagent-implementation (work-in-place on `main`). Commits (chronological):

- `4582849` — planning: design + spec
- `87ca91f` — CP1 providers: ProjectType (mod|modpack) selector + `page_url` on `ProjectSummary`
- `595e623` — CP2 window: open maximized + min-size floor
- `b7f7b80` — CP3 browse: unified ordered modpack discovery feed (badged, click→page); add-flow removed
- `1c69ac5` — CP4 instances: modpack import moved into the New Instance modal; Home buttons stripped
- `805be94` — CP5 instances: per-instance "Manage installs" slide-over (add + manage); reusable `SlideOver`
- `4e78c67` — polish: cleared all 6 harvested follow-ups (no behavior change)

**Out-of-scope work performed during this build:**
- `npm install` to install the already-declared `@tauri-apps/plugin-dialog` (node_modules was empty; the frontend build was broken pre-existing). Not committed (node_modules gitignored).

**Unforeseens — surprises that emerged during implementation:**
- CP3 rewrite dropped the IntersectionObserver dependency array (regression caught + fixed in review).
- A harvested test nit (F-2) was wrong: the trailing `"` in the modrinth facet assertion is load-bearing (disambiguates `mod` from `modpack`). Kept + documented instead of "fixing".

**Deferred items still open:**
- None. All 6 follow-ups (F-1..F-6) fixed in the polish pass per user disposition.
