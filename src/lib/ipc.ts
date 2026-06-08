import { invoke } from "@tauri-apps/api/core";

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
