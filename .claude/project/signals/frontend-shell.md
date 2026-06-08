# frontend-shell

## What it does

React 19 app entry point, routing, sidebar navigation, IPC wrapper layer, TanStack Query client setup, settings UI, and stub routes (Browse, Accounts). All Tauri `invoke` calls are centralized in `src/lib/ipc.ts` with hand-typed TS interfaces mirroring Rust structs.

## Artifacts

- `src/main.tsx` — app entry: mounts `QueryClientProvider` + `RouterProvider`; calls `prefetchStartupData` fire-and-forget at startup
- `src/router.tsx` — React Router v7 `createBrowserRouter`; routes: `/instances` (Home), `/instances/:slug` (InstanceDetail), `/browse` (Browse), `/accounts` (Accounts), `/settings` (Settings); root layout wraps with AppShell
- `src/components/AppShell.tsx` — root layout: Sidebar + `<Outlet />`
- `src/components/Sidebar.tsx` — fixed 240px left nav; links to Instances / Browse / Accounts / Settings; shows `v0.1.0 · pre-alpha` at bottom
- `src/routes/Browse.tsx` — stub; provider filter tabs (All / Modrinth / CurseForge) + search input; no backend wiring yet (Phase 5/6)
- `src/routes/Accounts.tsx` — stub placeholder (Phase 3)
- `src/routes/Settings.tsx` — live: loads/saves `Settings` (defaultMemoryMb, defaultJavaArgs, curseforgeApiKey); displays read-only `AppPaths`; dirty-state save button
- `src/lib/ipc.ts` — all typed `invoke` wrappers; exports interfaces for `AppInfo`, `Instance`, `InstanceDetail`, `FolderMod`, `CreateInstanceReq`, `Settings`, `AppPaths`, `McVersion`, `LoaderOption`, `LoaderKind`, `DownloadItem`, `DownloadPlan`, `ItemStatus`, `ItemOutcome`, `PlanResult`, `DownloadProgressPayload`, `ResolveResult`, `LaunchMeta`, `JavaSource`, `JavaInstallation`; functions: `getAppInfo`, `listInstances`, `createInstance`, `getInstance`, `deleteInstance`, `getSettings`, `saveSettings`, `getAppPaths`, `listMinecraftVersions`, `getLoaders`, `executeDownloadPlan`, `resolveVanilla`, `ensureJava`
- `src/lib/query.ts` — exports `queryClient` (staleTime=30s, gcTime=24h, retry=1, no refetch-on-focus) and `META_STALE_TIME` (6h)
- `src/lib/prefetch.ts` — `prefetchStartupData`: prefetches `["instances"]`, `["mc-versions"]`, `["loaders", latest]`
- `src/lib/utils.ts` — `cn` (clsx + tailwind-merge)
- `src/styles.css` — Tailwind v4 base + CSS custom properties for theme tokens (background, surface, surface-2, border, primary, muted, danger, foreground)

## Docs

- `docs/ARCHITECTURE.md` §8 — frontend structure overview
- `docs/ROADMAP.md` Phase 0 — scaffold/shell scope

## Coupling

- `ipc.ts` hand-mirrors Rust struct field names; no generated types yet (specta/ts-rs planned per `docs/ROADMAP.md` cross-cutting section). Any Rust struct rename or new field requires manual `ipc.ts` update.
- `Settings.tsx` is tightly coupled to `src/lib/ipc.ts` `Settings` interface; if `curseforge_api_key` moves from settings to a separate secret store (Phase 5), both files change.
- `prefetch.ts` imports `META_STALE_TIME` from `query.ts` and IPC fns from `ipc.ts`; changes to query key shapes affect both.
- `Browse.tsx` and `Accounts.tsx` are stubs — they will be replaced wholesale in Phase 5 and Phase 3 respectively.
- `JavaInstallation` and `JavaSource` in `ipc.ts` mirror `core/java.rs`; any rename in the Rust struct requires manual update here (java domain coupling).

## Conventions worth knowing

- Path alias `@/` maps to `src/` (configured in `vite.config.ts` and `tsconfig.json`).
- Tailwind v4 is used; design tokens are CSS custom properties in `styles.css`, referenced as `text-muted`, `bg-surface`, `bg-primary`, etc.
- `cn` from `src/lib/utils.ts` (clsx + tailwind-merge) is the standard class composition helper.
- TanStack Query mutation success handlers call `invalidateQueries` or `setQueryData` directly — no global invalidation strategy.
- No Zustand stores exist yet (listed as a dependency in `package.json` but unused in current code).
