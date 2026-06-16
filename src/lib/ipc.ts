import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/**
 * Typed wrappers over Tauri commands. Keep every `invoke` call behind a named
 * function here so the command surface is discoverable and the arg/return types
 * live in one place. Later phases will generate these from Rust (specta/ts-rs).
 */

export interface AppInfo {
  name: string;
  version: string;
  tauriVersion: string;
}

export function getAppInfo(): Promise<AppInfo> {
  return invoke<AppInfo>("app_info");
}

// --- Phase 1: instances. Mirrors `src-tauri/src/core/instances.rs`. ---

export type LoaderKind = "vanilla" | "fabric" | "quilt" | "forge" | "neoforge";

export interface Loader {
  kind: LoaderKind;
  version: string | null;
}

export interface JavaCfg {
  major: number | null;
  argsOverride: string | null;
  memoryMb: number;
}

export interface InstanceSource {
  provider: string;
  projectId: string;
  fileId: string;
  packVersion: string;
}

export interface ModEntry {
  provider: string;
  projectId: string;
  versionId: string;
  fileName: string;
  hashes: Record<string, string>;
  enabled: boolean;
  side: string;
}

export interface Instance {
  schema: number;
  id: string;
  name: string;
  slug: string;
  icon: string | null;
  minecraft: string;
  loader: Loader;
  java: JavaCfg;
  source: InstanceSource | null;
  mods: ModEntry[];
  created: string;
  lastPlayed: string | null;
  totalPlaytimeSec: number;
}

/** A jar present in `mc/mods/`, whether or not it's tracked in `mods[]`. */
export interface FolderMod {
  fileName: string;
  disabled: boolean;
  sizeBytes: number;
  managed: boolean;
}

export interface InstanceDetail {
  instance: Instance;
  folderMods: FolderMod[];
}

export interface CreateInstanceReq {
  name: string;
  minecraft: string;
  loader: Loader;
}

export function listInstances(): Promise<Instance[]> {
  return invoke<Instance[]>("list_instances");
}

export function createInstance(req: CreateInstanceReq): Promise<Instance> {
  return invoke<Instance>("create_instance", { req });
}

export function getInstance(slug: string): Promise<InstanceDetail> {
  return invoke<InstanceDetail>("get_instance", { slug });
}

export function deleteInstance(slug: string): Promise<void> {
  return invoke<void>("delete_instance", { slug });
}

// --- Phase 1: global settings. Mirrors `src-tauri/src/core/settings.rs`. ---

export interface Settings {
  schema: number;
  defaultMemoryMb: number;
  defaultJavaArgs: string;
  curseforgeApiKey: string | null;
}

export interface AppPaths {
  dataDir: string;
  instancesDir: string;
}

export function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

export function saveSettings(settings: Settings): Promise<Settings> {
  return invoke<Settings>("save_settings", { settings });
}

export function getAppPaths(): Promise<AppPaths> {
  return invoke<AppPaths>("app_paths");
}

// --- Version & loader metadata. Mirrors core/versions.rs + core/loaders.rs. ---

export interface McVersion {
  id: string;
  releaseTime: string;
}

/** A loader available for a given MC version, with its builds (newest first). */
export interface LoaderOption {
  kind: LoaderKind;
  versions: string[];
}

/** Stable Minecraft releases, newest first, down to 1.7.10. */
export function listMinecraftVersions(): Promise<McVersion[]> {
  return invoke<McVersion[]>("list_minecraft_versions");
}

/** Loaders (incl. vanilla) available for `minecraft`, each with its build list. */
export function getLoaders(minecraft: string): Promise<LoaderOption[]> {
  return invoke<LoaderOption[]>("get_loaders", { minecraft });
}

// --- Phase 2: download engine. Mirrors core/download.rs. ---

/** A single file to download as part of a plan. */
export interface DownloadItem {
  url: string;
  /** Absolute destination path on disk. */
  dest: string;
  /** Expected hash for verification; null disables hash-checking (not recommended). */
  expectedHash:
    | { type: "sha1"; value: string }
    | { type: "sha256"; value: string }
    | { type: "sha512"; value: string }
    | null;
  /** Expected file size in bytes; null if unknown. */
  size: number | null;
}

