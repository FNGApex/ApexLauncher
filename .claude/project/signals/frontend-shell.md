# frontend-shell

## What it does

React 19 app entry point, routing, sidebar navigation (with inline login/logout control), IPC wrapper layer, TanStack Query client setup, settings UI, modpack Browse page (discovery-only, unified feed), and instance detail page with a "Manage installs" slide-over for mod management. All Tauri `invoke` calls are centralized in `src/lib/ipc.ts` with hand-typed TS interfaces mirroring Rust structs. There is no standalone Accounts route — auth UI lives in the sidebar.

## Artifacts

- `src/main.tsx` — app entry: mounts `QueryClientProvider` + `RouterProvider`; calls `prefetchStartupData` fire-and-forget at startup
- `src/router.tsx` — React Router v7 `createBrowserRouter`; routes: `/instances` (Home), `/instances/:slug` (InstanceDetail), `/browse` (Browse), `/settings` (Settings); index redirects to `/instances`; no `/accounts` route (removed in storage-auth-reorg)
- `src/components/AppShell.tsx` — root layout: Sidebar + `<Outlet />`
- `src/components/Sidebar.tsx` — fixed left nav; links to Instances / Browse / Settings; inline login/logout control: queries `getAccount`, shows `LoggedOutControl` (device-code flow + code display + cancel) or `LoggedInControl` (username initial + Logout button); subscribes to `auth://device-code` event via `listenDeviceCode` before invoking `beginLogin`; version badge `v0.1.0 · pre-alpha` at bottom
- `src/components/ProviderBadge.tsx` — inline platform badge component; `ProviderKind` → colored label (`"Modrinth"` green, `"CurseForge"` orange); used in Browse cards and the slide-over Add tab in InstanceDetail
- `src/components/SlideOver.tsx` — reusable right-side slide-over panel with backdrop overlay, Escape-key close, configurable `title` and `widthClass` (default `"w-full max-w-xl"`); used by the "Manage installs" panel in InstanceDetail
- `src/components/NewInstanceModal.tsx` — create/import dialog with two tabs: **Create** (MC version + loader build selectors, calls `createInstance`) and **Import pack** (single file picker via `@tauri-apps/plugin-dialog`'s `open()`, routes by extension to `importMrpack` or `importCurseforgeZip`; `onMrpackImport`/`onCfImport` callbacks notify Home to surface result toasts); import fires `navigate` to the new instance on success
- `src/routes/Home.tsx` — instance grid; delete via confirmation; opens `NewInstanceModal`; renders `ImportResultToast` / `CfImportResultToast` (received as modal callbacks, no import buttons directly on page); TanStack Query key `["instances"]`
- `src/routes/Browse.tsx` — modpack discovery feed (UI/modpack rework): debounced search (400 ms), MC version + loader facet selectors; **single merged feed** from both providers (two independent `useInfiniteQuery` calls, client-side merge by `provider:id` key, sort downloads desc); each card (`ModpackCard`) is platform-badged via `ProviderBadge` and opens `pack.pageUrl` via `openUrl` on click (discovery-only, no add-to-instance modal); CF key-missing shows an inline dismissible notice without hiding Modrinth results; `isProviderCommandError` type guard shared with InstanceDetail; no side-by-side `ProviderColumn` layout, no `AddToInstanceModal`
- `src/routes/Settings.tsx` — live: loads/saves `Settings` (defaultMemoryMb, defaultJavaArgs, curseforgeApiKey); displays read-only `AppPaths`; dirty-state save button
- `src/routes/InstanceDetail.tsx` (1059 lines) — Launch/Stop toggle, running badge, live log console (500-line ring buffer); subscribes to `launch://log`, `launch://exit`, `install://log`; stat grid (Memory, Java, Managed mods, Created, Last played, Total playtime); mods summary line + "Manage installs" button opens `SlideOver`; `ManageInstallsPanel` inside the slide-over has two tabs: **Installed** (full `ModRow` list with enable/disable/update/remove, relocated from previous inline layout) and **Add mod** (`AddModTab` with source toggle, `searchMods(..., "mod")`, `ModSearchCard` per result with Install button, `AddResultSummary`)
- `src/lib/ipc.ts` — all typed `invoke` wrappers and event helpers; `ProjectType = "mod" | "modpack"` type alias; `ProjectSummary` now includes `pageUrl: string | null`; `searchMods` accepts `projectType: ProjectType = "mod"` as trailing param; exports all domain interfaces (see providers and mod-install domain signals for full lists); event constants `AUTH_DEVICE_CODE_EVENT`, `LAUNCH_LOG_EVENT`, `LAUNCH_EXIT_EVENT`, `INSTALL_LOG_EVENT`
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

## Coupling

- `ipc.ts` hand-mirrors Rust struct field names (camelCase via `serde rename_all`); no generated types yet (specta/ts-rs planned per `docs/ROADMAP.md`). Any Rust struct rename or new field requires a manual `ipc.ts` update.
- `MrpackImportResult`, `CfManualFile`, `CfImportResult` in `ipc.ts` mirror `core/modpack.rs`/`lib.rs`; `importMrpack`/`importCurseforgeZip` invoke the `import_mrpack`/`import_curseforge_zip` commands — changes to those Rust types propagate here (modpack domain coupling). Import flow now lives in `NewInstanceModal`'s Import tab, not in `Home.tsx` directly.
- `NewInstanceModal` uses `open` from `@tauri-apps/plugin-dialog` for the file picker (Import tab); this Tauri plugin must remain in `Cargo.toml`, `package.json`, and `capabilities/default.json` (`"dialog:default"`).
- `ManualMod`, `UnresolvedDep`, `Suggestion`, `IncompatibleWarning`, `FailedMod`, `AddModResult`, `UpdateModResult` mirror `core/mod_install.rs`; `addMod`/`setModEnabled`/`removeMod`/`updateMod` invoke the matching Rust commands — changes propagate here (providers + instances domain coupling).
- `Settings.tsx` is tightly coupled to `ipc.ts` `Settings` interface; if `curseforge_api_key` moves to a separate secret store, both files change.
- `Browse.tsx` calls `searchMods(..., "modpack")` and checks `ProviderCommandError.kind === "key_missing"` for the inline CF notice; changes to those in `core/providers.rs`/`lib.rs` must propagate here (providers domain coupling).
- `AccountMeta`, `DeviceCodePayload`, `AuthCommandError` in `ipc.ts` mirror `core/auth.rs` and `lib.rs`; auth IPC wrappers are `beginLogin`/`cancelLogin`/`getAccount`/`logout` — any rename in auth structs requires a manual `ipc.ts` update (auth domain coupling).
- `InstanceDetail.tsx` uses `openUrl` from `@tauri-apps/plugin-opener` for manual download URLs; this Tauri plugin must remain in `Cargo.toml` and `tauri.conf.json` permissions.

## Conventions worth knowing

- Path alias `@/` maps to `src/` (configured in `vite.config.ts` and `tsconfig.json`).
- Tailwind v4 is used; design tokens are CSS custom properties in `styles.css`, referenced as `text-muted`, `bg-surface`, `bg-primary`, `bg-accent`, `text-danger`, etc.
- `cn` from `src/lib/utils.ts` (clsx + tailwind-merge) is the standard class composition helper.
- TanStack Query mutation success handlers call `invalidateQueries` or `setQueryData` directly; `InstanceDetail` invalidates `["instance", slug]` after any mod mutation.
- No Zustand stores are in use (listed in `package.json` but unused).
- `install://log` carries no `instanceId` — the installer runs at most once at a time, so `InstanceDetail` attributes all installer lines to the instance currently being launched.
- `ProviderKind` response values use camelCase (`"curseForge"`); the routing param strings passed to `searchMods`/`getModVersions` use lowercase (`"curseforge"`) — these are distinct and must not be conflated. `ProviderBadge` uses the camelCase value to key its label/color maps.
- `ModRow` in the Installed tab shows enable/disable/update/remove controls only when a matching `ModEntry` exists in `instance.mods` (managed mods only); unmanaged folder mods show an "unmanaged" badge.
- Log console in `InstanceDetail` is shown only when `running || logLines.length > 0`; it auto-scrolls to bottom on each new line via a `scrollTop = scrollHeight` effect.
- Window opens `maximized: true`; `tauri.conf.json` sets `minWidth: 800`, `minHeight: 600`, restored size 1280×800.
