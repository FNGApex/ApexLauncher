# frontend-shell

## What it does

React 19 app entry point, routing, sidebar navigation (with inline login/logout control and a Download Manager toggle), IPC wrapper layer, Zustand store (tasks + runs slices), TanStack Query client setup, settings UI, modpack Browse page (discovery-only, unified feed), and instance detail page with a "Manage installs" slide-over for mod management. All Tauri `invoke` calls are centralized in `src/lib/ipc.ts` — a thin adapter over the generated `src/lib/bindings.ts` (tauri-specta). `AppShell` mounts once, subscribes to all `task://progress`, `task://update`, `run://update`, `launch://log`, `launch://exit`, `install://log` events via the generated `events.*` surface, and hydrates the store from `listTasks()` + `listRunning()` on startup. There is no standalone Accounts route — auth UI lives in the sidebar.

## Artifacts

- `src/main.tsx` — app entry: mounts `QueryClientProvider` + `RouterProvider`; calls `prefetchStartupData` fire-and-forget at startup
- `src/router.tsx` — React Router v7 `createBrowserRouter`; routes: `/instances` (Home), `/instances/:slug` (InstanceDetail), `/browse` (Browse), `/settings` (Settings); index redirects to `/instances`; no `/accounts` route
- `src/components/AppShell.tsx` — root layout: Sidebar + `<Outlet />` + `<Toasts />`; subscribes to all 6 event channels via the generated `events.<channel>.listen` surface (`events.taskUpdate.listen` → `upsertTask`, `events.taskProgress.listen` → `patchTaskProgress`, `events.runUpdate.listen` → `upsertRun`, `launch://log` / `launch://exit` / `install://log` → `appendLog`); hydrates store on mount via `listTasks()` + `listRunning()` + `getRunLogs(slug)` per active run; all subscriptions are torn down on unmount (never in production, but teardown is correct)
- `src/components/Sidebar.tsx` — fixed left nav; links to Instances / Browse / Settings; `<DownloadManagerButton />` for the Download Manager panel; `<RunningIndicator />` (reads `runs` slice from store, shows count of preparing+running instances); inline login/logout control (subscribes to `auth://device-code` via `events.authDeviceCode.listen`); version badge `v0.1.0 · pre-alpha` at bottom
- `src/components/DownloadManager.tsx` (254 lines) — `DownloadManagerButton` (toggle + active-count badge; reads `tasks` slice from store) + `DownloadManagerPanel` (fixed left-anchored panel, task list sorted newest-first); `TaskRow` shows parent label, current child, `done/total` progress bar, cancel button (calls `cancelTask`); `StatusBadge` maps all seven `TaskStatus` variants to colored badges; `isActive` helper covers queued/planning/downloading/applying
- `src/components/Toasts.tsx` (117 lines) — `Toasts` component (mounted in `AppShell`, fixed bottom-right); watches `tasks` slice for newly-Done tasks with a `result` field; shows one toast per completed task (label from result + "Open" navigate button if result has a `slug`); `shownRef` prevents duplicate toasts; `extractSlug` / `labelFor` helpers parse `TaskResult` union
- `src/components/NewInstanceModal.tsx` — create/import dialog with two tabs: **Create** (MC version + loader build selectors, calls `createInstance`) and **Import pack** (single file picker via `@tauri-apps/plugin-dialog`'s `open()`, routes by extension to `importMrpack` or `importCurseforgeZip` — both return `Promise<number>` task id; toast shown via `Toasts` component)
- `src/routes/Home.tsx` — instance grid; delete via confirmation; opens `NewInstanceModal`; exports `ImportResultToast` / `CfImportResultToast` named (reused in Browse); TanStack Query key `["instances"]`
- `src/routes/Browse.tsx` — modpack discovery feed: debounced search, MC version + loader facets; single merged feed from both providers; each `ModpackCard` Install button calls `installModpack` → returns task id; result shown via `Toasts` + imported toast components from `Home.tsx`
- `src/routes/Settings.tsx` — loads/saves `Settings` (defaultMemoryMb, defaultJavaArgs, curseforgeApiKey); displays read-only `AppPaths`; dirty-state save button; CF API key grouped under Advanced → API Keys
- `src/routes/InstanceDetail.tsx` (1076 lines) — Launch/Stop toggle, running badge, live log console (capped at 500 lines from store `runLogs`); run state read from store `runs` slice (`runState = useAppStore(s => s.runs.get(slug))`); logs read from store `runLogs` slice (no local `useState` for run/log — store is the source of truth); stat grid; mods summary + "Manage installs" button opens `SlideOver`; `ManageInstallsPanel` with Installed + Add tabs; `addMod`/`updateMod` return task ids (results arrive asynchronously via `task://update`)
- `src/lib/bindings.ts` (generated, committed) — tauri-specta output: 35 typed `commands.*`, 8 typed `events.*`, all DTOs. Single source of truth. Never hand-edit; regenerate via `scripts/build.sh dev` on Windows.
- `src/lib/store.ts` — Zustand store (`useAppStore`): two slices: **tasks** (`Map<number, Task>`, `upsertTask`, `patchTaskProgress`); **runs** (`Map<string, RunState>`, `Map<string, RunLogLine[]>`, `upsertRun`, `appendLog`, `setLogs`); re-exports `Task`/`TaskStatus`/`TaskKind`/`ChildItem`/`TaskResult` from `bindings.ts` (no hand-mirrored task types); `RunState = RunUpdatePayload & { elapsedMs?: number | null }` (composed from generated types); `RunLogLine = RunLogPayload`
- `src/lib/ipc.ts` — thin adapter over `bindings.ts`: re-exports `events` + all DTO types; command wrappers via `unwrap()` helper (restores reject-on-error); `listTasks()`, `cancelTask()`, `listRunning()`, `getRunState()`, `getRunLogs()`, `addMod`/`updateMod`/`importMrpack`/`importCurseforgeZip`/`installModpack`/`updateModpack` (all return `Promise<number>` task id); zero hand-declared IPC type interfaces or event constants
- `src/lib/query.ts` — exports `queryClient` (staleTime=30s, gcTime=24h, retry=1, no refetch-on-focus) and `META_STALE_TIME` (6h)
- `src/lib/prefetch.ts` — `prefetchStartupData`: prefetches `["instances"]`, `["mc-versions"]`, `["loaders", latest]`
- `src/lib/utils.ts` — `cn` (clsx + tailwind-merge)
- `src/styles.css` — Tailwind v4 base + CSS custom properties for theme tokens

## Docs

- `docs/ARCHITECTURE.md` §8 — frontend structure overview
- `docs/ROADMAP.md` Phase 0 — scaffold/shell scope
- `docs/design/storage-auth-reorg.md` — documents Accounts page removal, auth migration to sidebar
- `docs/design/ui-modpack-rework.md` — rationale for Browse-as-modpack-feed, slide-over approach, modal import tabs
- `docs/spec/ui-modpack-rework.md` — implementation contract for the rework (CP1–CP5)
- `docs/spec/download-runner-rework/cp-7-frontend-store-subscription.md` — CP-7 spec: Zustand store slices, `AppShell` subscription wiring, hydration
- `docs/spec/download-runner-rework/cp-8-download-manager-panel.md` — CP-8 spec: `DownloadManagerButton` + `DownloadManagerPanel` + `TaskRow` + `StatusBadge`
- `docs/spec/download-runner-rework/cp-9-running-indicator-resync-toast.md` — CP-9 spec: `RunningIndicator` in sidebar, `Toasts` component, `extractSlug`/`labelFor`

## Coupling

- `bindings.ts` is generated from Rust; a Rust DTO/command/event change requires regenerating `bindings.ts` via `scripts/build.sh dev` on Windows — not a manual `ipc.ts` edit. `ipc.ts` imports from `bindings.ts` and holds zero hand-declared IPC interfaces.
- `store.ts` imports task/run types from `bindings.ts`; `RunState` and `RunLogLine` are composed types (not hand-mirrored). A Rust type change propagates through `bindings.ts` regeneration; store reducers are unaffected.
- `AppShell` subscribes to `task://update` and `task://progress` via `events.taskProgress.listen` / `events.taskUpdate.listen`; `run://update` via `events.runUpdate.listen`; `launch://log` / `launch://exit` / `install://log` via their generated equivalents. All event channel names are defined in the Rust `#[tauri_specta(event_name = "...")]` attrs and frozen in `bindings.ts`.
- `Sidebar` subscribes to `auth://device-code` via `events.authDeviceCode.listen`.
- `Toasts` watches `tasks` for Done results; `DownloadManagerButton` watches `tasks` for active count; `RunningIndicator` watches `runs` for preparing/running count. All three read from the same Zustand store populated by `AppShell`.
- `MrpackImportResult`, `CfManualFile`, `CfImportResult`, `ModpackInstallResult`, `PackUpdateResult` come from `bindings.ts` (generated from `core/modpack.rs`/`lib.rs`); import/install wrappers return `Promise<number>` (task id) — results arrive asynchronously via store events.
- `ManualMod`, `UnresolvedDep`, `Suggestion`, `IncompatibleWarning`, `FailedMod`, `AddModResult`, `UpdateModResult` come from `bindings.ts` (generated from `core/mod_install.rs`); `addMod`/`updateMod` return `Promise<number>`.
- `AccountMeta` is normalized to `AccountMeta_Serialize` from `bindings.ts`; `DeviceCodePayload` re-exported from `bindings.ts` (auth domain coupling).
- `NewInstanceModal` uses `open` from `@tauri-apps/plugin-dialog` (must remain in `Cargo.toml`, `package.json`, `capabilities/default.json`).
- `InstanceDetail.tsx` uses `openUrl` from `@tauri-apps/plugin-opener` for manual download URLs.
- Zustand is now in active use (previously listed as installed but unused).

## Conventions worth knowing

- Path alias `@/` maps to `src/` (configured in `vite.config.ts` and `tsconfig.json`).
- Tailwind v4; design tokens are CSS custom properties in `styles.css`.
- `cn` from `src/lib/utils.ts` (clsx + tailwind-merge) is the standard class composition helper.
- TanStack Query mutation success handlers call `invalidateQueries` or `setQueryData`; `InstanceDetail` invalidates `["instance", slug]` after any mod mutation.
- Task/run event subscriptions live exclusively in `AppShell` (mount-once); routes read from the Zustand store, not from local event subscriptions. This is the authoritative pattern.
- `install://log` carries no `instanceId` — the installer runs at most once at a time; `AppShell` attributes all installer lines to whichever run has status `"preparing"` at the time.
- `ProviderKind` response values use camelCase (`"curseForge"`); routing param strings passed to `searchMods`/`getModVersions` use lowercase (`"curseforge"`) — distinct, must not be conflated.
- `ModRow` in the Installed tab shows controls only when a matching `ModEntry` exists in `instance.mods`; unmanaged folder mods show an "unmanaged" badge.
- Log console in `InstanceDetail` shown only when `running || logLines.length > 0`; auto-scrolls to bottom.
- Window opens `maximized: true`; `tauri.conf.json` sets `minWidth: 800`, `minHeight: 600`, restored size 1280×800.
