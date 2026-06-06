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
