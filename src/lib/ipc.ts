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
