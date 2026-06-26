import { commands } from "@/lib/bindings";
import type { LoaderOption as GenLoaderOption, Task } from "@/lib/bindings";

// Generated typed event surface (`events.<channel>.listen / once / emit`).
// Re-exported so the event layer routes through `bindings.ts` like the commands.
export { events } from "@/lib/bindings";

/**
 * Thin adapter over the generated `bindings.ts`. Command wrappers route through
 * the generated `commands.*` surface and preserve the throw-on-error +
 * positional / `?? null` ergonomics that call sites depend on. Generated DTOs are
 * re-exported below under the names components import.
 *
 * Generated fallible commands return a `{ status: "ok" | "error" }` Result rather
 * than rejecting; `unwrap` collapses that back to the historical "resolve data /
 * reject error" contract. The 5 infallible commands (`appInfo`, `listRunning`,
 * `getRunState`, `getRunLogs`, `cancelLogin`) already return a bare `Promise<T>`
 * and are called directly.
 *
 * Events route through the generated `events.*` surface (re-exported above);
 * subscribe with `events.<channel>.listen`. No hand-declared IPC types remain.
 */

// --- Result unwrap (restores reject-on-error from the generated Result shape) ---

type CmdResult<T> = { status: "ok"; data: T } | { status: "error"; error: unknown };

async function unwrap<T>(p: Promise<CmdResult<T>>): Promise<T> {
  const r = await p;
  if (r.status === "error") throw r.error;
  return r.data;
}

// --- Re-exported generated DTOs (replace the former hand-declared interfaces) ---

export type {
  AppInfo,
  Loader,
  JavaCfg,
  ModEntry,
  Instance,
  FolderMod,
  InstanceDetail,
  CreateInstanceReq,
  Settings,
  AppPaths,
  McVersion,
  DownloadItem,
  DownloadPlan,
  ExpectedHash,
  ItemStatus,
  ItemOutcome,
  PlanResult,
  LaunchMeta,
  ResolveResult,
  JavaInstallation,
  JavaSource,
  AuthCommandError,
  ProviderKind,
  ProjectType,
  ProjectSummary,
  VersionFile,
  Dependency,
  ProjectVersion,
  SearchResult,
  ProviderCommandError,
  ManualMod,
  UnresolvedDep,
  Suggestion,
  IncompatibleWarning,
  FailedMod,
  AddModResult,
  UpdateModResult,
  MrpackImportResult,
  CfManualFile,
  PendingManual,
  CfImportResult,
  ModpackInstallResult,
  PackUpdateResult,
  PackInfo,
  RunInfoPayload,
  RunLogPayload,
  JavaProbe,
  // Event payload type still referenced by name in a component (`Sidebar`);
  // channels themselves are subscribed via the generated `events.*`.
  DeviceCodePayload,
} from "@/lib/bindings";

import type { AccountMeta_Serialize, RunInfoPayload, RunLogPayload } from "@/lib/bindings";

/**
 * Persisted account metadata exposed to the frontend. The generated `AccountMeta`
 * union carries a malformed serde-alias arm (`{ mc_token_expires }`) that only
 * exists for loading legacy `account.json` on the Rust side and never reaches the
 * webview — the wire shape is always the serialize form, so we surface that.
 */
export type AccountMeta = AccountMeta_Serialize;

/**
 * Loader discriminant. The Rust enum is erased to a bare `string` in the
 * generated bindings (specta drops the variant set on `Loader.kind`), so the
 * narrow union stays frontend-owned. `LoaderOption` is re-narrowed over the
 * generated shape so loader pickers keep their exhaustive typing.
 */
export type LoaderKind = "vanilla" | "fabric" | "quilt" | "forge" | "neoforge";

/** A loader available for a given MC version, with its builds (newest first). */
export type LoaderOption = Omit<GenLoaderOption, "kind"> & { kind: LoaderKind };

// --- Phase 0: app metadata ---

export function getAppInfo() {
  return commands.appInfo();
}

// --- Phase 1: instances ---

export function listInstances() {
  return unwrap(commands.listInstances());
}

