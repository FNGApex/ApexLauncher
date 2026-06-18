# frontend-shell

## What it does

React 19 app entry point, routing, sidebar navigation (with inline login/logout control and a Download Manager toggle), IPC wrapper layer, Zustand store (tasks + runs slices), TanStack Query client setup, settings UI, modpack Browse page (discovery-only, unified feed), and instance detail page with a "Manage installs" slide-over for mod management. All Tauri `invoke` calls are centralized in `src/lib/ipc.ts` with hand-typed TS interfaces mirroring Rust structs. `AppShell` mounts once, subscribes to all `task://progress`, `task://update`, `run://update`, `launch://log`, `launch://exit`, `install://log` events, and hydrates the store from `listTasks()` + `listRunning()` on startup. There is no standalone Accounts route — auth UI lives in the sidebar.

## Artifacts

- `src/main.tsx` — app entry: mounts `QueryClientProvider` + `RouterProvider`; calls `prefetchStartupData` fire-and-forget at startup
- `src/router.tsx` — React Router v7 `createBrowserRouter`; routes: `/instances` (Home), `/instances/:slug` (InstanceDetail), `/browse` (Browse), `/settings` (Settings); index redirects to `/instances`; no `/accounts` route
- `src/components/AppShell.tsx` — root layout: Sidebar + `<Outlet />` + `<Toasts />`; subscribes to `task://update` (`listenTaskUpdate` → `upsertTask`), `task://progress` (`listenTaskProgress` → `patchTaskProgress`), `run://update` (`listenRunUpdate` → `upsertRun`), `launch://log` / `launch://exit` / `install://log` (→ `appendLog`); hydrates store on mount via `listTasks()` + `listRunning()` + `getRunLogs(slug)` per active run; all subscriptions are torn down on unmount (never in production, but teardown is correct)
- `src/components/Sidebar.tsx` — fixed left nav; links to Instances / Browse / Settings; `<DownloadManagerButton />` for the Download Manager panel; `<RunningIndicator />` (reads `runs` slice from store, shows count of preparing+running instances); inline login/logout control; version badge `v0.1.0 · pre-alpha` at bottom
- `src/components/DownloadManager.tsx` (254 lines) — `DownloadManagerButton` (toggle + active-count badge; reads `tasks` slice from store) + `DownloadManagerPanel` (fixed left-anchored panel, task list sorted newest-first); `TaskRow` shows parent label, current child, `done/total` progress bar, cancel button (calls `cancelTask`); `StatusBadge` maps all seven `TaskStatus` variants to colored badges; `isActive` helper covers queued/planning/downloading/applying
- `src/components/Toasts.tsx` (117 lines) — `Toasts` component (mounted in `AppShell`, fixed bottom-right); watches `tasks` slice for newly-Done tasks with a `result` field; shows one toast per completed task (label from result + "Open" navigate button if result has a `slug`); `shownRef` prevents duplicate toasts; `extractSlug` / `labelFor` helpers parse `TaskResult` union
- `src/components/NewInstanceModal.tsx` — create/import dialog with two tabs: **Create** (MC version + loader build selectors, calls `createInstance`) and **Import pack** (single file picker via `@tauri-apps/plugin-dialog`'s `open()`, routes by extension to `importMrpack` or `importCurseforgeZip` — both return `Promise<number>` task id; toast shown via `Toasts` component)
- `src/routes/Home.tsx` — instance grid; delete via confirmation; opens `NewInstanceModal`; exports `ImportResultToast` / `CfImportResultToast` named (reused in Browse); TanStack Query key `["instances"]`
- `src/routes/Browse.tsx` — modpack discovery feed: debounced search, MC version + loader facets; single merged feed from both providers; each `ModpackCard` Install button calls `installModpack` → returns task id; result shown via `Toasts` + imported toast components from `Home.tsx`
- `src/routes/Settings.tsx` — loads/saves `Settings` (defaultMemoryMb, defaultJavaArgs, curseforgeApiKey); displays read-only `AppPaths`; dirty-state save button; CF API key grouped under Advanced → API Keys
- `src/routes/InstanceDetail.tsx` (1076 lines) — Launch/Stop toggle, running badge, live log console (capped at 500 lines from store `runLogs`); run state read from store `runs` slice (`runState = useAppStore(s => s.runs.get(slug))`); logs read from store `runLogs` slice (no local `useState` for run/log — store is the source of truth); stat grid; mods summary + "Manage installs" button opens `SlideOver`; `ManageInstallsPanel` with Installed + Add tabs; `addMod`/`updateMod` return task ids (results arrive asynchronously via `task://update`)
- `src/lib/store.ts` (160 lines) — Zustand store (`useAppStore`): two slices: **tasks** (`Map<number, Task>`, `upsertTask`, `patchTaskProgress`); **runs** (`Map<string, RunState>`, `Map<string, RunLogLine[]>`, `upsertRun`, `appendLog`, `setLogs`); `Task`/`TaskStatus`/`TaskKind`/`ChildItem`/`TaskProgressUpdate`/`TaskResult` TS types mirroring `task_manager.rs`; `RunState`/`RunLogLine` types mirroring `launch.rs`
- `src/lib/ipc.ts` (1039 lines) — all typed `invoke` wrappers and event helpers; `listenTaskUpdate` / `listenTaskProgress` / `listenRunUpdate` event subscriptions; `listTasks()` → `Promise<unknown[]>`; `cancelTask(id)` → `Promise<void>`; `listRunning()` → `Promise<RunInfoPayload[]>`; `getRunState(slug)` → `Promise<RunInfoPayload | null>`; `getRunLogs(slug)` → `Promise<RunLogPayload[] | null>`; `TaskProgressPayload`, `RunUpdatePayload`, `RunInfoPayload`, `RunLogPayload` interfaces; `TASK_PROGRESS_EVENT = "task://progress"`, `TASK_UPDATE_EVENT = "task://update"`, `RUN_UPDATE_EVENT = "run://update"`; `addMod`/`updateMod`/`importMrpack`/`importCurseforgeZip`/`installModpack`/`updateModpack` all return `Promise<number>` (task id)
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

- `ipc.ts` hand-mirrors Rust struct field names (camelCase via `serde rename_all`); no generated types yet. Any Rust struct rename or new field requires a manual `ipc.ts` update.
- `store.ts` mirrors `task_manager.rs` types (`Task`, `TaskStatus` with `#[serde(tag = "kind")]`, `TaskKind`, `ChildItem`) and `launch.rs` types (`RunStatus` serialized as lowercase strings, `RunInfo` → `RunState`, `LogLine` → `RunLogLine`). Any change to those Rust types requires a matching `store.ts` update.
- `AppShell` subscribes to `task://update` and `task://progress` from `TauriTaskObserver` in `lib.rs`; and `run://update` from `TauriLaunchSink`; and `launch://log` / `launch://exit` / `install://log` from `TauriLaunchSink`/`TauriInstallSink`. All five event names are literals in both `lib.rs` and `ipc.ts` — a rename on either side breaks the subscription.
- `Toasts` watches `tasks` for Done results; `DownloadManagerButton` watches `tasks` for active count; `RunningIndicator` watches `runs` for preparing/running count. All three read from the same Zustand store populated by `AppShell`.
- `MrpackImportResult`, `CfManualFile`, `CfImportResult`, `ModpackInstallResult`, `PackUpdateResult` in `ipc.ts` mirror `core/modpack.rs`/`lib.rs`; import/install wrappers return `Promise<number>` (task id) — results arrive asynchronously via store events.
- `ManualMod`, `UnresolvedDep`, `Suggestion`, `IncompatibleWarning`, `FailedMod`, `AddModResult`, `UpdateModResult` mirror `core/mod_install.rs`; `addMod`/`updateMod` return `Promise<number>`.
- `AccountMeta`, `DeviceCodePayload`, `AuthCommandError` in `ipc.ts` mirror `core/auth.rs` (auth domain coupling).
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
