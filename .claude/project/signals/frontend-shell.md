# frontend-shell

## What it does

React 19 app entry point, routing, sidebar navigation (collapsible, with inline login/logout control and a Download Manager toggle), IPC wrapper layer, Zustand stores (task/run state in `useAppStore`, UI state in `useUiStore`), TanStack Query client setup, settings UI, modpack Browse page (per-provider split with sidebar sub-nav), `BrowsePackInfo` detail page, and tabbed instance detail page (Info/Modlist/Tech/Java tabs via React Router `<Outlet>`). All Tauri `invoke` calls are centralized in `src/lib/ipc.ts` — a thin adapter over the generated `src/lib/bindings.ts` (tauri-specta). `AppShell` mounts once, subscribes to all event channels, and hydrates the store from `listTasks()` + `listRunning()` on startup. There is no standalone Accounts route — auth UI lives in the sidebar. `SlideOver.tsx` was removed on ui-overhaul; the Manage Installs panel is now the `ModlistTab` route tab.

## Artifacts

- `src/main.tsx` — app entry: mounts `QueryClientProvider` + `RouterProvider`; calls `prefetchStartupData` fire-and-forget at startup
- `src/router.tsx` — React Router v7 `createBrowserRouter`; routes: `/instances` (Home), `/instances/:slug` (InstanceDetail shell with child routes: `info`, `modlist`, `tech`, `java`), `/browse` (redirects to last-used provider via `useUiStore.getState().browseProvider`), `/browse/:provider` (BrowseProvider), `/browse/:provider/:id` (BrowsePackInfo), `/settings` (Settings); no `/accounts` route
- `src/components/AppShell.tsx` — root layout: Sidebar + `<Outlet />` + `<Toasts />`; subscribes to all event channels via the generated `events.<channel>.listen` surface; hydrates store on mount via `listTasks()` + `listRunning()` + `getRunLogs(slug)` per active run; all subscriptions torn down on unmount
- `src/components/Sidebar.tsx` (503 lines) — collapsible left nav (width 16 collapsed / 60 expanded, animated transition); collapse state from `useUiStore` → persisted across renders; ChevronsLeft/ChevronsRight toggle button; links to Instances / Browse (with sub-items: CurseForge, Modrinth as `<NavLink>` + FTB, ATLauncher as "coming soon" static items) / Settings; `<DownloadManagerButton />`; `<RunningIndicator />`; inline login/logout control (subscribes to `auth://device-code` via `events.authDeviceCode.listen`); version badge `v0.1.0 · pre-alpha` at bottom
- `src/components/DownloadManager.tsx` — `DownloadManagerButton` (toggle + active-count badge) + `DownloadManagerPanel` (fixed left-anchored panel, task list sorted newest-first); `TaskRow`, `StatusBadge`; reads `tasks` slice from `useAppStore`
- `src/components/Toasts.tsx` — `Toasts` component (mounted in `AppShell`, fixed bottom-right); watches `tasks` slice for newly-Done tasks with a `result` field; `shownRef` prevents duplicate toasts; `extractSlug`/`labelFor` helpers
- `src/components/NewInstanceModal.tsx` — create/import dialog with two tabs: **Create** and **Import pack**; routes by extension to `importMrpack` or `importCurseforgeZip` (both return `Promise<number>` task id)
- `src/components/BrowseCard.tsx` (135 lines) — mod/pack card used in Browse grid; shows icon, name, description, download count, provider badge, installed pill; Install/View Detail buttons; used by `BrowseProvider` and wraps `ModpackCard` logic from old Browse
- `src/components/FiltersPopover.tsx` (242 lines) — filters popover for Browse; `FiltersState` type (`loaders: Set<string>`, `mcVersion: string|null`, `categories: Set<string>`); uses `categoryMap.ts` to filter categories by provider; rendered anchored to the filter button ref
- `src/components/PackDescription.tsx` (79 lines) — renders markdown-like pack body HTML returned by `get_pack_info`; used in `BrowsePackInfo`
- `src/components/ProviderBadge.tsx` — inline platform badge (`"Modrinth"` / `"CurseForge"`) with color coding
- `src/components/Toggle.tsx` (38 lines) — reusable toggle switch; used in `JavaTab` and `ModlistTab` for pack-settings / enable-disable controls
- `src/routes/Home.tsx` — instance grid; delete via confirmation; opens `NewInstanceModal`; exports `ImportResultToast` / `CfImportResultToast` (reused in Browse); TanStack Query key `["instances"]`
- `src/routes/Browse.tsx` (419 lines) — `BrowseProvider` component: per-provider browse page driven by `:provider` route param; debounced search, filters popover, infinite scroll via `useInfiniteQuery`; remembers last-used provider in `useUiStore`; `buildInstalledIndex` shows installed pills on cards; FTB/ATLauncher show a "not yet supported" placeholder
- `src/routes/BrowsePackInfo.tsx` (339 lines) — pack detail page at `/browse/:provider/:id`; lazy `getPackInfo` → `PackDescription`; `getModVersions` for version-select modal; Install button calls `installModpack`; "Installed" pill via `installedIndex`
- `src/routes/InstanceDetail.tsx` (1408 lines) — shell component for the tabbed instance view; owns the `getInstance` query, run-state from store, launch/stop logic, `refreshPackMeta` once-per-session call, `enrichInstanceMods` once-per-session backfill call, update-available banner state; passes context to tab children via React Router `<Outlet>` (`InstanceTabContext`: `slug, instance, folderMods, invalidate`); console log panel (capped at 1000 lines); also contains `ManageInstallsPanel` + `ModRow`/`ModSearchCard`/`AddResultSummary`/`ManualEntry` components exported for use by `ModlistTab`
- `src/routes/instance-tabs/InfoTab.tsx` (59 lines) — Info tab: pack source panel (provider badge, version, update button, page URL link), `getPackInfo` lazy details; renders `PackSourcePanel`
- `src/routes/instance-tabs/ModlistTab.tsx` (21 lines) — thin wrapper: renders `ManageInstallsPanel` (imported from `InstanceDetail.tsx`) inside the tab route
- `src/routes/instance-tabs/TechTab.tsx` (98 lines) — Tech tab: instance stats (MC version, loader, memory, Java tier display), folder mod count, effective Java resolution display (read-only)
- `src/routes/instance-tabs/JavaTab.tsx` (295 lines) — Java tab: per-instance Java/RAM config form; `use_pack_settings` toggle; memory slider + min-memory field; JVM args textarea; java path override with file picker + `validateJavaPath` probe; calls `setInstanceJava` on save; reads global settings for defaults display
- `src/routes/Settings.tsx` (233 lines) — loads/saves `Settings` (defaultMemoryMb, defaultJavaArgs, curseforgeApiKey, offlineMode, sidebarStartCollapsed, autoDownloadJava, showConsoleDefault, keepLauncherOpen, maximizeOnStart); displays read-only `AppPaths`
- `src/lib/bindings.ts` (1366 lines, generated, committed) — tauri-specta output: typed `commands.*`, `events.*`, all DTOs. Single source of truth. Never hand-edit; regenerate via `scripts/build.sh dev` on Windows.
- `src/lib/store.ts` (133 lines) — two Zustand stores: `useAppStore` (tasks `Map<number,Task>`, runs `Map<string,RunState>`, runLogs; `upsertTask`, `patchTaskProgress`, `upsertRun`, `appendLog`, `setLogs`); `useUiStore` (`sidebarCollapsed: boolean`, `toggleSidebar`, `setSidebarCollapsed`; `browseProvider: "curseforge"|"modrinth"`, `setBrowseProvider`) — initialized from `localStorage` implicitly via Zustand default, seeded on sidebar init from `settings.sidebarStartCollapsed`
- `src/lib/ipc.ts` (452 lines) — thin adapter over `bindings.ts`; new wrappers added: `getPackInfo`, `refreshPackMeta`, `enrichInstanceMods`, `setInstanceJava`, `validateJavaPath`; `getSettings`/`saveSettings` wrappers; all return-type changes tracked via generated `bindings.ts`
- `src/lib/query.ts` — exports `queryClient` (staleTime=30s, gcTime=24h, retry=1) and `META_STALE_TIME` (6h)
- `src/lib/prefetch.ts` — `prefetchStartupData`: prefetches `["instances"]`, `["mc-versions"]`, `["loaders", latest]`
- `src/lib/utils.ts` — `cn` (clsx + tailwind-merge)
- `src/lib/installedIndex.ts` (41 lines) — `buildInstalledIndex(instances): Map<string,string>` (key = `"<provider>:<project_id>"` → slug); `isInstalled(index, provider, projectId): boolean`; pure, no React/IPC imports
- `src/lib/categoryMap.ts` (84 lines) — `CATEGORY_MAP: CategoryRow[]` table mapping category names to provider slugs; `isSingleProvider(row)`, `resolveCategoriesFor(provider, categories)` — filters to provider-compatible categories for the search params
- `src/styles.css` — Tailwind v4 base + CSS custom properties for theme tokens