export function createInstance(req: Parameters<typeof commands.createInstance>[0]) {
  return unwrap(commands.createInstance(req));
}

export function getInstance(slug: string) {
  return unwrap(commands.getInstance(slug));
}

export async function deleteInstance(slug: string): Promise<void> {
  await unwrap(commands.deleteInstance(slug));
}

// --- Phase 1: global settings ---

export function getSettings() {
  return unwrap(commands.getSettings());
}

export function saveSettings(settings: Parameters<typeof commands.saveSettings>[0]) {
  return unwrap(commands.saveSettings(settings));
}

export function getAppPaths() {
  return unwrap(commands.appPaths());
}

// --- Version & loader metadata ---

/** Stable Minecraft releases, newest first, down to 1.7.10. */
export function listMinecraftVersions() {
  return unwrap(commands.listMinecraftVersions());
}

/** Loaders (incl. vanilla) available for `minecraft`, each with its build list. */
export async function getLoaders(minecraft: string): Promise<LoaderOption[]> {
  // The generated `kind` is `string`; the values are valid `LoaderKind` at runtime.
  return (await unwrap(commands.getLoaders(minecraft))) as LoaderOption[];
}

// --- Phase 2: download engine ---

/**
 * Execute a DownloadPlan concurrently. Progress rides the `download://progress`
 * event channel. `concurrency` defaults to 8 (null) when omitted.
 */
export function executeDownloadPlan(
  plan: Parameters<typeof commands.executeDownloadPlan>[0],
  concurrency?: number | null,
) {
  return unwrap(commands.executeDownloadPlan(plan, concurrency ?? null));
}

// --- Phase 2: vanilla resolver ---

/** Resolve a vanilla Minecraft version into a DownloadPlan + LaunchMeta. */
export function resolveVanilla(versionId: string) {
  return unwrap(commands.resolveVanilla(versionId));
}

// --- Phase 2, slice C: Java manager ---

/** Detect or provision a JRE for the given Java major version. */
export function ensureJava(major: number) {
  return unwrap(commands.ensureJava(major));
}

// --- Phase 2, slice D: vanilla launch ---

/** Launch a vanilla Minecraft instance. Streams output as `launch://log` events. */
export async function launchInstance(slug: string): Promise<void> {
  await unwrap(commands.launchInstance(slug));
}

/** Stop (kill) a running Minecraft instance. */
export async function killInstance(slug: string): Promise<void> {
  await unwrap(commands.killInstance(slug));
}

// --- Phase 3: Microsoft authentication ---

/**
 * Begin the Microsoft device-code login flow. Long-lived: emits an
 * `auth://device-code` event (subscribe with `events.authDeviceCode.listen` first), then
 * polls until the user completes sign-in. Resolves with the persisted account.
 */
export function beginLogin(): Promise<AccountMeta> {
  return unwrap(commands.beginLogin());
}

/** Cancel an in-flight `beginLogin`. No-op if no login is in progress. */
export function cancelLogin() {
  return commands.cancelLogin();
}

/** Return the persisted account, or null when not logged in. */
export function getAccount(): Promise<AccountMeta | null> {
  return unwrap(commands.getAccount());
}

/** Log out: clears the persisted account and OS keyring entry. */
export async function logout(): Promise<void> {
  await unwrap(commands.logout());
}

// --- Phase 5: provider browse ---

/**
 * Search mods on the given provider. `provider` is a lowercase routing string
 * (`"modrinth"` / `"curseforge"`), distinct from the `ProviderKind` response
 * casing (`"curseForge"`). `projectType` selects content class.
 */
export function searchMods(
  provider: "modrinth" | "curseforge",
  query: string,
  mcVersion: string | null,
  loader: string | null,
  offset: number,
  limit: number,
  projectType: "mod" | "modpack" = "mod",
  // BR-A: multi-loader + category filters (kept at the end so existing 7-arg
  // callers don't break; reordered to the command's arg order internally).
  loaders: string[] = [],
  categories: string[] = [],
) {
  return unwrap(
    commands.searchMods(
      provider,
      query,
      mcVersion,
      loader,
      loaders,
      categories,
      offset,
      limit,
      projectType,
    ),
  );
}

