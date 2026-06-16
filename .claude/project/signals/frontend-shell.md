# frontend-shell

## What it does

React 19 app entry point, routing, sidebar navigation (with inline login/logout control), IPC wrapper layer, TanStack Query client setup, settings UI, live Browse page (Phase 5 slice A), and instance detail page with mod management (Phase 5 slice B). All Tauri `invoke` calls are centralized in `src/lib/ipc.ts` with hand-typed TS interfaces mirroring Rust structs. There is no standalone Accounts route — auth UI lives in the sidebar.

## Artifacts

- `src/main.tsx` — app entry: mounts `QueryClientProvider` + `RouterProvider`; calls `prefetchStartupData` fire-and-forget at startup
- `src/router.tsx` — React Router v7 `createBrowserRouter`; routes: `/instances` (Home), `/instances/:slug` (InstanceDetail), `/browse` (Browse), `/settings` (Settings); index redirects to `/instances`; no `/accounts` route (removed in storage-auth-reorg)
- `src/components/AppShell.tsx` — root layout: Sidebar + `<Outlet />`
- `src/components/Sidebar.tsx` — fixed left nav; links to Instances / Browse / Settings; inline login/logout control: queries `getAccount`, shows `LoggedOutControl` (device-code flow + code display + cancel) or `LoggedInControl` (username initial + Logout button); subscribes to `auth://device-code` event via `listenDeviceCode` before invoking `beginLogin`; version badge `v0.1.0 · pre-alpha` at bottom
- `src/routes/Browse.tsx` — live Browse page (Phase 5 slice A): debounced search (400 ms), MC version + loader facet selectors, All/Modrinth/CurseForge tabs; All tab is two independent side-by-side `ProviderColumn` components; each column uses `useInfiniteQuery` + `IntersectionObserver` sentinel; CF key-absent surfaces as `KeyMissingState` CTA
- `src/routes/Settings.tsx` — live: loads/saves `Settings` (defaultMemoryMb, defaultJavaArgs, curseforgeApiKey); displays read-only `AppPaths`; dirty-state save button
- `src/routes/InstanceDetail.tsx` — Launch/Stop toggle, running badge, live log console (500-line ring buffer); subscribes to `launch://log` (slug-filtered), `launch://exit` (slug-filtered), and `install://log` (not slug-filtered, prefixed `[install:<stream>]`) in a single `useEffect` on slug change; stat grid shows Memory, Java, Managed mods, Created, Last played, Total playtime; full mod management section: `ModRow` per `FolderMod` with enable/disable toggle (`setModEnabled`), update (`updateMod` → `UpdateResultBadge`), and remove (`removeMod`) mutations; `UpdateResultBadge` opens manual download URL via `openUrl` from `@tauri-apps/plugin-opener`
- `src/lib/ipc.ts` — all typed `invoke` wrappers and event helpers; exports interfaces: `AppInfo`, `Loader`, `JavaCfg`, `InstanceSource`, `ModEntry`, `Instance`, `FolderMod`, `InstanceDetail`, `CreateInstanceReq`, `Settings`, `AppPaths`, `McVersion`, `LoaderOption`, `DownloadItem`, `DownloadPlan`, `ItemStatus`, `ItemOutcome`, `PlanResult`, `DownloadProgressPayload`, `LaunchMeta`, `ResolveResult`, `JavaInstallation`, `LaunchLogPayload`, `LaunchExitPayload`, `InstallLogPayload`, `AccountMeta`, `DeviceCodePayload`, `AuthCommandError`, `ProviderKind`, `ProjectSummary`, `VersionFile`, `Dependency`, `ProjectVersion`, `SearchResult`, `ProviderCommandError`, `ManualMod`, `UnresolvedDep`, `Suggestion`, `IncompatibleWarning`, `FailedMod`, `AddModResult`, `UpdateModResult`, `MrpackImportResult`, `CfManualFile`, `CfImportResult`; event constants `AUTH_DEVICE_CODE_EVENT`, `LAUNCH_LOG_EVENT`, `LAUNCH_EXIT_EVENT`, `INSTALL_LOG_EVENT`; listen helpers: `listenDeviceCode`, `listenInstallLog`; mod management wrappers: `addMod`, `setModEnabled`, `removeMod`, `updateMod`; modpack import wrappers: `importMrpack`, `importCurseforgeZip` (modpack domain); type alias `LoaderKind = "vanilla" | "fabric" | "quilt" | "forge" | "neoforge"`; `JavaSource = "detected" | "downloaded"`
- `src/lib/query.ts` — exports `queryClient` (staleTime=30s, gcTime=24h, retry=1, no refetch-on-focus) and `META_STALE_TIME` (6h)
- `src/lib/prefetch.ts` — `prefetchStartupData`: prefetches `["instances"]`, `["mc-versions"]`, `["loaders", latest]`
- `src/lib/utils.ts` — `cn` (clsx + tailwind-merge)
- `src/styles.css` — Tailwind v4 base + CSS custom properties for theme tokens