/** An ordered list of items to download as a unit. */
export interface DownloadPlan {
  items: DownloadItem[];
}

/** Per-item outcome status returned by executeDownloadPlan. */
export type ItemStatus =
  | { kind: "ok" }
  | { kind: "skipped" }
  | { kind: "failed"; error: string };

/** The outcome of a single item within an executed plan. */
export interface ItemOutcome {
  url: string;
  status: ItemStatus;
}

/** Aggregated result returned by executeDownloadPlan. */
export interface PlanResult {
  outcomes: ItemOutcome[];
}

/**
 * Payload emitted on the `download://progress` Tauri event channel.
 * Subscribe with `listen("download://progress", handler)`.
 */
export interface DownloadProgressPayload {
  /** Source URL of the item currently downloading. */
  url: string;
  /** Bytes received so far for this item. */
  bytesDone: number;
  /** Total expected bytes; null when Content-Length was absent. */
  bytesTotal: number | null;
}

/**
 * Execute a DownloadPlan concurrently.
 *
 * Progress is emitted on the `download://progress` event channel —
 * subscribe with `listen("download://progress", handler)` to receive
 * DownloadProgressPayload updates.
 *
 * @param plan       The files to download.
 * @param concurrency Max simultaneous downloads (1–32). Defaults to 8 when null.
 */
export function executeDownloadPlan(
  plan: DownloadPlan,
  concurrency?: number | null,
): Promise<PlanResult> {
  return invoke<PlanResult>("execute_download_plan", { plan, concurrency: concurrency ?? null });
}

// --- Phase 2: vanilla resolver. Mirrors core/resolver.rs LaunchMeta + ResolveResult. ---

/**
 * Metadata slice D (launch) needs to build the JVM argv.
 * All `${...}` placeholders are left intact — substitution is slice D's job.
 */
export interface LaunchMeta {
  versionId: string;
  /** The Mojang manifest `type` field: `"release"`, `"snapshot"`, etc. */
  versionType: string;
  mainClass: string;
  /** JVM arg templates (OS-filtered modern entries). Empty for legacy manifests. */
  jvmArgs: string[];
  /** Game arg templates (modern entries or legacy whitespace-split). */
  gameArgs: string[];
  assetIndexId: string;
  assetsLegacy: boolean;
  javaMajor: number;
  /** Ordered classpath dest paths: all libs + client jar last. */
  classpath: string[];
  /** Native jar dest paths — slice D extracts before launch. */
  natives: string[];
  /** Path to the logging config file; null when absent in the manifest. */
  loggingConfig: string | null;
}

/** Returned by resolveVanilla: the download plan + launch metadata. */
export interface ResolveResult {
  plan: DownloadPlan;
  launch: LaunchMeta;
}

/**
 * Resolve a vanilla Minecraft version into a DownloadPlan + LaunchMeta.
 *
 * Fetches and caches the per-version manifest and asset index (no live HTTP
 * in tests — the backend handles caching). Returns every file needed before
 * launch plus the metadata required to build the JVM argv.
 */
export function resolveVanilla(versionId: string): Promise<ResolveResult> {
  return invoke<ResolveResult>("resolve_vanilla", { versionId });
}

// --- Phase 2, slice C: Java manager. Mirrors core/java.rs. ---

/** How a JavaInstallation was found. */
export type JavaSource = "detected" | "downloaded";

/**
 * A located JRE: the Java major version and the absolute path to the
 * `java` / `java.exe` executable.
 */
export interface JavaInstallation {
  major: number;
  /** Absolute path to `java` / `java.exe`. */
  path: string;
  /** Whether this JRE was found on the system or downloaded by the launcher. */
  source: JavaSource;
}

/**
 * Detect or provision a JRE for the given Java major version.
 *
 * Probes system installs and the launcher cache first. Downloads and extracts
 * Temurin from Adoptium only on a cache miss.
 */
