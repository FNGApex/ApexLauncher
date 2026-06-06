# SKILLS — src/components/

Shared, reusable UI. Page-specific UI lives in `src/routes/`.

## Files
- `AppShell.tsx` — top-level chrome: fixed `Sidebar` + scrollable `<Outlet />`. Every route
  renders inside it (wired in `router.tsx`).
- `Sidebar.tsx` — left nav (Instances · Browse · Accounts · Settings) using `NavLink` with
  active-state styling; brand header + version footer. Nav items are a `NAV` array — add a
  route by adding an entry (with a `lucide-react` icon).

## Conventions
- Icons from `lucide-react`.
- Style with token utilities (`bg-surface`, `text-muted`, …) + `cn()` from `@/lib/utils`.
- When shadcn-style primitives are introduced, put them under `components/ui/`.