/** Fetch all versions of a mod project compatible with the given MC version + loader. */
export function getModVersions(
  provider: "modrinth" | "curseforge",
  projectId: string,
  mcVersion: string | null,
  loader: string | null,
) {
  return unwrap(commands.getModVersions(provider, projectId, mcVersion, loader));
}

/** Fetch the title, icon, and long description for a pack project. */
export function getPackInfo(provider: "modrinth" | "curseforge", projectId: string) {
  return unwrap(commands.getPackInfo(provider, projectId));
}

// --- Phase 5 slice B: mod install ---

/**
 * Enqueue a mod-add task. Returns the task id synchronously; the terminal
 * `AddModResult` arrives on `task://update` when status is `"done"`.
 * `slug` is the target instance slug. `provider` is a lowercase routing string.
 */
export function addMod(
  provider: "modrinth" | "curseforge",
  projectId: string,
  versionId: string,
  slug: string,
  mcVersion: string,
  loader: string,
  // Display metadata captured at add-time from the search result, persisted on the
  // mod entry so the Installed list never re-fetches it (api-frugality rule).
  meta?: { name?: string | null; iconUrl?: string | null; summary?: string | null },
) {
  return unwrap(
    commands.addMod(provider, projectId, versionId, slug, mcVersion, loader, {
      name: meta?.name ?? null,
      iconUrl: meta?.iconUrl ?? null,
      summary: meta?.summary ?? null,
    }),
  );
}

/** Enable or disable an installed mod by toggling the `.disabled` suffix on its file. */
export async function setModEnabled(
  slug: string,
  fileName: string,
  enabled: boolean,
): Promise<void> {
  await unwrap(commands.setModEnabled(slug, fileName, enabled));
}

/** Remove an installed mod: deletes its file and drops its manifest entry. */
export async function removeMod(slug: string, fileName: string): Promise<void> {
  await unwrap(commands.removeMod(slug, fileName));
}

/**
 * Enqueue a mod-update task. Returns the task id synchronously; the terminal
 * `UpdateModResult` arrives on `task://update` when status is `"done"`.
 */
export function updateMod(slug: string, projectId: string) {
  return unwrap(commands.updateMod(slug, projectId));
}

/**
 * Backfill missing display metadata (name/icon/summary) for an instance's mods —
 * e.g. modpack-imported mods, which carry no metadata. One batched provider call
 * per provider; persisted to the manifest. Returns the count enriched. Idempotent:
 * returns 0 with NO network call when nothing is missing (api-frugality).
 */
export function enrichInstanceMods(slug: string): Promise<number> {
  return unwrap(commands.enrichInstanceMods(slug));
}

// --- Phase 6 slice A: mrpack import ---

/**
 * Import a local `.mrpack` file into a new instance. Enqueues a task and resolves
 * to the task id; the terminal `MrpackImportResult` rides `task://update`.
 */
export function importMrpack(mrpackPath: string, nameOverride?: string) {
  return unwrap(commands.importMrpack(mrpackPath, nameOverride ?? null));
}

// --- Phase 6 slice B: CurseForge .zip import ---

/**
 * Import a local CurseForge modpack `.zip` into a new instance. Enqueues a task
 * and resolves to the task id; the terminal `CfImportResult` rides `task://update`.
 */
export function importCurseforgeZip(cfZipPath: string, nameOverride?: string) {
  return unwrap(commands.importCurseforgeZip(cfZipPath, nameOverride ?? null));
}

// --- Phase 6 slice C: Browse → one-click install ---

/**
 * Install a modpack from a Browse `ProjectSummary` in one click. Enqueues a task
 * and resolves to the task id; the terminal `ModpackInstallResult` rides
 * `task://update`. `provider` is the wire value from `ProjectSummary.provider`.
 */
export function installModpack(
  provider: string,
  projectId: string,
  pageUrl?: string,
  versionId?: string,
) {
  return unwrap(
    commands.installModpack(provider, projectId, pageUrl ?? null, versionId ?? null),
  );
}

// --- Phase 6 slice D: pack update + Pack Lock ---

