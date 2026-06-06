# SKILLS — src/ (frontend root)

React frontend. Renders only; all network/filesystem/process work goes to Rust over IPC.

## Stack
- React 19 + TypeScript, bundled by Vite 7.
- **Tailwind v4** (via `@tailwindcss/vite`) — no `tailwind.config.js`; theme tokens live in
  `styles.css` under `@theme inline`, fed by CSS variables (dark default, `.light` overrides).
- **React Router** (`router.tsx`) for navigation.
- **TanStack Query** for server/IPC data caching (provider in `main.tsx`).
- **Zustand** for local UI state (stores land in `src/stores/` when needed — not created yet).
- Path alias `@/` → `src/` (configured in both `vite.config.ts` and `tsconfig.json`).

## Layout
- `main.tsx` — entry; wraps `<RouterProvider>` in `<QueryClientProvider>`; imports `styles.css`.
- `router.tsx` — route table; all routes nest under `AppShell`.
- `styles.css` — Tailwind import + theme tokens + base/scrollbar styles.
- `lib/` — IPC wrappers and helpers (see its SKILLS.md).
- `components/` — shared UI (see its SKILLS.md).
- `routes/` — one component per page (see its SKILLS.md).

## Conventions
- Use semantic color utilities tied to tokens: `bg-background`, `bg-surface`, `text-muted`,
  `bg-primary`, `border-border`, etc. — never hard-code hex so theming keeps working.
- Reach Rust **only** through typed wrappers in `lib/ipc.ts`; never call `invoke` inline.
- Keep components presentational; fetch via TanStack Query hooks.
- Empty-state UIs reference the roadmap phase that will fill them in.