export function ensureJava(major: number): Promise<JavaInstallation> {
  return invoke<JavaInstallation>("ensure_java", { major });
}

// --- Phase 4, slice B: NeoForge/Forge install progress. Mirrors lib.rs InstallLogPayload. ---

/**
 * Payload emitted on the `install://log` Tauri event channel during
 * NeoForge / Forge headless installer runs.
 *
 * Distinct from `launch://log` — installer output is log-shaped (one line
 * at a time from the installer's stdout/stderr), not item-progress-shaped.
 * No `instanceId` field: only one installer runs at a time.
 *
 * Subscribe with `listen(INSTALL_LOG_EVENT, handler)`.
 */
export interface InstallLogPayload {
  /** `"stdout"` or `"stderr"`. */
  stream: string;
  line: string;
}

/** Event name constant for the install log channel. */
export const INSTALL_LOG_EVENT = "install://log" as const;

/**
 * Subscribe to the `install://log` event.
 * Returns an unlisten function — call it to unsubscribe.
 * Pattern mirrors `listenDeviceCode`.
 */
export function listenInstallLog(
  handler: (payload: InstallLogPayload) => void,
): Promise<UnlistenFn> {
  return listen<InstallLogPayload>(INSTALL_LOG_EVENT, (event) =>
    handler(event.payload),
  );
}

// --- Phase 2, slice D: vanilla launch. Mirrors lib.rs LaunchLogPayload/LaunchExitPayload. ---

/**
 * Payload emitted on the `launch://log` Tauri event channel.
 * Subscribe with `listen("launch://log", handler)`.
 */
export interface LaunchLogPayload {
  /** Slug of the instance emitting this line. */
  instanceId: string;
  /** `"stdout"` or `"stderr"`. */
  stream: string;
  line: string;
}

/**
 * Payload emitted on the `launch://exit` Tauri event channel.
 * Subscribe with `listen("launch://exit", handler)`.
 */
export interface LaunchExitPayload {
  /** Slug of the instance that exited. */
  instanceId: string;
  /** Process exit code; null if the code could not be determined. */
  code: number | null;
}

/** Event name constants for the launch channel. */
export const LAUNCH_LOG_EVENT = "launch://log" as const;
export const LAUNCH_EXIT_EVENT = "launch://exit" as const;

/**
 * Launch a vanilla Minecraft instance.
 *
 * Returns promptly; the child runs under a background task that streams
 * stdout+stderr as `launch://log` events and records playtime on exit.
 */
export function launchInstance(slug: string): Promise<void> {
  return invoke<void>("launch_instance", { slug });
}

/**
 * Stop (kill) a running Minecraft instance.
 *
 * Returns Ok if the signal was sent, Err if the instance is not running.
 */
export function killInstance(slug: string): Promise<void> {
  return invoke<void>("kill_instance", { slug });
}

// --- Phase 3: Microsoft authentication. Mirrors core/auth.rs AccountMeta + lib.rs AuthCommandError. ---

/**
 * Persisted account metadata (no secrets). Mirrors `AccountMeta` in `core/auth.rs`.
 * The MC access token and refresh token are never exposed to the frontend.
 */
export interface AccountMeta {
  id: string;
  username: string;
  xuid: string;
  /** Unix timestamp (seconds) when the MC token expires; null when unknown. */
  mcTokenExpires: number | null;
}

/**
 * Payload emitted on the `auth://device-code` event channel during `beginLogin`.
 * Subscribe with `listenDeviceCode(handler)` before awaiting `beginLogin()`.
 */
export interface DeviceCodePayload {
  userCode: string;
  verificationUri: string;
}

/**
 * Error shape returned by auth commands when the Tauri command returns Err.
 * `kind` is one of the `AuthCommandError` kind strings from lib.rs.
 */
export interface AuthCommandError {
  kind: string;
  message: string;
}

/** Event name constant for the auth device-code channel. */
export const AUTH_DEVICE_CODE_EVENT = "auth://device-code" as const;

