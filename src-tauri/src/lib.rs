use serde::Serialize;

mod core;
use core::instances::{self, CreateInstanceReq, Instance, InstanceDetail};
use core::loaders::{self, LoaderOption};
use core::settings::{self, Settings};
use core::versions::{self, McVersion};

/// Basic app metadata, surfaced to the UI as the Phase-0 IPC smoke test.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    name: String,
    version: String,
    tauri_version: String,
}

#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo {
        name: "Modloader".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        tauri_version: tauri::VERSION.to_string(),
    }
}

// --- Phase 1: instance management. Thin wrappers over `core::instances`. ---

#[tauri::command]
fn list_instances(app: tauri::AppHandle) -> Result<Vec<Instance>, String> {
    instances::list(&app)
}

#[tauri::command]
fn create_instance(app: tauri::AppHandle, req: CreateInstanceReq) -> Result<Instance, String> {
    instances::create(&app, req)
}

#[tauri::command]
fn get_instance(app: tauri::AppHandle, slug: String) -> Result<InstanceDetail, String> {
    instances::get(&app, &slug)
}

#[tauri::command]
fn delete_instance(app: tauri::AppHandle, slug: String) -> Result<(), String> {
    instances::delete(&app, &slug)
}

// --- Phase 1: global settings + on-disk path display. ---

#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> Result<Settings, String> {
    settings::load(&app)
}

#[tauri::command]
fn save_settings(app: tauri::AppHandle, settings: Settings) -> Result<Settings, String> {
    self::settings::save(&app, settings)
}

/// Resolved on-disk locations, surfaced read-only on the Settings page.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppPaths {
    data_dir: String,
    instances_dir: String,
}

#[tauri::command]
fn app_paths(app: tauri::AppHandle) -> Result<AppPaths, String> {
    let data = core::store::data_dir(&app)?;
    let instances = data.join("instances");
    Ok(AppPaths {
        data_dir: data.to_string_lossy().into_owned(),
        instances_dir: instances.to_string_lossy().into_owned(),
    })
}

// --- Version & loader metadata (Mojang / Forge / Fabric / Quilt / NeoForge). ---

#[tauri::command]
async fn list_minecraft_versions(app: tauri::AppHandle) -> Result<Vec<McVersion>, String> {
    versions::list_releases(&app).await
}

#[tauri::command]
async fn get_loaders(app: tauri::AppHandle, minecraft: String) -> Result<Vec<LoaderOption>, String> {
    loaders::for_mc(&app, &minecraft).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            app_info,
            list_instances,
            create_instance,
            get_instance,
            delete_instance,
            get_settings,
            save_settings,
            app_paths,
            list_minecraft_versions,
            get_loaders
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
