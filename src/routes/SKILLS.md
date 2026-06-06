# SKILLS — src/routes/

One component per page. Routes are registered in `src/router.tsx` (nested under `AppShell`).

## Files / routes
- `Home.tsx` — `/instances`. Instance grid (empty state for now) + the live IPC status pill
  (`app_info`) proving the React↔Rust bridge. Remove the pill once real instances exist.
- `InstanceDetail.tsx` — `/instances/:slug`. Stub; mods/versions/logs come in Phase 1.
- `Browse.tsx` — `/browse`. Search bar + provider tabs (All/Modrinth/CurseForge); results
  wired in Phase 5.
- `Accounts.tsx` — `/accounts`. Microsoft sign-in lands in Phase 3.
- `Settings.tsx` — `/settings`. Launcher prefs (memory, CF API key, Java); persisted later.

## Conventions
- Page header pattern: `<h1 class="text-2xl font-semibold">` + muted subtitle.
- Empty states name the roadmap phase that fills them (e.g. "arrives in Phase 5").
- Data via TanStack Query hooks calling `@/lib/ipc` wrappers — no inline `invoke`.
- Add a new page: create the component here, register it in `router.tsx`, and (if it needs
  nav) add it to the `NAV` array in `components/Sidebar.tsx`.