/**
 * Subscribe to the `auth://device-code` event.
 * Returns an unlisten function — call it to unsubscribe.
 * Mirror of how `launch://log` is subscribed in InstanceDetail.tsx.
 */
export function listenDeviceCode(
  handler: (payload: DeviceCodePayload) => void,
): Promise<UnlistenFn> {
  return listen<DeviceCodePayload>(AUTH_DEVICE_CODE_EVENT, (event) =>
    handler(event.payload),
  );
}

/**
 * Begin the Microsoft device-code login flow.
 *
 * This command is long-lived: it opens the device-code request, emits an
 * `auth://device-code` event (subscribe with `listenDeviceCode` before calling
 * this), then polls Microsoft until the user completes sign-in or the code expires.
 * On success, the new account is persisted and returned.
 */
export function beginLogin(): Promise<AccountMeta> {
  return invoke<AccountMeta>("begin_login");
}

/**
 * Cancel an in-flight `beginLogin`. No-op if no login is in progress.
 */
export function cancelLogin(): Promise<void> {
  return invoke<void>("cancel_login");
}

/** Return the persisted account, or null when not logged in. */
export function getAccount(): Promise<AccountMeta | null> {
  return invoke<AccountMeta | null>("get_account");
}

/** Log out: clears the persisted account and OS keyring entry. */
export function logout(): Promise<void> {
  return invoke<void>("logout");
}

// --- Phase 5: provider browse. Mirrors core/providers.rs normalized types. ---

/**
 * Which backend owns a search result or version.
 * Matches the `ProviderKind` Rust enum serialized as camelCase strings.
 *
 * NOTE: these are RESPONSE values ("curseForge" camelCase), not the lowercase
 * routing params accepted by `searchMods`/`getModVersions` ("curseforge").
 * Keep the two usages distinct — the routing params are plain strings, not this type.
 */
export type ProviderKind = "modrinth" | "curseForge";

/**
 * Which project class to search for. Mirrors `ProjectType` in `core/providers.rs`.
 * Serialized as `"mod"` or `"modpack"` over IPC.
 */
export type ProjectType = "mod" | "modpack";

/**
 * Condensed summary of a mod project, as returned by `searchMods`.
 * Mirrors `ProjectSummary` in `core/providers.rs`.
 */
export interface ProjectSummary {
  provider: ProviderKind;
  id: string;
  slug: string;
  name: string;
  summary: string;
  downloads: number;
  iconUrl: string | null;
  categories: string[];
  /**
   * Provider project page URL.
   * Modrinth: `https://modrinth.com/{project_type}/{slug}`.
   * CurseForge: `links.websiteUrl` from the search row; `null` when absent.
   */
  pageUrl: string | null;
}

/**
 * A single downloadable file within a `ProjectVersion`.
 * `url` is `null` when the author has disabled distribution (CF `allowModDistribution: false`).
 * Mirrors `VersionFile` in `core/providers.rs`.
 */
export interface VersionFile {
  url: string | null;
  fileName: string;
  size: number | null;
  hashes: Record<string, string>;
  primary: boolean;
}

/**
 * A declared dependency of a `ProjectVersion`.
 * Mirrors `Dependency` in `core/providers.rs`.
 */
export interface Dependency {
  projectId: string | null;
  versionId: string | null;
  dependencyType: string;
}

/**
 * A specific release of a mod project.
 * Mirrors `ProjectVersion` in `core/providers.rs`.
 */
export interface ProjectVersion {
  provider: ProviderKind;
  id: string;
  name: string;
  versionNumber: string;
  gameVersions: string[];
  loaders: string[];
  files: VersionFile[];
  dependencies: Dependency[];
}

/**
 * Paginated search response returned by `searchMods`.
 * Mirrors `SearchResult` in `core/providers.rs`.
 */
export interface SearchResult {
  hits: ProjectSummary[];
  offset: number;
  total: number;
}

