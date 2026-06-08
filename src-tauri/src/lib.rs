use serde::Serialize;
use std::sync::Arc;

mod core;
use core::download::{self, DownloadPlan, ItemOutcome, ItemStatus, PlanResult, ProgressSink, ProgressUpdate};
use core::instances::{self, CreateInstanceReq, Instance, InstanceDetail};
use core::java::JavaInstallation;
use core::launch::{self, LaunchSink, RunningRegistry};
use core::loaders::{self, LoaderOption};
use core::resolver::{self, ResolveResult};
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

// --- Phase 2: download engine. ---

/// Payload emitted on the `download://progress` Tauri event channel.
///
/// Mirrors [`ProgressUpdate`] with serde rename so the TypeScript side
/// receives camelCase field names.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressPayload {
    /// Source URL of the item currently downloading.
    url: String,
    /// Bytes received so far for this item.
    bytes_done: u64,
    /// Total expected bytes for this item; `null` when `Content-Length` was absent.
    bytes_total: Option<u64>,
}

/// A [`ProgressSink`] that emits [`ProgressPayload`] on the `download://progress`
/// Tauri event channel.
///
/// Throttle/coalesce policy lives here (not in the core loop). Currently emits
/// every chunk — a future iteration may coalesce by byte-interval or time-interval.
struct TauriEventSink {
    app: tauri::AppHandle,
}

impl ProgressSink for TauriEventSink {
    fn report(&self, update: ProgressUpdate) {
        use tauri::Emitter as _;
        // Consume url and bytes_total explicitly so the compiler sees the fields
        // as used (resolves F-2 dead-code warning from CP-1).
        let payload = ProgressPayload {
            url: update.url,
            bytes_done: update.bytes_done,
            bytes_total: update.bytes_total,
        };
        // Best-effort emit: if the webview is gone, ignore the error.
        let _ = self.app.emit("download://progress", payload);
    }
}

/// Execute a [`DownloadPlan`] concurrently, emitting per-chunk progress on
/// the `download://progress` Tauri event channel.
///
/// # Parameters
/// - `plan` — the list of files to download.
/// - `concurrency` — maximum simultaneous downloads (clamped to 1..=32).
///   Pass `null` / omit from TS to use the default (8).
#[tauri::command]
async fn execute_download_plan(
    app: tauri::AppHandle,
    plan: DownloadPlan,
    concurrency: Option<usize>,
) -> Result<PlanResult, String> {
    let n = concurrency.unwrap_or(8).clamp(1, 32);
    let client = download::build_client().map_err(|e| e.to_string())?;
    let sink = TauriEventSink { app };
    Ok(download::execute_plan(&client, &plan, &sink, n).await)
}

// --- Phase 2: vanilla resolver. ---

/// Resolve a vanilla Minecraft version into a `DownloadPlan` + `LaunchMeta`.
///
/// Fetches + caches the per-version manifest (via `piston-meta`) and asset
/// index; assembles every file that must be present before launch (client jar,
/// libraries, natives, asset objects, asset index, logging config) into a
/// single `DownloadPlan`, and computes `LaunchMeta` for slice D.
#[tauri::command]
async fn resolve_vanilla(
    app: tauri::AppHandle,
    version_id: String,
) -> Result<ResolveResult, String> {
    let spec = resolver::fetch_version_spec(&app, &version_id).await?;
    let asset_data = resolver::fetch_asset_index(&app, &spec.asset_index).await?;
    let data_dir = core::store::data_dir(&app)?;
    let target_os = resolver::host_os_name();
    let (plan, launch) = resolver::assemble(&spec, &asset_data, target_os, &data_dir);
    Ok(ResolveResult { plan, launch })
}

// --- Phase 2, slice C: Java manager. ---

/// Detect or provision a JRE matching `major`.
///
/// Probes system installs + the launcher cache first; downloads and extracts
/// Temurin from Adoptium only on a cache miss.
#[tauri::command]
async fn ensure_java(app: tauri::AppHandle, major: u32) -> Result<JavaInstallation, String> {
    core::java::ensure_java(&app, major).await
}

// --- Phase 2, slice D: vanilla launch. ---

/// Payload emitted on the `launch://log` Tauri event channel.
///
/// Mirrors the `CapturingLaunchSink` line tuple with serde rename so the
/// TypeScript side receives camelCase field names.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LaunchLogPayload {
    instance_id: String,
    /// `"stdout"` or `"stderr"`.
    stream: String,
    line: String,
}

/// Payload emitted on the `launch://exit` Tauri event channel.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LaunchExitPayload {
    instance_id: String,
    /// Process exit code; `null` if the code could not be determined.
    code: Option<i32>,
}

/// A [`LaunchSink`] that emits `launch://log` and `launch://exit` events on the
/// Tauri event channel.
struct TauriLaunchSink {
    app: tauri::AppHandle,
}

impl LaunchSink for TauriLaunchSink {
    fn log(&self, instance_id: &str, stream: &str, line: &str) {
        use tauri::Emitter as _;
        let payload = LaunchLogPayload {
            instance_id: instance_id.to_owned(),
            stream: stream.to_owned(),
            line: line.to_owned(),
        };
        let _ = self.app.emit("launch://log", payload);
    }