## Docs

- `docs/ARCHITECTURE.md` §8 — frontend structure overview
- `docs/ROADMAP.md` Phase 0 — scaffold/shell scope
- `docs/design/storage-auth-reorg.md` — documents Accounts page removal, auth migration to sidebar

## Coupling

- `ipc.ts` hand-mirrors Rust struct field names (camelCase via `serde rename_all`); no generated types yet (specta/ts-rs planned per `docs/ROADMAP.md`). Any Rust struct rename or new field requires a manual `ipc.ts` update.
- `MrpackImportResult`, `CfManualFile`, `CfImportResult` in `ipc.ts` mirror `core/modpack.rs`/`lib.rs`; `importMrpack`/`importCurseforgeZip` invoke the `import_mrpack`/`import_curseforge_zip` commands — changes to those Rust types propagate here (modpack domain coupling).
- `Home.tsx` imports `open` from `@tauri-apps/plugin-dialog` for the `.mrpack`/`.zip` file pickers (modpack domain's import buttons); this Tauri plugin must remain in `Cargo.toml`, `package.json`, and `capabilities/default.json` (`"dialog:default"`) — same pattern as `@tauri-apps/plugin-opener` for `InstanceDetail`'s manual-download links.
- `InstallLogPayload` in `ipc.ts` mirrors `lib.rs::InstallLogPayload` (`{ stream, line }`, camelCase); added in Phase 4 slice B.
- `ManualMod`, `UnresolvedDep`, `Suggestion`, `IncompatibleWarning`, `FailedMod`, `AddModResult`, `UpdateModResult` mirror `core/mod_install.rs`; `addMod`/`setModEnabled`/`removeMod`/`updateMod` invoke `add_mod`/`set_mod_enabled`/`remove_mod`/`update_mod` commands — changes to those Rust types propagate here (providers + instances domain coupling).
- `Settings.tsx` is tightly coupled to `ipc.ts` `Settings` interface; if `curseforge_api_key` moves to a separate secret store, both files change.
- `prefetch.ts` imports `META_STALE_TIME` from `query.ts` and IPC fns from `ipc.ts`; changes to query key shapes affect both.
- `Browse.tsx` calls `searchMods` and relies on `ProviderCommandError.kind === "key_missing"` for the CF key-absent state; changes to those in `core/providers.rs`/`lib.rs` must propagate here (providers domain coupling).
- `AccountMeta`, `DeviceCodePayload`, `AuthCommandError` in `ipc.ts` mirror `core/auth.rs` and `lib.rs`; auth IPC wrappers are `beginLogin`/`cancelLogin`/`getAccount`/`logout` — any rename in auth structs requires a manual `ipc.ts` update (auth domain coupling).
- `InstanceDetail.tsx` uses `openUrl` from `@tauri-apps/plugin-opener` for manual download URLs; this Tauri plugin must remain in `Cargo.toml` and `tauri.conf.json` permissions.

## Conventions worth knowing

- Path alias `@/` maps to `src/` (configured in `vite.config.ts` and `tsconfig.json`).
- Tailwind v4 is used; design tokens are CSS custom properties in `styles.css`, referenced as `text-muted`, `bg-surface`, `bg-primary`, `bg-accent`, `text-danger`, etc.
- `cn` from `src/lib/utils.ts` (clsx + tailwind-merge) is the standard class composition helper.
- TanStack Query mutation success handlers call `invalidateQueries` or `setQueryData` directly; `InstanceDetail` invalidates `["instance", slug]` after any mod mutation.
- No Zustand stores are in use (listed in `package.json` but unused).
- `install://log` carries no `instanceId` — the installer runs at most once at a time, so `InstanceDetail` attributes all installer lines to the instance currently being launched.
- `ProviderKind` response values use camelCase (`"curseForge"`); the routing param strings passed to `searchMods`/`getModVersions` use lowercase (`"curseforge"`) — these are distinct and must not be conflated.
- `ModRow` only shows enable/disable/update/remove controls when a matching `ModEntry` exists in `instance.mods` (i.e., managed mods only); unmanaged folder mods show an "unmanaged" badge with no mutation buttons.
- Log console in `InstanceDetail` is shown only when `running || logLines.length > 0`; it auto-scrolls to bottom on each new line via a `scrollTop = scrollHeight` effect.
