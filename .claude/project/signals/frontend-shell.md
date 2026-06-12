# frontend-shell

## What it does

React 19 app entry point, routing, sidebar navigation, IPC wrapper layer, TanStack Query client setup, settings UI, live accounts UI, and live Browse page (Phase 5). All Tauri `invoke` calls are centralized in `src/lib/ipc.ts` with hand-typed TS interfaces mirroring Rust structs. Accounts route is fully implemented (Phase 3). InstanceDetail subscribes to `install://log` events (Phase 4 slice B) in addition to `launch://log` + `launch://exit`.

## Artifacts

- `src/main.tsx` — app entry: mounts `QueryClientProvider` + `RouterProvider`; calls `prefetchStartupData` fire-and-forget at startup
- `src/router.tsx` — React Router v7 `createBrowserRouter`; routes: `/instances` (Home), `/instances/:slug` (InstanceDetail), `/browse` (Browse), `/accounts` (Accounts), `/settings` (Settings); root layout wraps with AppShell
- `src/components/AppShell.tsx` — root layout: Sidebar + `<Outlet />`
- `src/components/Sidebar.tsx` — fixed 240px left nav; links to Instances / Browse / Accounts / Settings; shows `v0.1.0 · pre-alpha` at bottom
- `src/routes/Browse.tsx` — live Browse page (Phase 5 slice A): debounced search (400 ms), MC version + loader facet selectors, All/Modrinth/CurseForge tabs; All tab is two independent side-by-side `ProviderColumn` components; each column uses `useInfiniteQuery` + `IntersectionObserver` sentinel; CF key-absent surfaces as `KeyMissingState` CTA, not a crash
- `src/routes/Accounts.tsx` — fully implemented accounts page (Phase 3): device-code sign-in modal, account list with active indicator, set-active/remove buttons, cancel-in-progress; subscribes to `auth://device-code` event via `listenDeviceCode` before invoking `beginLogin`
- `src/routes/Settings.tsx` — live: loads/saves `Settings` (defaultMemoryMb, defaultJavaArgs, curseforgeApiKey); displays read-only `AppPaths`; dirty-state save button
- `src/routes/InstanceDetail.tsx` — Launch/Stop toggle, running badge, live log console (500-line ring buffer); subscribes to `launch://log` (slug-filtered), `launch://exit` (slug-filtered), and `install://log` (not slug-filtered, prefixed `[install:<stream>]`) simultaneously; all three listeners set up in a single `useEffect` on slug change
- `src/lib/ipc.ts` — all typed `invoke` wrappers; exports interfaces for `AppInfo`, `Instance`, `InstanceDetail`, `FolderMod`, `CreateInstanceReq`, `Settings`, `AppPaths`, `McVersion`, `LoaderOption`, `LoaderKind`, `DownloadItem`, `DownloadPlan`, `ItemStatus`, `ItemOutcome`, `PlanResult`, `DownloadProgressPayload`, `ResolveResult`, `LaunchMeta`, `JavaSource`, `JavaInstallation`, `AccountMeta`, `DeviceCodePayload`, `AuthCommandError`, `InstallLogPayload`, `ProviderKind`, `ProjectSummary`, `ProjectVersion`, `VersionFile`, `Dependency`, `SearchResult`, `ProviderCommandError`; event constants `AUTH_DEVICE_CODE_EVENT`, `LAUNCH_LOG_EVENT`, `LAUNCH_EXIT_EVENT`, `INSTALL_LOG_EVENT`; functions include: `getAppInfo`, `listInstances`, `createInstance`, `getInstance`, `deleteInstance`, `getSettings`, `saveSettings`, `getAppPaths`, `listMinecraftVersions`, `getLoaders`, `executeDownloadPlan`, `resolveVanilla`, `ensureJava`, `launchInstance`, `killInstance`, `listenDeviceCode`, `beginLogin`, `cancelLogin`, `listAccounts`, `getActiveAccountId`, `removeAccount`, `setActiveAccount`, `listenInstallLog`, `searchMods`, `getModVersions`
- `src/lib/query.ts` — exports `queryClient` (staleTime=30s, gcTime=24h, retry=1, no refetch-on-focus) and `META_STALE_TIME` (6h)
- `src/lib/prefetch.ts` — `prefetchStartupData`: prefetches `["instances"]`, `["mc-versions"]`, `["loaders", latest]`
- `src/lib/utils.ts` — `cn` (clsx + tailwind-merge)
- `src/styles.css` — Tailwind v4 base + CSS custom properties for theme tokens (background, surface, surface-2, border, primary, muted, danger, foreground)

## Docs

- `docs/ARCHITECTURE.md` §8 — frontend structure overview
- `docs/ROADMAP.md` Phase 0 — scaffold/shell scope

## Coupling

- `ipc.ts` hand-mirrors Rust struct field names; no generated types yet (specta/ts-rs planned per `docs/ROADMAP.md` cross-cutting section). Any Rust struct rename or new field requires manual `ipc.ts` update.
- `InstallLogPayload` in `ipc.ts` mirrors `lib.rs::InstallLogPayload` (`{ stream, line }`, camelCase); added in Phase 4 slice B.
- `Settings.tsx` is tightly coupled to `src/lib/ipc.ts` `Settings` interface; if `curseforge_api_key` moves from settings to a separate secret store (Phase 5), both files change.
- `prefetch.ts` imports `META_STALE_TIME` from `query.ts` and IPC fns from `ipc.ts`; changes to query key shapes affect both.
- `Browse.tsx` calls `searchMods` and relies on `ProviderCommandError.kind === "key_missing"` for the CF key-absent state; changes to those in providers/lib.rs must propagate here.
- `JavaInstallation` and `JavaSource` in `ipc.ts` mirror `core/java.rs`; any rename in the Rust struct requires manual update here (java domain coupling).
- `AccountMeta`, `DeviceCodePayload`, `AuthCommandError` in `ipc.ts` mirror `core/auth.rs` and `lib.rs`; any rename in auth structs requires manual `ipc.ts` update (auth domain coupling).

## Conventions worth knowing

- Path alias `@/` maps to `src/` (configured in `vite.config.ts` and `tsconfig.json`).
- Tailwind v4 is used; design tokens are CSS custom properties in `styles.css`, referenced as `text-muted`, `bg-surface`, `bg-primary`, etc.
- `cn` from `src/lib/utils.ts` (clsx + tailwind-merge) is the standard class composition helper.
- TanStack Query mutation success handlers call `invalidateQueries` or `setQueryData` directly — no global invalidation strategy.
- No Zustand stores exist yet (listed as a dependency in `package.json` but unused in current code).
- `install://log` carries no `instanceId` — the installer runs at most once at a time, so InstanceDetail attributes all installer lines to the instance currently being launched.