/**
 * Error shape returned by provider commands when the Tauri command returns Err.
 * `kind` is one of: `"key_missing"`, `"http_status"`, `"bad_response"`, `"unknown_provider"`.
 * Mirrors `ProviderCommandError` in `lib.rs`.
 */
export interface ProviderCommandError {
  kind: string;
  message: string;
}

/**
 * Search mods on the given provider.
 *
 * `provider` accepts `"modrinth"` or `"curseforge"` (lowercase routing strings,
 * distinct from the `ProviderKind` response value casing e.g. `"curseForge"`).
 * `projectType` selects the content class: `"mod"` (default) or `"modpack"`.
 * Returns a paginated `SearchResult` with `hits`, `offset`, and `total` for
 * infinite scroll. CF key absence surfaces as a rejected promise with
 * `kind: "key_missing"`.
 */
export function searchMods(
  provider: "modrinth" | "curseforge",
  query: string,
  mcVersion: string | null,
  loader: string | null,
  offset: number,
  limit: number,
  projectType: ProjectType = "mod",
): Promise<SearchResult> {
  return invoke<SearchResult>("search_mods", {
    provider,
    query,
    mcVersion,
    loader,
    offset,
    limit,
    projectType,
  });
}

/**
 * Fetch all versions of a mod project compatible with the given MC version + loader.
 *
 * `provider` accepts `"modrinth"` or `"curseforge"` (lowercase routing strings,
 * distinct from the `ProviderKind` response value casing e.g. `"curseForge"`).
 */
export function getModVersions(
  provider: "modrinth" | "curseforge",
  projectId: string,
  mcVersion: string | null,
  loader: string | null,
): Promise<ProjectVersion[]> {
  return invoke<ProjectVersion[]>("get_mod_versions", {
    provider,
    projectId,
    mcVersion,
    loader,
  });
}

// --- Phase 5 slice B: mod install. Mirrors core/mod_install.rs. ---

/**
 * A resolved mod whose distribution is disabled — must be downloaded manually.
 * Mirrors `ManualMod` in `core/mod_install.rs`.
 */
export interface ManualMod {
  provider: ProviderKind;
  projectId: string;
  versionId: string;
  fileName: string;
  pageUrl: string;
}

/**
 * A dependency for which no compatible version could be found.
 * Mirrors `UnresolvedDep` in `core/mod_install.rs`.
 */
export interface UnresolvedDep {
  projectId: string;
  reason: string;
}

/**
 * An optional dependency surfaced as a suggestion (not auto-installed).
 * Mirrors `Suggestion` in `core/mod_install.rs`.
 */
export interface Suggestion {
  projectId: string;
  versionId: string | null;
}

/**
 * A declared-incompatible mod.
 * Mirrors `IncompatibleWarning` in `core/mod_install.rs`.
 */
export interface IncompatibleWarning {
  projectId: string;
}

/**
 * A mod that had a URL but whose download failed at runtime.
 * Mirrors `FailedMod` in `core/mod_install.rs`.
 */
export interface FailedMod {
  fileName: string;
  error: string;
}

/**
 * Structured result returned by the `add_mod` command.
 * Mirrors `AddModResult` in `core/mod_install.rs`.
 */
export interface AddModResult {
  added: ModEntry[];
  manual: ManualMod[];
  unresolved: UnresolvedDep[];
  suggestions: Suggestion[];
  warnings: IncompatibleWarning[];
  failed: FailedMod[];
}

/**
 * Structured result returned by the `update_mod` command.
 * `status` is one of: `"updated"` | `"upToDate"` | `"manual"` | `"unresolved"` | `"failed"`.
 * Mirrors `UpdateModResult` in `core/mod_install.rs`.
 */
export interface UpdateModResult {
  status: string;
  entry: ModEntry | null;
  pageUrl: string | null;
  error: string | null;
}

/**
 * Install a mod (and its required transitive deps) into an instance.
 *
 * `slug` is the instance slug (target instance, not the mod's URL slug).
 * `provider` accepts `"modrinth"` or `"curseforge"` (lowercase routing strings).
 */