## Docs

- `docs/ARCHITECTURE.md` §8 — frontend structure overview
- `docs/design/ui-overhaul.md` — 6-workstream overhaul design doc
- `docs/spec/ui-overhaul.md` — implementation contract for ui-overhaul workstreams
- `docs/spec/browse-rework.md` — Browse per-provider split spec (BR-A through BR-D)
- `docs/spec/browse-providers-split.md` — per-provider sidebar sub-nav spec
- `docs/spec/persistent-bar-update-check.md` — pack update-check persistent bar spec (PB-F1/F2/F3)
- `docs/spec/mod-metadata-ux.md` — mod metadata at add-time + backfill spec
- `docs/spec/download-feedback.md` — honest task status and download progress feedback spec
- `docs/design/storage-auth-reorg.md` — Accounts page removal, auth migration to sidebar
- `docs/spec/download-runner-rework/cp-7-frontend-store-subscription.md` — Zustand store slices, AppShell subscription wiring, hydration

## Coupling

- `bindings.ts` is generated from Rust; a Rust DTO/command/event change requires regenerating `bindings.ts` via `scripts/build.sh dev` on Windows. `ipc.ts` imports from `bindings.ts`; zero hand-declared IPC interfaces.
- `store.ts` imports task/run types from `bindings.ts`; `RunState` and `RunLogLine` are composed types.
- `InstanceDetail.tsx` passes `InstanceTabContext` to all four tab children via React Router `<Outlet context={...}>`. Tab components use `useOutletContext<InstanceTabContext>()`.
- `useUiStore.browseProvider` is read at `/browse` redirect time via `useUiStore.getState()` (outside React) and during `BrowseProvider` render via `useUiStore(s => ...)`. Both forms are valid.
- `JavaTab` calls `getSettings` for global defaults display, `setInstanceJava` to persist, and `validateJavaPath` to probe a custom binary — three separate IPC round-trips.
- `BrowsePackInfo` calls `getPackInfo` (lazy on mount) and `getModVersions` (lazy on version-modal open), then `installModpack` on confirm — three separate IPC calls per install flow.
- `installedIndex.ts` is called with `listInstances()` data inside `Browse.tsx` and `BrowsePackInfo.tsx` via `useQuery(["instances"])` — no separate query; shares the existing instances cache.
- `ProviderKind` response value `"curseForge"` (camelCase) vs routing param `"curseforge"` (lowercase) remain distinct — see providers domain coupling note.
- `Sidebar.tsx` uses `useUiStore` for collapse state. Initial seed on first load can be set via `settings.sidebarStartCollapsed` (written to `settings.json`; `AppShell` calls `setSidebarCollapsed` once after `getSettings` resolves if no prior localStorage state exists).
- `ModlistTab` is a thin wrapper over `ManageInstallsPanel` from `InstanceDetail.tsx`; changes to the panel component only require editing `InstanceDetail.tsx`.