    fn exited(&self, instance_id: &str, code: Option<i32>) {
        use tauri::Emitter as _;
        let payload = LaunchExitPayload {
            instance_id: instance_id.to_owned(),
            code,
        };
        let _ = self.app.emit("launch://exit", payload);
    }
}

/// Launch a vanilla Minecraft instance.
///
/// Orchestrates: load instance → resolve → download → ensure Java →
/// extract natives → build argv → spawn JVM.
///
/// Returns promptly; the child runs under a background `tokio::spawn` task that
/// streams stdout+stderr as `launch://log` events and records playtime on exit.
///
/// The registry is managed as `Arc<RunningRegistry>` so the monitor task (which
/// runs under `tokio::spawn` and outlives this command's stack frame) can hold a
/// clone of the Arc and still access the shared map.
#[tauri::command]
async fn launch_instance(
    app: tauri::AppHandle,
    registry_state: tauri::State<'_, Arc<RunningRegistry>>,
    slug: String,
) -> Result<(), String> {
    use core::download::NoOpSink;

    // --- 1. Load instance manifest. ---
    let inst_detail = instances::get(&app, &slug).map_err(|e| {
        format!("failed to load instance '{slug}': {e}")
    })?;
    let inst = inst_detail.instance;

    // --- 2. Reject if already running (keyed by slug; check before any async work). ---
    {
        let guard = registry_state.lock().unwrap();
        if guard.contains_key(&inst.slug) {
            return Err(format!("instance '{}' is already running", inst.slug));
        }
    }

    // --- 3. Resolve: manifest → DownloadPlan + LaunchMeta. ---
    let spec = resolver::fetch_version_spec(&app, &inst.minecraft).await?;
    let asset_data = resolver::fetch_asset_index(&app, &spec.asset_index).await?;
    let data_dir = core::store::data_dir(&app)?;
    let target_os = resolver::host_os_name();
    let (plan, launch_meta) = resolver::assemble(&spec, &asset_data, target_os, &data_dir);

    // --- 4. Download: execute the plan; inspect outcomes before proceeding. ---
    let client = download::build_client().map_err(|e| e.to_string())?;
    let plan_result = download::execute_plan(&client, &plan, &NoOpSink, 8).await;

    // Collect failures. The engine never aborts early — all items run — so we
    // must inspect outcomes here and abort before spawning a doomed JVM.
    let failures: Vec<&ItemOutcome> = plan_result
        .outcomes
        .iter()
        .filter(|o| matches!(o.status, ItemStatus::Failed { .. }))
        .collect();

    if !failures.is_empty() {
        let first = &failures[0];
        return Err(format!(
            "{} download(s) failed; cannot launch. First failure: {}",
            failures.len(),
            first.url,
        ));
    }

    // --- 5. Ensure Java. ---
    let java_inst = core::java::ensure_java(&app, launch_meta.java_major).await?;

    // --- 6. Resolve paths. ---
    let instances_dir = core::store::instances_dir(&app)?;
    let paths = launch::LaunchPaths::new(&data_dir, &instances_dir, &inst.slug);

    // --- 7. Extract natives. ---
    launch::extract_natives(&launch_meta.natives, &paths.natives_directory)?;

    // --- 8. Build argv. ---
    let argv = launch::build_argv(&launch_meta, &paths)
        .map_err(|e| format!("argv assembly failed: {e}"))?;

    // --- 9. Spawn (registry keyed by slug). ---
    let inst_dir = instances_dir.join(&inst.slug);
    let game_dir = paths.game_directory.clone();
    let java_path = java_inst.path.clone();

    // Clone the Arc so the monitor task owns its own reference.
    // State<Arc<…>> auto-derefs to Arc<…>, so &*registry_state is &Arc<…>.
    let registry_arc = Arc::clone(&*registry_state);
    let sink = Arc::new(TauriLaunchSink { app: app.clone() });

    launch::spawn_instance(
        inst.slug.clone(),
        inst_dir,
        game_dir,
        java_path,
        argv,
        registry_arc,
        sink,
    )
    .await?;

    Ok(())
}

/// Stop (kill) a running Minecraft instance.
///
/// Fires the kill signal to the child process. The monitor task will then
/// record playtime and deregister the instance after the child actually exits.
///
/// Returns `Ok` if the signal was sent, `Err` if the instance is not running.
#[tauri::command]
fn kill_instance(
    registry_state: tauri::State<'_, Arc<RunningRegistry>>,
    slug: String,
) -> Result<(), String> {
    // registry_state is State<Arc<Mutex<…>>>; deref State then Arc to get &Mutex<…>.
    launch::kill_instance(&**registry_state, &slug)
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
    // The running instance registry is the first managed state in this app.
    // Managed as Arc<RunningRegistry> so the monitor background tasks can clone
    // the Arc and still access the shared map after the command returns.
    let registry: Arc<RunningRegistry> = Arc::new(launch::new_running_registry());

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(registry)
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
            get_loaders,
            execute_download_plan,
            resolve_vanilla,
            ensure_java,
            launch_instance,
            kill_instance
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