export function addMod(
  provider: "modrinth" | "curseforge",
  projectId: string,
  versionId: string,
  slug: string,
  mcVersion: string,
  loader: string,
): Promise<AddModResult> {
  return invoke<AddModResult>("add_mod", {
    provider,
    projectId,
    versionId,
    slug,
    mcVersion,
    loader,
  });
}

/**
 * Enable or disable an installed mod by toggling the `.disabled` suffix on its file.
 * `fileName` is the base `.jar` name (no `.disabled` suffix).
 */
export function setModEnabled(slug: string, fileName: string, enabled: boolean): Promise<void> {
  return invoke<void>("set_mod_enabled", { slug, fileName, enabled });
}

/**
 * Remove an installed mod: deletes its file and drops its manifest entry.
 * `fileName` is the base `.jar` name (no `.disabled` suffix).
 */
export function removeMod(slug: string, fileName: string): Promise<void> {
  return invoke<void>("remove_mod", { slug, fileName });
}

/**
 * Update an installed mod to its newest compatible version.
 * `projectId` is the provider project id stored in the mod's `ModEntry`.
 */
export function updateMod(slug: string, projectId: string): Promise<UpdateModResult> {
  return invoke<UpdateModResult>("update_mod", { slug, projectId });
}

// --- Phase 6 slice A: mrpack import. Mirrors MrpackImportResult in lib.rs. ---

/**
 * Result returned by the `import_mrpack` command.
 * Mirrors `MrpackImportResult` in `src-tauri/src/lib.rs` (serde camelCase).
 */
export interface MrpackImportResult {
  /** Slug of the newly created instance. */
  slug: string;
  /** Display name from the pack manifest. */
  name: string;
  /** Number of mod files successfully downloaded. */
  installed: number;
  /** Number of mod files that failed to download. */
  failed: number;
  /** Number of mod files skipped (e.g. server-only, env unsupported). */
  skipped: number;
  /** File names of the failed downloads, if any. */
  failedFiles: string[];
}

/**
 * Import a local `.mrpack` file into a new instance.
 *
 * `mrpackPath` is the absolute path to the `.mrpack` file on disk.
 * `nameOverride` optionally overrides the pack's `name` field for the instance name.
 */
export function importMrpack(
  mrpackPath: string,
  nameOverride?: string,
): Promise<MrpackImportResult> {
  return invoke<MrpackImportResult>("import_mrpack", {
    mrpackPath,
    nameOverride: nameOverride ?? null,
  });
}

// --- Phase 6 slice B: CurseForge .zip import. Mirrors CfImportResult in lib.rs. ---

/**
 * A CurseForge file the user must download manually (distribution-disabled,
 * or resolved without a usable hash).
 * Mirrors `CfManualFile` in `src-tauri/src/core/modpack.rs` (serde camelCase).
 */
export interface CfManualFile {
  projectId: number;
  fileId: number;
  fileName: string;
  pageUrl: string;
}

/**
 * Result returned by the `import_curseforge_zip` command.
 * Mirrors `CfImportResult` in `src-tauri/src/lib.rs` (serde camelCase).
 */
export interface CfImportResult {
  /** Slug of the newly created instance. */
  slug: string;
  /** Display name from the pack manifest. */
  name: string;
  /** Number of mod files successfully downloaded. */
  installed: number;
  /** Number of mod files that failed (download or resolution failure). */
  failed: number;
  /** Files the user must download manually (distribution-disabled, or no usable hash). */
  manual: CfManualFile[];
}

/**
 * Import a local CurseForge modpack `.zip` into a new instance.
 *
 * `cfZipPath` is the absolute path to the `.zip` file on disk.
 * `nameOverride` optionally overrides the pack's `name` field for the instance name.
 */
export function importCurseforgeZip(
  cfZipPath: string,
  nameOverride?: string,
): Promise<CfImportResult> {
  return invoke<CfImportResult>("import_curseforge_zip", {
    cfZipPath,
    nameOverride: nameOverride ?? null,
  });
}