/**
 * Update an installed modpack to a newer (or user-chosen) version. Enqueues a
 * task and resolves to the task id; the terminal `PackUpdateResult` rides
 * `task://update`. Requires the instance to have a recorded `source`.
 */
export function updateModpack(slug: string, versionId?: string) {
  return unwrap(commands.updateModpack(slug, versionId ?? null));
}

/**
 * Toggle the Pack Lock on an instance. When locked, the backend rejects all
 * mod-mutation commands (add_mod, set_mod_enabled, remove_mod, update_mod).
 */
export async function setPackLock(slug: string, locked: boolean): Promise<void> {
  await unwrap(commands.setPackLock(slug, locked));
}

/**
 * Persist the per-instance "don't warn me again" choice for the pre-launch
 * missing-mods dialog (CF manual-download UX). Sync local op.
 */
export async function setPendingLaunchWarningSuppressed(
  slug: string,
  suppressed: boolean,
): Promise<void> {
  await unwrap(commands.setPendingLaunchWarningSuppressed(slug, suppressed));
}

/**
 * Rescan an instance's mods/ for dropped manual downloads. Returns the remaining
 * pending list. Sync local op (dir read + hash); off the task queue.
 */
export function rescanPendingManual(slug: string) {
  return unwrap(commands.rescanPendingManual(slug));
}

/**
 * Start a lazy watch on an instance's mods/ dir (call from the detail page when
 * it mounts with pending files). Idempotent.
 */
export async function startPendingWatch(slug: string): Promise<void> {
  await unwrap(commands.startPendingWatch(slug));
}

/** Stop the lazy mods/ watch for an instance (detail unmount / list emptied). */
export async function stopPendingWatch(slug: string): Promise<void> {
  await unwrap(commands.stopPendingWatch(slug));
}

/**
 * Refresh a managed instance's pack metadata (icon / author / latest version) and
 * check for an update. Throttled to once per 24h in the BACKEND (zero network when
 * within 24h — it returns the cached result). Returns `{ updateAvailable, latestVersion,
 * checked }`. `checked: true` means it actually polled (so stored fields changed → refetch
 * the instance). Safe to call on every managed-instance open.
 */
export function refreshPackMeta(slug: string) {
  return unwrap(commands.refreshPackMeta(slug));
}

// --- D-3: Per-instance Java settings ---

/**
 * Persist a JavaCfg override to an instance's manifest. When
 * `java.usePackSettings` is `true` the launcher uses these per-instance
 * settings at launch; when `false` it falls back to the global default.
 */
export async function setInstanceJava(
  slug: string,
  java: Parameters<typeof commands.setInstanceJava>[1],
): Promise<void> {
  await unwrap(commands.setInstanceJava(slug, java));
}

/**
 * Probe a Java binary by running `<path> -version` and parsing its stderr.
 * Resolves with a `JavaProbe` (`major` + `version`) on success, or rejects with
 * a human-readable message when the binary cannot be spawned or parsed.
 */
export function validateJavaPath(path: string) {
  return unwrap(commands.validateJavaPath(path));
}

// --- Run query commands ---

/** Enumerate non-terminal (preparing / running) instances. Used on AppShell mount. */
export function listRunning() {
  return commands.listRunning();
}

/** Read a single instance's run state (any status). Returns `null` when untracked. */
export function getRunState(slug: string): Promise<RunInfoPayload | null> {
  // `commands.getRunState` returns an anonymous inline struct (specta inlines the
  // `Option<T>` return); it is structurally identical to `RunInfoPayload`.
  return commands.getRunState(slug);
}

/** Replay buffered log lines for an instance. Returns `null` when untracked. */
export function getRunLogs(slug: string): Promise<RunLogPayload[] | null> {
  return commands.getRunLogs(slug);
}

/** Read the current Download-Manager task snapshot (all tasks, any status). */
export function listTasks(): Promise<Task[]> {
  return unwrap(commands.listTasks());
}

/** Cancel a task by id. Idempotent; no-op for unknown / already-terminal ids. */
export async function cancelTask(id: number): Promise<void> {
  await unwrap(commands.cancelTask(id));
}