## Conventions worth knowing

- Path alias `@/` maps to `src/` (configured in `vite.config.ts` and `tsconfig.json`).
- Tailwind v4; design tokens are CSS custom properties in `styles.css`.
- `cn` from `src/lib/utils.ts` (clsx + tailwind-merge) is the standard class composition helper.
- TanStack Query mutation success handlers call `invalidateQueries` or `setQueryData`; `InstanceDetail` invalidates `["instance", slug]` after any mod mutation.
- Task/run event subscriptions live exclusively in `AppShell` (mount-once); routes read from the Zustand store, not from local event subscriptions. This is the authoritative pattern.
- `install://log` carries no `instanceId` — the installer runs at most once at a time; `AppShell` attributes all installer lines to whichever run has status `"preparing"` at the time.
- Log console in `InstanceDetail` shown when `running || (runLogLines?.length ?? 0) > 0`; auto-scrolls to bottom.
- `enrichedSlugs` and `refreshedSlugs` are module-level `Set<string>` in `InstanceDetail.tsx` — they gate once-per-session calls regardless of how many times the component mounts/unmounts.
- `BrowseProvider` resets filters to `EMPTY_FILTERS` when `:provider` param changes to avoid stale cross-provider categories.
- `buildInstalledIndex` keys on `"<provider>:<project_id>"`. Provider values are normalized to lowercase (`"modrinth"`, `"curseforge"`) since `ModEntry.provider` is lowercase in the manifest.
