use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::Manager as _;
use tauri_specta::{Builder, collect_commands, collect_events};

// `pub` so integration tests in `tests/` can reach the pure helpers
// (e.g. `store::cache_subdir_path`). This is an internal app lib, not a
// published crate, so widening visibility for testing is harmless.
pub mod core;
use core::auth::{self, AccountMeta, AccountStore, AuthError};
use core::download::{self, CancellableStatus, DownloadPlan, ItemOutcome, ItemStatus, PlanResult, ProgressSink, ProgressUpdate, execute_plan_cancellable};
use core::forge_installer::{self, InstallerLoaderKind, InstallSink};
use core::instances::{self, CreateInstanceReq, Instance, InstanceDetail};
use core::java::JavaInstallation;
use core::launch::{self, LaunchSink, RunningRegistry};
use core::loader_profile;
use core::loaders::{self, LoaderOption};
use core::resolver::{self, ResolveResult};
use core::curseforge::CurseForgeProvider;
use core::mod_install::{AddModResult, UpdateModResult};
use core::modpack;
use core::modrinth::ModrinthProvider;
use core::providers::{
    self, cf_api_key_from, ModProvider, PackInfo, ProjectType, ProviderError,
    ReqwestProviderClient, SearchParams, SearchResult, ProjectVersion,
};
use core::settings::{self, Settings};
use core::task_manager::{ChildItem, Task, TaskJob, TaskKind, TaskManager, TaskObserver, TaskProgress, TaskSpec};
use core::versions::{self, McVersion};

// --- Phase 3: authentication ---

/// Serializable command error wrapping [`AuthError`] (F-5).
///
/// `AuthError::Http` contains `reqwest::Error` which is not `Serialize`.
/// This wrapper converts every variant to a string at the command boundary so
/// the frontend always receives a JSON-serializable `{"error": "..."}` on
/// failure. The named variant is preserved in the `kind` field for typed
/// handling in the UI.
#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct AuthCommandError {
    kind: String,
    message: String,
}

impl From<AuthError> for AuthCommandError {
    fn from(e: AuthError) -> Self {
        let kind = match &e {
            AuthError::DeviceCodeExpired => "deviceCodeExpired",
            AuthError::AuthorizationDeclined => "authorizationDeclined",
            AuthError::NoXboxAccount => "noXboxAccount",
            AuthError::XboxRegionBlocked => "xboxRegionBlocked",
            AuthError::ChildAccount => "childAccount",
            AuthError::NoMinecraftLicense => "noMinecraftLicense",
            AuthError::Http(_) => "httpError",
            AuthError::BadResponse(_) => "badResponse",
            AuthError::HttpStatus { .. } => "httpStatus",
            AuthError::Keyring(_) => "keyringError",
            AuthError::Store(_) => "storeError",
            AuthError::Internal(_) => "internalError",
        };
        AuthCommandError {
            kind: kind.to_string(),
            message: e.to_string(),
        }
    }
}

impl std::fmt::Display for AuthCommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.kind, self.message)
    }
}

// Implement serde::Serialize on AuthCommandError already done above.
// Tauri commands return Result<T, AuthCommandError>; Tauri serializes the
// Err variant as the error payload to the frontend.

/// Payload for the `auth://device-code` event emitted by `begin_login`.
#[derive(Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "auth://device-code")]
struct DeviceCodePayload {
    user_code: String,
    verification_uri: String,
}

/// Shared account store managed as `Arc<tokio::sync::Mutex<AccountStore>>`.
///
/// `tokio::sync::Mutex` because the `begin_login` command holds the store
/// across an await point (persisting the account after the full auth chain).
type SharedAccountStore = Arc<tokio::sync::Mutex<AccountStore>>;

/// Cancel token for an in-flight `begin_login` — a one-shot channel sender.
///
/// When `cancel_login` fires, it sends `()` on this channel; `begin_login`
/// polls the receiver in its poll loop and aborts on receipt.
///
/// Wrapped in `Mutex<Option<…>>` so `begin_login` can place the sender and
/// `cancel_login` can take it atomically.
type CancelToken = std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>;

/// Start the Microsoft device-code login flow.
///
/// Emits an `auth://device-code` event with `user_code` + `verification_uri`
/// before the first poll, then polls the MS token endpoint until resolved,
/// expired, or cancelled. On success, runs the Xbox chain, persists the
/// account (with the MS refresh token in the keyring), and returns the
/// `AccountMeta`.
///
/// Pattern mirrors `launch_instance`: long-lived async command that emits
/// progress events via the Tauri event channel.
#[tauri::command]
#[specta::specta]
async fn begin_login(
    app: tauri::AppHandle,
    store_state: tauri::State<'_, SharedAccountStore>,
    cancel_state: tauri::State<'_, CancelToken>,
) -> Result<AccountMeta, AuthCommandError> {
    use tauri::Emitter as _;
    use tokio::time::{sleep, Duration};

    log::info!("auth: begin_login — starting device-code flow");
    let http = auth::ReqwestAuthClient(reqwest::Client::new());

    // Request device code.
    let device_code_resp = auth::request_device_code(&http, auth::MS_DEVICE_CODE_URL)
        .await
        .map_err(AuthCommandError::from)?;

    // Emit auth://device-code event so the UI can display the code.
    let _ = app.emit("auth://device-code", DeviceCodePayload {
        user_code: device_code_resp.user_code.clone(),
        verification_uri: device_code_resp.verification_uri.clone(),
    });

    // Register a cancel oneshot so cancel_login can abort this loop.
    let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel::<()>();
    {
        let mut guard = cancel_state.lock().unwrap();
        *guard = Some(cancel_tx);
    }

    // Poll loop — interval from device code response.
    let interval = Duration::from_secs(device_code_resp.interval.max(1));
    let ms_tokens = loop {
        // Check for cancellation first (non-blocking).
        match cancel_rx.try_recv() {
            Ok(_) => {
                // User explicitly cancelled via cancel_login.
                return Err(AuthCommandError::from(AuthError::AuthorizationDeclined));
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                // Sender dropped without sending — unexpected internal condition
                // (not a user cancel; e.g. mutex poisoned or CancelToken state lost).
                return Err(AuthCommandError::from(AuthError::Internal(
                    "cancel channel closed unexpectedly".to_owned(),
                )));
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
        }

        sleep(interval).await;

        match auth::poll_token_once(&http, auth::MS_TOKEN_URL, &device_code_resp.device_code)
            .await
            .map_err(AuthCommandError::from)?
        {
            Some(tokens) => break tokens,
            None => {} // still pending — loop
        }
    };

    // Capture the MS refresh token before xbox_chain consumes MsTokens.
    // The keyring stores this token; it is used at launch time to re-derive
    // a fresh MC access token via refresh_ms_token → xbox_chain.
    let ms_refresh_token = ms_tokens.refresh_token.clone();

    // Clear cancel token — login completed.
    {
        let mut guard = cancel_state.lock().unwrap();
        *guard = None;
    }

    // Run Xbox chain → Account.
    let account = auth::xbox_chain(&http, ms_tokens)
        .await
        .map_err(AuthCommandError::from)?;

    // Build AccountMeta and persist with the MS refresh token in the keyring.
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let meta = auth::AccountMeta::from_account(&account, now_secs);

    let mut guard = store_state.lock().await;
    guard
        .set_account(meta.clone(), &ms_refresh_token)
        .map_err(AuthCommandError::from)?;

    log::info!("auth: begin_login — signed in as '{}'", meta.username);
    Ok(meta)
}

/// Cancel an in-flight `begin_login` without panic or deadlock.
///
/// If no login is in progress, this is a no-op.
#[tauri::command]
#[specta::specta]
fn cancel_login(cancel_state: tauri::State<'_, CancelToken>) {
    let mut guard = cancel_state.lock().unwrap();
    if let Some(tx) = guard.take() {
        // Receiver dropped = login already completed; silently ignore.
        let _ = tx.send(());
    }
}

/// Get the current account metadata, or `None` if not logged in.
#[tauri::command]
#[specta::specta]
async fn get_account(
    store_state: tauri::State<'_, SharedAccountStore>,
) -> Result<Option<AccountMeta>, AuthCommandError> {
    let guard = store_state.lock().await;
    Ok(guard.get_account().cloned())
}

/// Log out: clear `account.json` and the keyring entry.
#[tauri::command]
#[specta::specta]
async fn logout(
    store_state: tauri::State<'_, SharedAccountStore>,
) -> Result<(), AuthCommandError> {
    let mut guard = store_state.lock().await;
    guard.logout().map_err(AuthCommandError::from)
}

/// Basic app metadata, surfaced to the UI as the Phase-0 IPC smoke test.
#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    name: String,
    version: String,
    tauri_version: String,
}

#[tauri::command]
#[specta::specta]
fn app_info() -> AppInfo {
    AppInfo {
        name: "Modloader".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        tauri_version: tauri::VERSION.to_string(),
    }
}

// --- Phase 1: instance management. Thin wrappers over `core::instances`. ---

#[tauri::command]
#[specta::specta]
fn list_instances(app: tauri::AppHandle) -> Result<Vec<Instance>, String> {
    instances::list(&app)
}

#[tauri::command]
#[specta::specta]
fn create_instance(app: tauri::AppHandle, req: CreateInstanceReq) -> Result<Instance, String> {
    instances::create(&app, req)
}

#[tauri::command]
#[specta::specta]
fn get_instance(app: tauri::AppHandle, slug: String) -> Result<InstanceDetail, String> {
    instances::get(&app, &slug)
}

#[tauri::command]
#[specta::specta]
fn delete_instance(app: tauri::AppHandle, slug: String) -> Result<(), String> {
    instances::delete(&app, &slug)
}

// --- Phase 1: global settings + on-disk path display. ---

#[tauri::command]
#[specta::specta]
fn get_settings(app: tauri::AppHandle) -> Result<Settings, String> {
    settings::load(&app)
}

#[tauri::command]
#[specta::specta]
fn save_settings(app: tauri::AppHandle, settings: Settings) -> Result<Settings, String> {
    self::settings::save(&app, settings)
}

/// Resolved on-disk locations, surfaced read-only on the Settings page.
#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct AppPaths {
    data_dir: String,
    instances_dir: String,
}

#[tauri::command]
#[specta::specta]
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
#[derive(Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "download://progress")]
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
#[specta::specta]
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
#[specta::specta]
async fn resolve_vanilla(
    app: tauri::AppHandle,
    version_id: String,
) -> Result<ResolveResult, String> {
    let spec = resolver::fetch_version_spec(&app, &version_id).await?;
    let asset_data = resolver::fetch_asset_index(&app, &spec.asset_index).await?;
    let cache_dir = core::store::cache_dir(&app)?;
    let target_os = resolver::host_os_name();
    let (plan, launch) = resolver::assemble(&spec, &asset_data, target_os, &cache_dir);
    Ok(ResolveResult { plan, launch })
}

// --- Phase 2, slice C: Java manager. ---

/// Detect or provision a JRE matching `major`.
///
/// Probes system installs + the launcher cache first; downloads and extracts
/// Temurin from Adoptium only on a cache miss.
#[tauri::command]
#[specta::specta]
async fn ensure_java(app: tauri::AppHandle, major: u32) -> Result<JavaInstallation, String> {
    core::java::ensure_java(&app, major).await
}

// --- Phase 2, slice D: vanilla launch. ---

/// Payload emitted on the `launch://log` Tauri event channel.
///
/// Mirrors the `CapturingLaunchSink` line tuple with serde rename so the
/// TypeScript side receives camelCase field names.
#[derive(Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "launch://log")]
struct LaunchLogPayload {
    instance_id: String,
    /// `"stdout"` or `"stderr"`.
    stream: String,
    line: String,
}

/// Payload emitted on the `launch://exit` Tauri event channel.
#[derive(Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "launch://exit")]
struct LaunchExitPayload {
    instance_id: String,
    /// Process exit code; `null` if the code could not be determined.
    code: Option<i32>,
}

/// Payload emitted on the `run://update` Tauri event channel.
///
/// Fired on each run-status transition (`preparing` → `running` → terminal).
/// Mirrors [`launch::RunInfo`]'s queryable scalars so the frontend runs slice can
/// patch its state without a follow-up `get_run_state` round-trip.
#[derive(Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "run://update")]
struct RunUpdatePayload {
    /// Instance slug (registry key).
    slug: String,
    /// Lowercase status tag: `preparing` / `running` / `exited` / `killed` / `failed`.
    status: String,
    /// Process exit code on a terminal status; `null` otherwise / if unknown.
    exit_code: Option<i32>,
}

/// A [`LaunchSink`] that emits `launch://log`, `launch://exit`, and `run://update`
/// events on the Tauri event channel.
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

    fn status(&self, instance_id: &str, status: launch::RunStatus, exit_code: Option<i32>) {
        use tauri::Emitter as _;
        let payload = RunUpdatePayload {
            slug: instance_id.to_owned(),
            status: status.as_str().to_owned(),
            exit_code,
        };
        let _ = self.app.emit("run://update", payload);
    }
}

/// Run-state read payload returned by `list_running` / `get_run_state`.
///
/// Mirrors [`launch::RunInfo`] with serde camelCase for the TypeScript side.
#[derive(Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct RunInfoPayload {
    slug: String,
    /// Lowercase status tag.
    status: String,
    exit_code: Option<i32>,
    /// Milliseconds since the launch began.
    elapsed_ms: u128,
}

impl From<launch::RunInfo> for RunInfoPayload {
    fn from(info: launch::RunInfo) -> Self {
        Self {
            slug: info.slug,
            status: info.status.as_str().to_owned(),
            exit_code: info.exit_code,
            elapsed_ms: info.elapsed_ms,
        }
    }
}

/// One replayed log line returned by `get_run_logs`.
///
/// Mirrors [`launch::LogLine`] with serde camelCase.
#[derive(Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct RunLogPayload {
    /// `"stdout"` or `"stderr"`.
    stream: String,
    line: String,
}

impl From<launch::LogLine> for RunLogPayload {
    fn from(l: launch::LogLine) -> Self {
        Self {
            stream: l.stream,
            line: l.line,
        }
    }
}

// --- Download Manager (task queue) payloads + observer (CP-2). ---

/// Payload emitted on the `task://progress` Tauri event channel.
///
/// Mirrors [`task_manager::TaskProgress`] with serde camelCase. Fires per child
/// transition while a task downloads. The frontend mirror lands in CP-7.
#[derive(Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "task://progress")]
struct TaskProgressPayload {
    task_id: u64,
    /// Label of the child currently in flight; `null` between children.
    current_child: Option<String>,
    done: u32,
    total: u32,
    /// Bytes received so far for the current child (best-effort).
    bytes: u64,
}

/// Payload emitted on the `task://update` Tauri event channel — one full task
/// snapshot per lifecycle/status transition.
///
/// Mirrors [`task_manager::Task`] with serde camelCase. The status tag and
/// child-item shapes already serialize camelCase from the core types. The
/// frontend mirror lands in CP-7.
#[derive(Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "task://update")]
struct TaskUpdatePayload {
    #[serde(flatten)]
    task: Task,
}

/// A [`TaskObserver`] that emits `task://progress` / `task://update` on the
/// Tauri event channel. The single Download-Manager worker calls this from its
/// task; emission is best-effort (ignored if the webview is gone).
struct TauriTaskObserver {
    app: tauri::AppHandle,
}

impl TaskObserver for TauriTaskObserver {
    fn progress(&self, update: TaskProgress) {
        use tauri::Emitter as _;
        let payload = TaskProgressPayload {
            task_id: update.task_id,
            current_child: update.current_child,
            done: update.done,
            total: update.total,
            bytes: update.bytes,
        };
        let _ = self.app.emit("task://progress", payload);
    }

    fn status_changed(&self, task: &Task) {
        use tauri::Emitter as _;
        let payload = TaskUpdatePayload { task: task.clone() };
        let _ = self.app.emit("task://update", payload);
    }
}

/// Read the current Download-Manager task snapshot. Survives navigation —
/// any page can hydrate from this regardless of when it mounted.
#[tauri::command]
#[specta::specta]
async fn list_tasks(manager: tauri::State<'_, TaskManager>) -> Result<Vec<Task>, String> {
    Ok(manager.list().await)
}

/// Cancel a task by id. Trips the task's cancel token (a running download stops
/// acquiring new permits) and short-circuits a still-queued task to `Cancelled`.
/// Idempotent; a no-op for unknown or already-terminal ids.
#[tauri::command]
#[specta::specta]
async fn cancel_task(manager: tauri::State<'_, TaskManager>, id: u64) -> Result<(), String> {
    manager.cancel(id).await;
    Ok(())
}

/// Payload emitted on the `install://log` Tauri event channel.
///
/// Distinct from `launch://log` — installer output is log-shaped (one line at
/// a time from the installer's stdout/stderr), not item-progress-shaped.
#[derive(Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "install://log")]
struct InstallLogPayload {
    /// `"stdout"` or `"stderr"`.
    stream: String,
    line: String,
}

/// An [`InstallSink`] that emits `install://log` events on the Tauri event channel
/// and (when `registry` is set) records each line into the sole `Preparing`
/// instance's ring for replay — prep-phase log attribution.
struct TauriInstallSink {
    app: tauri::AppHandle,
    /// When present, install lines are also buffered into the preparing instance's
    /// log ring. `None` outside the launch prep path (e.g. modpack import).
    registry: Option<Arc<RunningRegistry>>,
}

impl InstallSink for TauriInstallSink {
    fn log(&self, stream: &str, line: &str) {
        use tauri::Emitter as _;
        if let Some(registry) = &self.registry {
            launch::record_prep_log(registry, stream, line);
        }
        let payload = InstallLogPayload {
            stream: stream.to_owned(),
            line: line.to_owned(),
        };
        let _ = self.app.emit("install://log", payload);
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
#[specta::specta]
async fn launch_instance(
    app: tauri::AppHandle,
    registry_state: tauri::State<'_, Arc<RunningRegistry>>,
    prep_state: tauri::State<'_, launch::PrepSemaphore>,
    store_state: tauri::State<'_, SharedAccountStore>,
    slug: String,
) -> Result<(), String> {
    use core::download::NoOpSink;

    log::info!("launch: launch_instance '{slug}'");

    // --- 1. Load instance manifest. ---
    let inst_detail = instances::get(&app, &slug).map_err(|e| {
        format!("failed to load instance '{slug}': {e}")
    })?;
    let inst = inst_detail.instance;

    // --- 2. Reject if already active (keyed by slug; only a Running/Preparing
    //        entry blocks — terminal leftovers are reusable. Check before async). ---
    {
        let guard = registry_state.lock().unwrap();
        if let Some(state) = guard.get(&inst.slug) {
            if state.status == launch::RunStatus::Running
                || state.status == launch::RunStatus::Preparing
            {
                return Err(format!("instance '{}' is already running", inst.slug));
            }
        }
    }

    // --- 2b. Acquire the prep permit, serializing the blocking prep phase
    //         (resolve → download → materialize → natives) across launches. The
    //         permit is held until the JVM spawns (step 9), then dropped — so a
    //         second launch waits for prep but both packs then run concurrently.
    //         Never block on a pack's *exit*. ---
    let prep_sem: launch::PrepSemaphore = Arc::clone(&*prep_state);
    let prep_permit = prep_sem
        .acquire_owned()
        .await
        .map_err(|_| "prep semaphore closed".to_string())?;

    // Insert a `Preparing` registry entry so prep-phase install logs attribute to
    // this (now sole) preparing instance and the run is visible from any page.
    let sink = Arc::new(TauriLaunchSink { app: app.clone() });
    launch::mark_preparing(&registry_state, &inst.slug, &*sink);

    // RAII guard: on ANY early return from the prep body below (a `?` error), mark
    // the run `Failed` (and the permit drops on scope exit). Disarmed once spawn
    // succeeds. Avoids threading failure handling through every prep step.
    let mut prep_guard = PrepFailGuard {
        registry: Arc::clone(&*registry_state),
        slug: inst.slug.clone(),
        sink: Arc::clone(&sink),
        armed: true,
    };

    // --- 3. Resolve: manifest → DownloadPlan + LaunchMeta. ---
    let spec = resolver::fetch_version_spec(&app, &inst.minecraft).await?;
    let asset_data = resolver::fetch_asset_index(&app, &spec.asset_index).await?;
    // All shared artifacts (assets, libraries, versions, installers) live under
    // cache_dir — pass it wherever consumers previously received data_dir.
    let cache_dir = core::store::cache_dir(&app)?;
    let target_os = resolver::host_os_name();
    let (mut plan, mut launch_meta) = resolver::assemble(&spec, &asset_data, target_os, &cache_dir);

    // --- 3b. Merge loader profile (fabric / quilt with a pinned version). ---
    if inst.loader.kind == "fabric" || inst.loader.kind == "quilt" {
        if let Some(ref v) = inst.loader.version {
            let profile = loader_profile::fetch_profile(&app, &inst.loader.kind, &inst.minecraft, v).await?;
            resolver::merge_loader_profile(&mut plan, &mut launch_meta, &profile, target_os, &cache_dir);
        }
    }

    // Holds the JavaInstallation produced in the forge/neoforge installer step so
    // step 5 can reuse it instead of calling ensure_java a second time.
    let mut java_inst_opt: Option<core::java::JavaInstallation> = None;

    // --- 3c. Forge / NeoForge: run headless installer (idempotent), then merge profile. ---
    if inst.loader.kind == "forge" || inst.loader.kind == "neoforge" {
        let loader_version = inst.loader.version.as_deref().ok_or_else(|| {
            format!(
                "instance '{}' has loader kind '{}' but no loader version set",
                inst.slug, inst.loader.kind
            )
        })?;

        let installer_kind = if inst.loader.kind == "neoforge" {
            InstallerLoaderKind::NeoForge
        } else {
            InstallerLoaderKind::Forge
        };

        // Ensure Java before running the installer (installer is a JVM process).
        // Stored in `java_inst_opt` so step 5 can reuse it without a second call.
        let java_for_install = core::java::ensure_java(&app, launch_meta.java_major).await?;
        java_inst_opt = Some(java_for_install.clone());

        // The install sink emits `install://log` AND records each line into the
        // sole `Preparing` instance's ring (prep-phase log attribution: install
        // lines carry no correlation id, but prep is serialized so they belong to
        // the one preparing instance).
        let install_sink = TauriInstallSink {
            app: app.clone(),
            registry: Some(Arc::clone(&*registry_state)),
        };
        let version_json_path = forge_installer::run_installer(
            installer_kind,
            loader_version,
            &inst.minecraft,
            &java_for_install.path,
            &cache_dir,
            &install_sink,
        )
        .await?;

        let profile = loader_profile::load_forge_profile(&version_json_path)?;
        resolver::merge_loader_profile(&mut plan, &mut launch_meta, &profile, target_os, &cache_dir);
    }

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

    // --- 5. Resolve effective Java config + ensure Java binary. ---
    // Load settings once here; reused at step 8 for offline_mode (avoids a
    // second settings::load call). `effective` captures per-instance overrides
    // (RAM, extra args, optional java_path) resolved against global defaults.
    let app_settings = settings::load(&app).unwrap_or_default();
    let effective = core::java_resolve::resolve_effective_java(&inst, &app_settings);

    // Java binary selection:
    //   - If the forge/neoforge installer already provisioned a JRE, reuse it
    //     (the installer itself needed a real provisioned JRE, so we keep it).
    //   - Else if the user supplied an explicit path override, use it directly
    //     and skip the ensure_java provisioning step.
    //   - Else fall through to ensure_java (auto-provision Temurin as before).
    let java_inst = match java_inst_opt {
        Some(j) => j,
        None => {
            if effective.java_path.is_some() {
                // Path override set — no provisioning needed.
                // Construct a minimal JavaInstallation so the path is available;
                // it is only used for `.path` (checked by grep in CLAUDE.md).
                core::java::JavaInstallation {
                    path: effective.java_path.clone().unwrap(),
                    major: launch_meta.java_major,
                    source: core::java::JavaSource::Detected,
                }
            } else if app_settings.auto_download_java {
                core::java::ensure_java(&app, launch_meta.java_major).await?
            } else {
                // auto_download_java is disabled — attempt detection only.
                let os = core::java::TargetOs::current();
                let data_dir = core::store::data_dir(&app).unwrap_or_default();
                let candidates = core::java::default_candidates(os, &data_dir);
                let cache_java = core::store::cache_java_dir(&app).ok();
                let cache_prefix = cache_java.as_deref();
                core::java::detect(launch_meta.java_major, &candidates, os, cache_prefix)
                    .ok_or_else(|| {
                        "No Java found and auto-download is disabled. \
                        Set a Java path in the instance's Java tab or enable \
                        automatic Java downloads in Settings."
                            .to_string()
                    })?
            }
        }
    };

    // --- 6. Resolve paths. ---
    let instances_dir = core::store::instances_dir(&app)?;
    let inst_dir = instances_dir.join(&inst.slug);
    let paths = launch::LaunchPaths::new(&cache_dir, &instances_dir, &inst.slug);

    // --- 6b. Materialize libraries + version jars into the instance tree. ---
    // Rewrites classpath and natives in launch_meta from cache paths to
    // instance-local paths, then hardlinks (or copies) the artifacts.
    // Assets stay in cache/assets (design Rec A); only libs + version jars move.
    // cache_libraries_dir / cache_versions_dir helpers in store.rs define the
    // same subdirs the resolver writes to; the rewrite uses the cache_dir root
    // directly (strip_prefix) which is equivalent and avoids calling into Tauri
    // twice. Those helpers form the public cache API and remain for callers that
    // need a created-if-missing dir reference.
    let rel_paths = launch::rewrite_classpath_for_instance(&cache_dir, &inst_dir, &mut launch_meta);
    core::materialize::materialize(&cache_dir, &inst_dir, &rel_paths)
        .map_err(|e| format!("materialize failed: {e}"))?;

    // --- 7. Extract natives. ---
    launch::extract_natives(&launch_meta.natives, &paths.natives_directory)?;

    // --- 8. Resolve launch identity. ---
    // The MC token is never cached (CP3 decision), so the resolver always performs
    // a full MS refresh → Xbox chain when an active account is present. Lock is
    // held for the duration of the async call; the resolver drops no sub-locks
    // internally — single lock site here. app_settings loaded at step 5.
    let http = auth::ReqwestAuthClient(reqwest::Client::new());
    let identity = {
        let mut store_guard = store_state.lock().await;
        launch::resolve_launch_identity(&mut store_guard, &http, app_settings.offline_mode)
            .await
            .map_err(|e| format!("identity resolution failed: {e}"))?
    };

    // --- 8b. Build argv. ---
    let argv = launch::build_argv(&launch_meta, &paths, &identity, &effective)
        .map_err(|e| format!("argv assembly failed: {e}"))?;

    // --- 9. Spawn — transition `Preparing` → `Running`. The prep permit is held
    //        until spawn returns, then dropped so the next launch's prep can begin
    //        while this pack runs. Never wait on the pack's exit. ---
    let game_dir = paths.game_directory.clone();
    // When a path_override is set, it wins for the launch binary (even if the
    // installer also ran — the installer needed a provisioned JRE, but the user
    // wants their own binary for the actual game launch).
    let java_path = effective.java_path.clone().unwrap_or_else(|| java_inst.path.clone());

    // Clone the Arc so the monitor task owns its own reference.
    // State<Arc<…>> auto-derefs to Arc<…>, so &*registry_state is &Arc<…>.
    let registry_arc = Arc::clone(&*registry_state);

    let spawn_res = launch::spawn_instance(
        inst.slug.clone(),
        inst_dir,
        game_dir,
        java_path,
        argv,
        registry_arc,
        Arc::clone(&sink),
    )
    .await;

    // Release the prep permit now the JVM has spawned (or failed to). On error the
    // `prep_guard` (still armed) marks the run `Failed` when it drops.
    drop(prep_permit);
    spawn_res?;

    // Spawn succeeded — disarm the guard so it does NOT mark the live run `Failed`.
    prep_guard.armed = false;

    log::info!("launch: launch_instance '{slug}' — spawn succeeded");
    Ok(())
}

/// RAII guard that marks a launch's run `Failed` if it drops while still armed.
///
/// Installed once a `Preparing` entry exists; disarmed only after `spawn_instance`
/// succeeds. Any `?`-driven early return from the prep body drops the guard armed,
/// flipping the stale `Preparing` entry to `Failed` (and the prep permit drops on
/// the same scope exit) so a future launch is not blocked by a phantom entry.
struct PrepFailGuard {
    registry: Arc<RunningRegistry>,
    slug: String,
    sink: Arc<TauriLaunchSink>,
    armed: bool,
}

impl Drop for PrepFailGuard {
    fn drop(&mut self) {
        if self.armed {
            launch::mark_failed(&self.registry, &self.slug, &*self.sink);
        }
    }
}

/// Stop (kill) a running Minecraft instance.
///
/// Fires the kill signal to the child process. The monitor task will then
/// record playtime and deregister the instance after the child actually exits.
///
/// Returns `Ok` if the signal was sent, `Err` if the instance is not running.
#[tauri::command]
#[specta::specta]
fn kill_instance(
    registry_state: tauri::State<'_, Arc<RunningRegistry>>,
    slug: String,
) -> Result<(), String> {
    // registry_state is State<Arc<Mutex<…>>>; deref State then Arc to get &Mutex<…>.
    launch::kill_instance(&**registry_state, &slug)
}

/// Enumerate the currently non-terminal (preparing / running) instances.
///
/// Lets any page hydrate the running-indicator state regardless of when it
/// mounted. Terminal entries (exited / killed / failed) are excluded.
#[tauri::command]
#[specta::specta]
fn list_running(registry_state: tauri::State<'_, Arc<RunningRegistry>>) -> Vec<RunInfoPayload> {
    launch::list_running(&**registry_state)
        .into_iter()
        .map(RunInfoPayload::from)
        .collect()
}

/// Read a single instance's run state (any status, incl. terminal). `None` if the
/// slug is not tracked — lets `InstanceDetail` recover status + a missed exit code.
#[tauri::command]
#[specta::specta]
fn get_run_state(
    registry_state: tauri::State<'_, Arc<RunningRegistry>>,
    slug: String,
) -> Option<RunInfoPayload> {
    launch::get_run_state(&**registry_state, &slug).map(RunInfoPayload::from)
}

/// Replay an instance's buffered log lines (works after exit; entry is retained).
/// `None` if the slug is not tracked.
#[tauri::command]
#[specta::specta]
fn get_run_logs(
    registry_state: tauri::State<'_, Arc<RunningRegistry>>,
    slug: String,
) -> Option<Vec<RunLogPayload>> {
    launch::get_run_logs(&**registry_state, &slug)
        .map(|lines| lines.into_iter().map(RunLogPayload::from).collect())
}

// --- Version & loader metadata (Mojang / Forge / Fabric / Quilt / NeoForge). ---

#[tauri::command]
#[specta::specta]
async fn list_minecraft_versions(app: tauri::AppHandle) -> Result<Vec<McVersion>, String> {
    versions::list_releases(&app).await
}

#[tauri::command]
#[specta::specta]
async fn get_loaders(app: tauri::AppHandle, minecraft: String) -> Result<Vec<LoaderOption>, String> {
    loaders::for_mc(&app, &minecraft).await
}

// --- Phase 5: provider browse (search + versions) ---

/// Serializable command error wrapping [`ProviderError`].
///
/// Mirrors `AuthCommandError` above. Every variant maps to a camelCase `kind`
/// string; `"key_missing"` lets the frontend show the "API key required" state.
#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct ProviderCommandError {
    kind: String,
    message: String,
}

impl From<ProviderError> for ProviderCommandError {
    fn from(e: ProviderError) -> Self {
        let kind = match &e {
            ProviderError::KeyMissing => "key_missing",
            ProviderError::Network(_) => "network",
            ProviderError::HttpStatus { .. } => "http_status",
            ProviderError::BadResponse(_) => "bad_response",
        };
        ProviderCommandError {
            kind: kind.to_string(),
            message: e.to_string(),
        }
    }
}

impl std::fmt::Display for ProviderCommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.kind, self.message)
    }
}

/// Build the `unknown_provider` error for an unrecognised provider string.
///
/// Extracted so both commands share the same error path and tests can exercise
/// the real construction logic (not a hand-rolled constant).
fn unknown_provider_err(other: &str) -> ProviderCommandError {
    ProviderCommandError {
        kind: "unknown_provider".to_string(),
        message: format!("unknown provider: {other:?}"),
    }
}

/// Search mods across a single provider.
///
/// `provider` must be `"modrinth"` or `"curseforge"` (case-sensitive).
/// Unknown provider strings return a typed `unknown_provider` error rather than panicking.
/// The CF key is resolved from `MODLOADER_CF_API_KEY` env or `settings.curseforge_api_key`.
/// `project_type` selects the content class: `"mod"` (default) or `"modpack"`.
#[tauri::command]
#[specta::specta]
async fn search_mods(
    app: tauri::AppHandle,
    provider: String,
    query: String,
    mc_version: Option<String>,
    loader: Option<String>,
    offset: u32,
    limit: u32,
    project_type: Option<ProjectType>,
) -> Result<SearchResult, ProviderCommandError> {
    let params = SearchParams {
        query,
        mc_version,
        loader,
        offset,
        limit,
        project_type: project_type.unwrap_or_default(),
    };
    let http = ReqwestProviderClient(reqwest::Client::new());

    match provider.as_str() {
        "modrinth" => {
            let p = ModrinthProvider;
            p.search(&http, &params).await.map_err(ProviderCommandError::from)
        }
        "curseforge" => {
            let settings = settings::load(&app).unwrap_or_default();
            let key = cf_api_key_from(
                std::env::var(providers::CF_API_KEY_ENV).ok(),
                settings.curseforge_api_key,
                option_env!("MODLOADER_CF_API_KEY").map(str::to_string),
            );
            let p = CurseForgeProvider::new(key);
            p.search(&http, &params).await.map_err(ProviderCommandError::from)
        }
        other => Err(unknown_provider_err(other)),
    }
}

/// Fetch versions of a mod project compatible with the supplied MC version + loader.
///
/// `provider` must be `"modrinth"` or `"curseforge"`.
#[tauri::command]
#[specta::specta]
async fn get_mod_versions(
    app: tauri::AppHandle,
    provider: String,
    project_id: String,
    mc_version: Option<String>,
    loader: Option<String>,
) -> Result<Vec<ProjectVersion>, ProviderCommandError> {
    let http = ReqwestProviderClient(reqwest::Client::new());

    match provider.as_str() {
        "modrinth" => {
            let p = ModrinthProvider;
            p.get_versions(&http, &project_id, mc_version.as_deref(), loader.as_deref())
                .await
                .map_err(ProviderCommandError::from)
        }
        "curseforge" => {
            let settings = settings::load(&app).unwrap_or_default();
            let key = cf_api_key_from(
                std::env::var(providers::CF_API_KEY_ENV).ok(),
                settings.curseforge_api_key,
                option_env!("MODLOADER_CF_API_KEY").map(str::to_string),
            );
            let p = CurseForgeProvider::new(key);
            p.get_versions(&http, &project_id, mc_version.as_deref(), loader.as_deref())
                .await
                .map_err(ProviderCommandError::from)
        }
        other => Err(unknown_provider_err(other)),
    }
}

/// Fetch rich project info (title, long description, icon) for the Info tab.
///
/// `provider` must be `"modrinth"` or `"curseforge"` (case-sensitive).
/// Modrinth returns Markdown (`body_is_html: false`); CurseForge returns HTML
/// (`body_is_html: true`). The CF key is resolved exactly like `search_mods`.
#[tauri::command]
#[specta::specta]
async fn get_pack_info(
    app: tauri::AppHandle,
    provider: String,
    project_id: String,
) -> Result<PackInfo, ProviderCommandError> {
    let http = ReqwestProviderClient(reqwest::Client::new());

    match provider.as_str() {
        "modrinth" => {
            let p = ModrinthProvider;
            p.get_project(&http, &project_id)
                .await
                .map_err(ProviderCommandError::from)
        }
        "curseforge" => {
            let settings = settings::load(&app).unwrap_or_default();
            let key = cf_api_key_from(
                std::env::var(providers::CF_API_KEY_ENV).ok(),
                settings.curseforge_api_key,
                option_env!("MODLOADER_CF_API_KEY").map(str::to_string),
            );
            let p = CurseForgeProvider::new(key);
            p.get_project(&http, &project_id)
                .await
                .map_err(ProviderCommandError::from)
        }
        other => Err(unknown_provider_err(other)),
    }
}

/// Install a mod (and its required transitive dependencies) into an instance.
///
/// # Flow
/// 1. Construct the provider + HTTP client (mirrors `search_mods` / `get_mod_versions`).
/// 2. Fetch the requested version via `get_versions` filtered to the exact `version_id`.
/// 3. Load the instance manifest; derive `already_installed` from `instance.mods`.
/// 4. Run `resolve_install` (BFS dependency walk).
/// 5. Build a `DownloadPlan` from `plan.downloads`; execute with `NoOpSink`.
/// 6. For each successful download, build a `ModEntry` and merge into the manifest.
/// 7. Save the manifest; return `AddModResult`.
///
/// Per-item download failures are captured in `AddModResult.failed`; they do NOT
/// abort the whole operation (remaining items still execute).
/// Enqueue an add-mod task and return its task id synchronously.
///
/// The `ensure_not_locked` guard runs here (synchronously, before enqueue) so a
/// locked instance errors without touching the queue. All download work runs
/// inside [`ModAddJob`].
#[tauri::command]
#[specta::specta]
async fn add_mod(
    app: tauri::AppHandle,
    manager: tauri::State<'_, TaskManager>,
    provider: String,
    project_id: String,
    version_id: String,
    slug: String,
    mc_version: String,
    loader: String,
) -> Result<u64, String> {
    // Validate provider string early so a bad caller gets a synchronous error.
    match provider.as_str() {
        "modrinth" | "curseforge" => {}
        other => return Err(format!("unknown provider: {other}")),
    }

    // Guard: reject if pack-locked (synchronous, before enqueue).
    let instance = instances::load_manifest(&app, &slug)?;
    instances::ensure_not_locked(&instance)?;

    log::info!("mod: add_mod enqueue project_id={project_id} version={version_id} instance={slug}");

    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    let task_id_cell: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
    let task_id_cell_job = Arc::clone(&task_id_cell);

    let id = manager.enqueue(TaskSpec {
        kind: TaskKind::ModAdd,
        parent_label: format!("Add mod {project_id}"),
        job: Box::new(ModAddJob {
            app: app.clone(),
            task_id_cell: task_id_cell_job,
            provider,
            project_id,
            version_id,
            slug,
            mc_version,
            loader,
        }),
    }).await;

    task_id_cell.store(id, Ordering::SeqCst);
    Ok(id)
}

// ---------------------------------------------------------------------------
// CP3: Mod state operations
// ---------------------------------------------------------------------------

/// Enable or disable an installed mod by renaming its file and flipping the
/// `enabled` flag in the manifest.
///
/// `file_name` is the base `.jar` name (no `.disabled` suffix). Both `slug`
/// and `file_name` are validated for path traversal before any FS access.
/// Blocked when the instance is pack-locked.
#[tauri::command]
#[specta::specta]
fn set_mod_enabled(
    app: tauri::AppHandle,
    slug: String,
    file_name: String,
    enabled: bool,
) -> Result<(), String> {
    let instance = instances::load_manifest(&app, &slug)?;
    instances::ensure_not_locked(&instance)?;
    instances::set_mod_enabled(&app, &slug, &file_name, enabled)
}

/// Delete a mod's on-disk file (enabled or disabled form) and drop its
/// manifest entry. Missing file is not an error — system converges to "gone".
///
/// `file_name` is the base `.jar` name (no `.disabled` suffix). Both `slug`
/// and `file_name` are validated for path traversal before any FS access.
/// Blocked when the instance is pack-locked.
#[tauri::command]
#[specta::specta]
fn remove_mod(app: tauri::AppHandle, slug: String, file_name: String) -> Result<(), String> {
    let instance = instances::load_manifest(&app, &slug)?;
    instances::ensure_not_locked(&instance)?;
    instances::remove_mod(&app, &slug, &file_name)
}

// ---------------------------------------------------------------------------
// D4: Pack Lock toggle
// ---------------------------------------------------------------------------

/// Set or clear the pack-lock flag on an instance.
///
/// When locked, mod-mutation commands (`add_mod`, `set_mod_enabled`,
/// `remove_mod`, `update_mod`) will return an error. `update_modpack` is
/// deliberately left unguarded — updating the pack is how you change it.
#[tauri::command]
#[specta::specta]
fn set_pack_lock(app: tauri::AppHandle, slug: String, locked: bool) -> Result<(), String> {
    instances::set_pack_lock(&app, &slug, locked)
}

// ---------------------------------------------------------------------------
// D-3: Java tab — set per-instance Java config + validate a Java path
// ---------------------------------------------------------------------------

/// Persist a `JavaCfg` override to an instance's manifest.
///
/// When `cfg.use_pack_settings` is `true` the launcher uses this instance's own
/// Java/memory settings at launch; when `false` it falls back to the global
/// default from `Settings`. Resolution is handled by `resolve_effective_java`
/// (WS-A) — this command only persists the user's choice.
#[tauri::command]
#[specta::specta]
fn set_instance_java(
    app: tauri::AppHandle,
    slug: String,
    java: crate::core::instances::JavaCfg,
) -> Result<(), String> {
    instances::set_instance_java(&app, &slug, java)
}

/// Probe a Java binary by running `<path> -version` and parsing its stderr.
///
/// Returns a [`JavaProbe`] with the major version and full version string on
/// success, or a human-readable `Err` when the binary cannot be spawned or its
/// output doesn't contain a recognisable `java version "…"` line.
#[tauri::command]
#[specta::specta]
async fn validate_java_path(path: String) -> Result<crate::core::java::JavaProbe, String> {
    use tokio::process::Command;

    let output = Command::new(&path)
        .arg("-version")
        .output()
        .await
        .map_err(|e| format!("failed to spawn '{path}': {e}"))?;

    // `java -version` writes to stderr on all JVM vendors.
    let stderr = String::from_utf8_lossy(&output.stderr);

    let (major, version) = crate::core::java::parse_java_version_output(&stderr)
        .ok_or_else(|| format!("could not parse java version from output of '{path}'"))?;

    Ok(crate::core::java::JavaProbe { major, version })
}

// ---------------------------------------------------------------------------
// CP4: Update a single installed mod
// ---------------------------------------------------------------------------

/// Enqueue an update-mod task and return its task id synchronously.
///
/// The `ensure_not_locked` guard runs here (synchronously, before enqueue) so a
/// locked instance errors without touching the queue. The entry-existence check
/// also runs here so a bad project_id errors immediately. All download work runs
/// inside [`ModUpdateJob`].
#[tauri::command]
#[specta::specta]
async fn update_mod(
    app: tauri::AppHandle,
    manager: tauri::State<'_, TaskManager>,
    slug: String,
    project_id: String,
) -> Result<u64, String> {
    // Guard: reject if pack-locked (synchronous, before enqueue).
    let instance = instances::load_manifest(&app, &slug)?;
    instances::ensure_not_locked(&instance)?;

    // Validate the mod entry exists.
    if !instance.mods.iter().any(|m| m.project_id == project_id) {
        return Err(format!("mod with project_id '{project_id}' not found in instance '{slug}'"));
    }

    log::info!("mod: update_mod enqueue project_id={project_id} instance={slug}");

    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    let task_id_cell: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
    let task_id_cell_job = Arc::clone(&task_id_cell);

    let id = manager.enqueue(TaskSpec {
        kind: TaskKind::ModUpdate,
        parent_label: format!("Update mod {project_id}"),
        job: Box::new(ModUpdateJob {
            app: app.clone(),
            task_id_cell: task_id_cell_job,
            slug,
            project_id,
        }),
    }).await;

    task_id_cell.store(id, Ordering::SeqCst);
    Ok(id)
}

// ---------------------------------------------------------------------------
// CP-4: ModAddJob
// ---------------------------------------------------------------------------

/// [`TaskJob`] that installs a mod (and required transitive deps) into an instance.
///
/// Pattern mirrors [`ImportMrpackJob`]:
/// - PLAN: fetch root version + run BFS dep resolver → build child queue.
/// - DOWNLOAD: stage into `<inst_dir>/.staging-<task_id>/mods/` with cancel support.
/// - APPLY: promote staging → `mc/mods/`, merge mod entries, save manifest.
///
/// Terminal result = [`AddModResult`] JSON on `task://update`.
struct ModAddJob {
    app: tauri::AppHandle,
    task_id_cell: std::sync::Arc<std::sync::atomic::AtomicU64>,
    provider: String,
    project_id: String,
    version_id: String,
    slug: String,
    mc_version: String,
    loader: String,
}

#[async_trait::async_trait]
impl TaskJob for ModAddJob {
    async fn run(self: Box<Self>, ctx: &core::task_manager::TaskContext) {
        use core::mod_install::{attribute_outcomes, build_download_items, merge_mod_entries, partition_by_file_name, resolve_install};
        use core::modpack::{promote_staging, remap_to_staging};
        use std::collections::HashSet;
        use std::sync::atomic::Ordering;

        let task_id = self.task_id_cell.load(Ordering::SeqCst);
        log::info!("mod: ModAddJob task_id={task_id} project_id={} instance={}", self.project_id, self.slug);

        // --- PLAN ---
        ctx.enter_planning().await;

        // Build provider + HTTP client.
        let http = ReqwestProviderClient(reqwest::Client::new());
        let settings = settings::load(&self.app).unwrap_or_default();
        let cf_key = cf_api_key_from(
            std::env::var(providers::CF_API_KEY_ENV).ok(),
            settings.curseforge_api_key,
            option_env!("MODLOADER_CF_API_KEY").map(str::to_string),
        );

        // Fetch the root version.
        let versions = match self.provider.as_str() {
            "modrinth" => {
                let p = ModrinthProvider;
                match p.get_versions(&http, &self.project_id, Some(&self.mc_version), Some(&self.loader)).await {
                    Ok(v) => v,
                    Err(e) => { ctx.finish_failed(e.to_string()).await; return; }
                }
            }
            "curseforge" => {
                let p = CurseForgeProvider::new(cf_key.clone());
                match p.get_versions(&http, &self.project_id, Some(&self.mc_version), Some(&self.loader)).await {
                    Ok(v) => v,
                    Err(e) => { ctx.finish_failed(e.to_string()).await; return; }
                }
            }
            other => { ctx.finish_failed(format!("unknown provider: {other}")).await; return; }
        };

        let root_version = match versions.into_iter().find(|v| v.id == self.version_id) {
            Some(v) => v,
            None => {
                ctx.finish_failed(format!("version '{}' not found for project '{}'", self.version_id, self.project_id)).await;
                return;
            }
        };

        // Load manifest (re-load in job for freshness; lock was already checked in command).
        let mut instance = match instances::load_manifest(&self.app, &self.slug) {
            Ok(i) => i,
            Err(e) => { ctx.finish_failed(e).await; return; }
        };
        let already_installed: HashSet<String> = instance.mods.iter().map(|m| m.project_id.clone()).collect();

        // BFS dep resolver.
        let plan = match self.provider.as_str() {
            "modrinth" => {
                let p = ModrinthProvider;
                resolve_install(&p, &http, root_version, &self.project_id, &self.project_id, &self.mc_version, &self.loader, &already_installed).await
            }
            "curseforge" => {
                let p = CurseForgeProvider::new(cf_key);
                resolve_install(&p, &http, root_version, &self.project_id, &self.project_id, &self.mc_version, &self.loader, &already_installed).await
            }
            other => { ctx.finish_failed(format!("unknown provider: {other}")).await; return; }
        };

        // Validate + partition download items.
        let (valid_downloads, invalid_downloads) = partition_by_file_name(plan.downloads.clone());

        let inst_dir = match core::store::instances_dir(&self.app) {
            Ok(d) => d.join(&self.slug),
            Err(e) => { ctx.finish_failed(e).await; return; }
        };
        let mc_dir = inst_dir.join("mc");
        let mods_dir = mc_dir.join("mods");

        if let Err(e) = std::fs::create_dir_all(&mods_dir) {
            ctx.finish_failed(format!("could not create mods dir: {e}")).await;
            return;
        }

        let items = build_download_items(
            &core::mod_install::InstallPlan {
                downloads: valid_downloads.clone(),
                ..Default::default()
            },
            &mods_dir,
        );

        let staging_dir = staging_dir_for(&inst_dir, task_id);

        // Remap destinations from mc_dir → staging_dir (preserves mods/ sub-path).
        let staged_items = remap_to_staging(items, &mc_dir, &staging_dir);
        let children = children_from_items(&staged_items);

        // --- DOWNLOAD ---
        let staged_plan = DownloadPlan::new(staged_items);
        ctx.enter_downloading(children).await;

        let http_client = match download::build_client() {
            Ok(c) => c,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&staging_dir);
                ctx.finish_failed(e.to_string()).await;
                return;
            }
        };

        let dl_result = execute_plan_cancellable(
            &http_client,
            &staged_plan,
            &core::download::NoOpSink,
            8,
            ctx.cancel_token(),
        ).await;

        if dl_result.cancelled || ctx.is_cancelled() {
            let _ = std::fs::remove_dir_all(&staging_dir);
            ctx.finish_cancelled().await;
            return;
        }

        // Build a synthetic PlanResult from CancellableOutcomes for attribute_outcomes.
        let plan_result = {
            use core::download::{ItemOutcome, PlanResult};
            let outcomes: Vec<ItemOutcome> = dl_result.outcomes.iter().filter_map(|o| {
                match &o.status {
                    CancellableStatus::Ran(s) => Some(ItemOutcome {
                        url: o.url.clone(),
                        status: s.clone(),
                    }),
                    CancellableStatus::Cancelled => None,
                }
            }).collect();
            PlanResult { outcomes }
        };

        let (added, mut dl_failed) = attribute_outcomes(&valid_downloads, &plan_result);

        // --- APPLY ---
        ctx.enter_applying().await;

        if let Err(e) = promote_staging(&staging_dir, &mc_dir) {
            let _ = std::fs::remove_dir_all(&staging_dir);
            ctx.finish_failed(format!("promote failed: {e}")).await;
            return;
        }
        let _ = std::fs::remove_dir_all(&staging_dir);

        // Merge entries + persist manifest.
        merge_mod_entries(&mut instance.mods, added.clone());
        if let Err(e) = instances::save_manifest(&self.app, &self.slug, &instance) {
            ctx.finish_failed(e).await;
            return;
        }

        let mut result = AddModResult {
            added,
            manual: plan.manual,
            unresolved: plan.unresolved,
            suggestions: plan.suggestions,
            warnings: plan.warnings,
            failed: invalid_downloads,
        };
        result.failed.append(&mut dl_failed);

        log::info!(
            "mod: ModAddJob done task_id={task_id} — added={} failed={} manual={}",
            result.added.len(), result.failed.len(), result.manual.len()
        );

        match serde_json::to_value(&result) {
            Ok(v) => ctx.finish_done_with_result(v).await,
            Err(e) => ctx.finish_failed(format!("serialize result: {e}")).await,
        }
    }
}

// ---------------------------------------------------------------------------
// CP-4: ModUpdateJob
// ---------------------------------------------------------------------------

/// [`TaskJob`] that updates a single installed mod to its newest compatible version.
///
/// Pattern mirrors [`ModAddJob`]:
/// - PLAN: load manifest, resolve newest compatible version, decide action.
/// - DOWNLOAD (Swap case only): stage new file in `.staging-<task_id>/mods/`.
/// - APPLY: promote staging, delete old file, update manifest entry.
///
/// No-op cases (UpToDate / Unresolved / Manual) skip Download/Apply phases and
/// finish immediately with a result payload.
///
/// Terminal result = [`UpdateModResult`] JSON on `task://update`.
struct ModUpdateJob {
    app: tauri::AppHandle,
    task_id_cell: std::sync::Arc<std::sync::atomic::AtomicU64>,
    slug: String,
    project_id: String,
}

#[async_trait::async_trait]
impl TaskJob for ModUpdateJob {
    async fn run(self: Box<Self>, ctx: &core::task_manager::TaskContext) {
        use core::download::{DownloadItem, ExpectedHash};
        use core::mod_install::{apply_swap, decide_update, fetch_newest_compatible, page_url_for, UpdateAction};
        use core::modpack::{promote_staging, remap_to_staging};
        use std::sync::atomic::Ordering;

        let task_id = self.task_id_cell.load(Ordering::SeqCst);
        log::info!("mod: ModUpdateJob task_id={task_id} project_id={} instance={}", self.project_id, self.slug);

        // --- PLAN ---
        ctx.enter_planning().await;

        let mut instance = match instances::load_manifest(&self.app, &self.slug) {
            Ok(i) => i,
            Err(e) => { ctx.finish_failed(e).await; return; }
        };

        let entry_idx = match instance.mods.iter().position(|m| m.project_id == self.project_id) {
            Some(i) => i,
            None => {
                ctx.finish_failed(format!("mod '{}' not found in instance '{}'", self.project_id, self.slug)).await;
                return;
            }
        };

        let mc_version = instance.minecraft.clone();
        let loader = instance.loader.kind.clone();

        let http = ReqwestProviderClient(reqwest::Client::new());
        let settings = settings::load(&self.app).unwrap_or_default();
        let cf_key = cf_api_key_from(
            std::env::var(providers::CF_API_KEY_ENV).ok(),
            settings.curseforge_api_key,
            option_env!("MODLOADER_CF_API_KEY").map(str::to_string),
        );

        let provider_str = instance.mods[entry_idx].provider.clone();
        let newest = match provider_str.as_str() {
            "modrinth" => {
                let p = ModrinthProvider;
                fetch_newest_compatible(&p, &http, &self.project_id, &mc_version, &loader, None).await
            }
            "curseforge" => {
                let p = CurseForgeProvider::new(cf_key);
                fetch_newest_compatible(&p, &http, &self.project_id, &mc_version, &loader, None).await
            }
            other => {
                ctx.finish_failed(format!("unknown provider '{other}' in manifest")).await;
                return;
            }
        };

        let action = decide_update(&instance.mods[entry_idx], newest.as_ref());

        // Handle non-download outcomes immediately (skip Download/Apply phases).
        let finish_result = match &action {
            UpdateAction::UpToDate => {
                log::info!("mod-update: project_id={} already up to date", self.project_id);
                Some(UpdateModResult { status: "upToDate".to_string(), entry: None, page_url: None, error: None })
            }
            UpdateAction::Unresolved => {
                log::info!("mod-update: project_id={} no matching version / unresolved", self.project_id);
                Some(UpdateModResult { status: "unresolved".to_string(), entry: None, page_url: None, error: None })
            }
            UpdateAction::Manual { .. } => {
                let newest_version = newest.as_ref().unwrap();
                let new_file = newest_version.files.iter().find(|f| f.primary).or_else(|| newest_version.files.first()).unwrap();
                let real_page_url = page_url_for(newest_version.provider, &self.project_id);
                log::warn!(
                    "mod-update: project_id={} distribution disabled — download '{}' manually from {real_page_url}",
                    self.project_id, new_file.file_name
                );
                Some(UpdateModResult {
                    status: "manual".to_string(),
                    entry: None,
                    page_url: Some(real_page_url),
                    error: Some(format!("distribution disabled; download '{}' manually", new_file.file_name)),
                })
            }
            UpdateAction::Swap { .. } => None,
        };

        if let Some(result) = finish_result {
            match serde_json::to_value(&result) {
                Ok(v) => ctx.finish_done_with_result(v).await,
                Err(e) => ctx.finish_failed(format!("serialize result: {e}")).await,
            }
            return;
        }

        // Swap path: extract new_file + new_version_id from action.
        let (new_file, new_version_id) = match action {
            UpdateAction::Swap { new_file, new_version_id } => (new_file, new_version_id),
            _ => unreachable!(),
        };

        // Validate provider file name.
        if let Err(e) = instances::validate_mod_file_name(&new_file.file_name) {
            let result = UpdateModResult {
                status: "failed".to_string(), entry: None, page_url: None,
                error: Some(format!("invalid provider file name: {e}")),
            };
            match serde_json::to_value(&result) {
                Ok(v) => ctx.finish_done_with_result(v).await,
                Err(e2) => ctx.finish_failed(format!("serialize result: {e2}")).await,
            }
            return;
        }

        let url = new_file.url.clone().unwrap(); // safe: Swap arm guarantees url is Some

        let expected_hash = if let Some(h) = new_file.hashes.get("sha512") {
            Some(ExpectedHash::Sha512(h.clone()))
        } else if let Some(h) = new_file.hashes.get("sha1") {
            Some(ExpectedHash::Sha1(h.clone()))
        } else {
            None
        };

        let enabled = instance.mods[entry_idx].enabled;
        let dest_file_name = if enabled {
            new_file.file_name.clone()
        } else {
            format!("{}.disabled", new_file.file_name)
        };

        let inst_dir = match core::store::instances_dir(&self.app) {
            Ok(d) => d.join(&self.slug),
            Err(e) => { ctx.finish_failed(e).await; return; }
        };
        let mc_dir = inst_dir.join("mc");
        let mods_dir = mc_dir.join("mods");

        if let Err(e) = std::fs::create_dir_all(&mods_dir) {
            ctx.finish_failed(format!("could not create mods dir: {e}")).await;
            return;
        }

        let staging_dir = staging_dir_for(&inst_dir, task_id);

        // Remap single-file download into staging.
        let items = vec![DownloadItem {
            url: url.clone(),
            dest: mods_dir.join(&dest_file_name),
            expected_hash,
            size: new_file.size,
        }];
        let staged_items = remap_to_staging(items, &mc_dir, &staging_dir);
        let children = children_from_items(&staged_items);

        // --- DOWNLOAD ---
        let staged_plan = DownloadPlan::new(staged_items);
        ctx.enter_downloading(children).await;

        let http_client = match download::build_client() {
            Ok(c) => c,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&staging_dir);
                ctx.finish_failed(e.to_string()).await;
                return;
            }
        };

        let dl_result = execute_plan_cancellable(
            &http_client,
            &staged_plan,
            &core::download::NoOpSink,
            1,
            ctx.cancel_token(),
        ).await;

        if dl_result.cancelled || ctx.is_cancelled() {
            let _ = std::fs::remove_dir_all(&staging_dir);
            ctx.finish_cancelled().await;
            return;
        }

        // Check download outcome.
        let download_ok = dl_result.outcomes.first().map(|o| {
            matches!(&o.status, CancellableStatus::Ran(ItemStatus::Ok) | CancellableStatus::Ran(ItemStatus::Skipped))
        }).unwrap_or(false);

        if !download_ok {
            let _ = std::fs::remove_dir_all(&staging_dir);
            let err = dl_result.outcomes.first().and_then(|o| {
                match &o.status {
                    CancellableStatus::Ran(ItemStatus::Failed { error }) => Some(error.clone()),
                    _ => None,
                }
            }).unwrap_or_else(|| "download failed: no outcome recorded".to_string());
            log::warn!("mod-update: project_id={} download failed — {err}", self.project_id);
            let result = UpdateModResult { status: "failed".to_string(), entry: None, page_url: None, error: Some(err) };
            match serde_json::to_value(&result) {
                Ok(v) => ctx.finish_done_with_result(v).await,
                Err(e) => ctx.finish_failed(format!("serialize result: {e}")).await,
            }
            return;
        }

        // --- APPLY ---
        ctx.enter_applying().await;

        if let Err(e) = promote_staging(&staging_dir, &mc_dir) {
            let _ = std::fs::remove_dir_all(&staging_dir);
            ctx.finish_failed(format!("promote failed: {e}")).await;
            return;
        }
        let _ = std::fs::remove_dir_all(&staging_dir);

        // Delete old file (both variants).
        let old_file_name = instance.mods[entry_idx].file_name.clone();
        instances::remove_mod_from_disk_files(&mods_dir, &old_file_name);

        // Update manifest entry in place.
        apply_swap(&mut instance.mods[entry_idx], &new_file, &new_version_id);
        if let Err(e) = instances::save_manifest(&self.app, &self.slug, &instance) {
            ctx.finish_failed(e).await;
            return;
        }

        log::info!(
            "mod: ModUpdateJob done task_id={task_id} — project_id={} updated to version {new_version_id}",
            self.project_id
        );

        let result = UpdateModResult {
            status: "updated".to_string(),
            entry: Some(instance.mods[entry_idx].clone()),
            page_url: None,
            error: None,
        };
        match serde_json::to_value(&result) {
            Ok(v) => ctx.finish_done_with_result(v).await,
            Err(e) => ctx.finish_failed(format!("serialize result: {e}")).await,
        }
    }
}

// ---------------------------------------------------------------------------
// CP4: Modpack import
// ---------------------------------------------------------------------------

/// Result returned to the frontend after a successful `.mrpack` import.
#[derive(Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MrpackImportResult {
    /// Slug of the newly-created instance.
    pub slug: String,
    /// Display name of the instance (from the pack manifest or `name_override`).
    pub name: String,
    /// Number of pack files that were downloaded successfully.
    pub installed: u32,
    /// Number of pack files whose download failed.
    pub failed: u32,
    /// Number of pack files skipped because they are client-unsupported.
    pub skipped: u32,
    /// File paths of any downloads that failed (empty on full success).
    pub failed_files: Vec<String>,
}

// ---------------------------------------------------------------------------
// CP-3: Pack job helpers
// ---------------------------------------------------------------------------

/// Staging directory name for a task: `<inst_dir>/.staging-<task_id>`.
/// The staging dir is a sibling of the instance dir, so both reside on the
/// same volume — rename is guaranteed atomic.
fn staging_dir_for(inst_dir: &std::path::Path, task_id: u64) -> std::path::PathBuf {
    inst_dir.join(format!(".staging-{task_id}"))
}

/// Tally outcomes from a cancellable plan run (installed / failed / failed_files).
fn tally_cancellable(
    outcomes: &[core::download::CancellableOutcome],
) -> (u32, u32, Vec<String>) {
    let mut installed = 0u32;
    let mut failed = 0u32;
    let mut failed_files: Vec<String> = Vec::new();
    for o in outcomes {
        match &o.status {
            CancellableStatus::Ran(ItemStatus::Ok | ItemStatus::Skipped) => installed += 1,
            CancellableStatus::Ran(ItemStatus::Failed { .. }) => {
                failed += 1;
                failed_files.push(o.url.clone());
            }
            CancellableStatus::Cancelled => {} // not counted; run was cancelled
        }
    }
    (installed, failed, failed_files)
}

/// Build a [`ChildItem`] list from plan items (basename of each dest path).
fn children_from_items(items: &[core::download::DownloadItem]) -> Vec<ChildItem> {
    items
        .iter()
        .map(|item| ChildItem {
            label: item
                .dest
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| item.url.clone()),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// CP-3: ImportMrpackJob
// ---------------------------------------------------------------------------

/// [`TaskJob`] that imports a local `.mrpack` into a new instance.
///
/// Mirrors the old `import_mrpack_from_bytes` flow but:
/// - Downloads into a staging dir (`<inst_dir>/.staging-<task_id>/`).
/// - Uses [`execute_plan_cancellable`] with the task's cancel token.
/// - Promotes staging on success; discards on cancel/failure.
/// - Emits lifecycle via [`TaskContext`]; terminal result = [`MrpackImportResult`].
///
/// `task_id_cell` carries the assigned task id (written by the command after
/// `enqueue()` returns, read by `run()` before first use). See
/// `enqueue_import_mrpack` for the ordering guarantee.
struct ImportMrpackJob {
    app: tauri::AppHandle,
    task_id_cell: std::sync::Arc<std::sync::atomic::AtomicU64>,
    bytes: Vec<u8>,
    name_override: Option<String>,
    pack_source: Option<instances::Source>,
}

#[async_trait::async_trait]
impl TaskJob for ImportMrpackJob {
    async fn run(self: Box<Self>, ctx: &core::task_manager::TaskContext) {
        use core::modpack::{extract_overrides, promote_staging, remap_to_staging};
        use std::sync::atomic::Ordering;

        let task_id = self.task_id_cell.load(Ordering::SeqCst);
        log::info!("modpack: ImportMrpackJob task_id={task_id} starting");

        // --- PLAN ---
        ctx.enter_planning().await;

        // Parse manifest (minimal) for instance creation.
        let pre_manifest = {
            use std::io::{Cursor, Read as _};
            let r: Result<_, String> = (|| {
                let mut archive = zip::ZipArchive::new(Cursor::new(&self.bytes))
                    .map_err(|e| format!("could not open mrpack zip: {e}"))?;
                let mut entry = archive
                    .by_name("modrinth.index.json")
                    .map_err(|_| "modrinth.index.json not found".to_string())?;
                let mut buf = String::new();
                entry.read_to_string(&mut buf).map_err(|e| e.to_string())?;
                modpack::parse_modrinth_index(&buf).map_err(|e| e.to_string())
            })();
            match r {
                Ok(m) => m,
                Err(e) => {
                    ctx.finish_failed(e).await;
                    return;
                }
            }
        };

        let instance_name = self
            .name_override
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| pre_manifest.name.clone());

        let inst = match instances::create(
            &self.app,
            CreateInstanceReq {
                name: instance_name,
                minecraft: pre_manifest.minecraft.clone(),
                loader: instances::Loader {
                    kind: pre_manifest.loader.kind.clone(),
                    version: pre_manifest.loader.version.clone(),
                },
            },
        ) {
            Ok(i) => i,
            Err(e) => {
                ctx.finish_failed(e).await;
                return;
            }
        };

        let inst_dir = match core::store::instances_dir(&self.app) {
            Ok(d) => d.join(&inst.slug),
            Err(e) => { ctx.finish_failed(e).await; return; }
        };
        let mc_dir = inst_dir.join("mc");
        let staging_dir = staging_dir_for(&inst_dir, task_id);

        // Parse + build plan (items still target mc_dir at this stage).
        let (plan_result, skipped_count, mod_entries) = match modpack::read_mrpack(&self.bytes, &mc_dir) {
            Ok((_manifest, plan)) => {
                let skipped = plan.skipped.len() as u32;
                let mods = plan.mods.clone();
                (plan.items, skipped, mods)
            }
            Err(e) => {
                ctx.finish_failed(e.to_string()).await;
                return;
            }
        };

        // Remap destinations to staging dir.
        let staged_items = remap_to_staging(plan_result, &mc_dir, &staging_dir);

        // Build child labels from the (remapped) items.
        let children = children_from_items(&staged_items);

        // --- DOWNLOAD ---
        let staged_plan = DownloadPlan::new(staged_items);
        ctx.enter_downloading(children).await;

        let http_client = match download::build_client() {
            Ok(c) => c,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&staging_dir);
                ctx.finish_failed(e.to_string()).await;
                return;
            }
        };

        let dl_result = execute_plan_cancellable(
            &http_client,
            &staged_plan,
            &core::download::NoOpSink,
            8,
            ctx.cancel_token(),
        )
        .await;

        if dl_result.cancelled || ctx.is_cancelled() {
            let _ = std::fs::remove_dir_all(&staging_dir);
            ctx.finish_cancelled().await;
            return;
        }

        let (installed, failed, failed_files) = tally_cancellable(&dl_result.outcomes);

        // --- APPLY ---
        ctx.enter_applying().await;

        // Promote staging → mc_dir.
        if let Err(e) = promote_staging(&staging_dir, &mc_dir) {
            let _ = std::fs::remove_dir_all(&staging_dir);
            ctx.finish_failed(format!("promote failed: {e}")).await;
            return;
        }
        let _ = std::fs::remove_dir_all(&staging_dir);

        // Extract overrides directly into mc_dir (post-promote).
        {
            use std::io::Cursor;
            if let Err(e) = zip::ZipArchive::new(Cursor::new(&self.bytes))
                .map_err(|e| format!("could not re-open mrpack zip for overrides: {e}"))
                .and_then(|mut archive| {
                    extract_overrides(&mut archive, &mc_dir).map_err(|e| e.to_string())
                })
            {
                ctx.finish_failed(e).await;
                return;
            }
        }

        // Merge mod entries + persist manifest.
        let result = match instances::load_manifest(&self.app, &inst.slug) {
            Ok(mut instance) => {
                instance.mods = mod_entries;
                if let Some(src) = self.pack_source {
                    instance.source = Some(src);
                }
                match instances::save_manifest(&self.app, &inst.slug, &instance) {
                    Ok(_) => Ok(MrpackImportResult {
                        slug: inst.slug,
                        name: inst.name,
                        installed,
                        failed,
                        skipped: skipped_count,
                        failed_files,
                    }),
                    Err(e) => Err(e),
                }
            }
            Err(e) => Err(e),
        };

        match result {
            Ok(r) => {
                log::info!(
                    "modpack: ImportMrpackJob done — slug='{}' installed={} failed={}",
                    r.slug, r.installed, r.failed
                );
                match serde_json::to_value(&r) {
                    Ok(v) => ctx.finish_done_with_result(v).await,
                    Err(e) => ctx.finish_failed(format!("serialize result: {e}")).await,
                }
            }
            Err(e) => ctx.finish_failed(e).await,
        }
    }
}

/// Enqueue an `.mrpack` import task and return its id synchronously.
///
/// The task runs Plan → Download (staged) → Apply (promote + overrides + manifest).
/// The terminal `task://update` event carries the [`MrpackImportResult`] JSON.
#[tauri::command]
#[specta::specta]
async fn import_mrpack(
    app: tauri::AppHandle,
    manager: tauri::State<'_, TaskManager>,
    mrpack_path: String,
    name_override: Option<String>,
) -> Result<u64, String> {
    let bytes = std::fs::read(&mrpack_path)
        .map_err(|e| format!("could not read mrpack file '{mrpack_path}': {e}"))?;
    enqueue_import_mrpack(&app, &manager, bytes, name_override, None).await
}

/// Inner fn shared by `import_mrpack` and `install_modpack` (mrpack branch).
///
/// The `task_id_cell` pattern: we need the task id inside the job for staging
/// dir naming. The id is only known after `enqueue()` returns, but the job
/// struct must be moved into the spec before enqueue. Solution: share an
/// `Arc<AtomicU64>` between command and job; command writes the id immediately
/// after enqueue returns; the job reads it at first use (inside `run()`, which
/// the worker calls only after the enqueue `send` completes on the channel —
/// the Tokio `unbounded_channel` guarantees the send-before-recv ordering, so
/// the job always reads a non-zero id).
async fn enqueue_import_mrpack(
    app: &tauri::AppHandle,
    manager: &tauri::State<'_, TaskManager>,
    bytes: Vec<u8>,
    name_override: Option<String>,
    pack_source: Option<instances::Source>,
) -> Result<u64, String> {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    // Parse pack name for the task label (best-effort; fall back to generic label).
    let pack_name = {
        use std::io::{Cursor, Read as _};
        (|| {
            let mut archive = zip::ZipArchive::new(Cursor::new(&bytes)).ok()?;
            let mut entry = archive.by_name("modrinth.index.json").ok()?;
            let mut buf = String::new();
            entry.read_to_string(&mut buf).ok()?;
            modpack::parse_modrinth_index(&buf).ok().map(|m| m.name)
        })()
        .unwrap_or_else(|| "Modpack".to_string())
    };

    let task_id_cell: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
    let task_id_cell_job = Arc::clone(&task_id_cell);

    let id = manager.enqueue(TaskSpec {
        kind: TaskKind::PackInstall,
        parent_label: format!("Install {pack_name}"),
        job: Box::new(ImportMrpackJob {
            app: app.clone(),
            task_id_cell: task_id_cell_job,
            bytes,
            name_override,
            pack_source,
        }),
    }).await;

    // Write the assigned id into the cell so the job can read it.
    // This happens before the worker's `recv()` yields the message to run the job.
    task_id_cell.store(id, Ordering::SeqCst);
    Ok(id)
}

// ---------------------------------------------------------------------------
// CP B4: CurseForge `.zip` modpack import
// ---------------------------------------------------------------------------

/// Result returned to the frontend after a successful CurseForge `.zip` import.
#[derive(Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CfImportResult {
    /// Slug of the newly-created instance.
    pub slug: String,
    /// Display name of the instance (from the pack manifest or `name_override`).
    pub name: String,
    /// Number of pack files that were downloaded successfully.
    pub installed: u32,
    /// Number of pack files whose download failed.
    pub failed: u32,
    /// Files the user must download manually (distribution-disabled, or
    /// resolved without a usable hash).
    pub manual: Vec<core::modpack::CfManualFile>,
}

// ---------------------------------------------------------------------------
// CP-3: ImportCfZipJob
// ---------------------------------------------------------------------------

/// [`TaskJob`] that imports a CurseForge `.zip` modpack into a new instance.
///
/// Same stage-and-promote pattern as [`ImportMrpackJob`]:
/// - PLAN: parse manifest + resolve files via CF API → build child queue.
/// - DOWNLOAD: execute into staging dir with cancel support.
/// - APPLY: promote staging → mc_dir, extract overrides, save manifest.
struct ImportCfZipJob {
    app: tauri::AppHandle,
    task_id_cell: std::sync::Arc<std::sync::atomic::AtomicU64>,
    bytes: Vec<u8>,
    name_override: Option<String>,
    pack_source: Option<instances::Source>,
}

#[async_trait::async_trait]
impl TaskJob for ImportCfZipJob {
    async fn run(self: Box<Self>, ctx: &core::task_manager::TaskContext) {
        use core::modpack::{
            extract_overrides, promote_staging, remap_to_staging,
            read_cf_manifest, resolve_and_build_cf_plan,
        };
        use std::sync::atomic::Ordering;

        let task_id = self.task_id_cell.load(Ordering::SeqCst);
        log::info!("modpack: ImportCfZipJob task_id={task_id} starting");

        // --- PLAN ---
        ctx.enter_planning().await;

        // Parse manifest.json.
        let manifest = match read_cf_manifest(&self.bytes) {
            Ok(m) => m,
            Err(e) => { ctx.finish_failed(e.to_string()).await; return; }
        };

        let instance_name = self
            .name_override
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| manifest.name.clone());

        let inst = match instances::create(
            &self.app,
            CreateInstanceReq {
                name: instance_name,
                minecraft: manifest.minecraft.clone(),
                loader: instances::Loader {
                    kind: manifest.loader.kind.clone(),
                    version: manifest.loader.version.clone(),
                },
            },
        ) {
            Ok(i) => i,
            Err(e) => { ctx.finish_failed(e).await; return; }
        };

        let inst_dir = match core::store::instances_dir(&self.app) {
            Ok(d) => d.join(&inst.slug),
            Err(e) => { ctx.finish_failed(e).await; return; }
        };
        let mc_dir = inst_dir.join("mc");
        let staging_dir = staging_dir_for(&inst_dir, task_id);

        // Resolve CF files + build plan.
        let settings = settings::load(&self.app).unwrap_or_default();
        let cf_key = cf_api_key_from(
            std::env::var(providers::CF_API_KEY_ENV).ok(),
            settings.curseforge_api_key,
            option_env!("MODLOADER_CF_API_KEY").map(str::to_string),
        );
        let provider = CurseForgeProvider::new(cf_key);
        let http = ReqwestProviderClient(reqwest::Client::new());

        let plan = match resolve_and_build_cf_plan(&provider, &http, &manifest, &mc_dir).await {
            Ok(p) => p,
            Err(e) => { ctx.finish_failed(e.to_string()).await; return; }
        };

        let manual = plan.manual.clone();
        let mod_entries = plan.mods.clone();
        let resolve_failed_count = plan.failed.len() as u32;

        // Remap to staging.
        let staged_items = remap_to_staging(plan.items, &mc_dir, &staging_dir);
        let children = children_from_items(&staged_items);

        // --- DOWNLOAD ---
        let staged_plan = DownloadPlan::new(staged_items);
        ctx.enter_downloading(children).await;

        let http_client = match download::build_client() {
            Ok(c) => c,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&staging_dir);
                ctx.finish_failed(e.to_string()).await;
                return;
            }
        };

        let dl_result = execute_plan_cancellable(
            &http_client,
            &staged_plan,
            &core::download::NoOpSink,
            8,
            ctx.cancel_token(),
        )
        .await;

        if dl_result.cancelled || ctx.is_cancelled() {
            let _ = std::fs::remove_dir_all(&staging_dir);
            ctx.finish_cancelled().await;
            return;
        }

        let (installed, dl_failed, _) = tally_cancellable(&dl_result.outcomes);
        let failed = resolve_failed_count + dl_failed;

        // --- APPLY ---
        ctx.enter_applying().await;

        if let Err(e) = promote_staging(&staging_dir, &mc_dir) {
            let _ = std::fs::remove_dir_all(&staging_dir);
            ctx.finish_failed(format!("promote failed: {e}")).await;
            return;
        }
        let _ = std::fs::remove_dir_all(&staging_dir);

        {
            use std::io::Cursor;
            if let Err(e) = zip::ZipArchive::new(Cursor::new(&self.bytes))
                .map_err(|e| format!("could not re-open CF zip for overrides: {e}"))
                .and_then(|mut archive| {
                    extract_overrides(&mut archive, &mc_dir).map_err(|e| e.to_string())
                })
            {
                ctx.finish_failed(e).await;
                return;
            }
        }

        let result = match instances::load_manifest(&self.app, &inst.slug) {
            Ok(mut instance) => {
                instance.mods = mod_entries;
                if let Some(src) = self.pack_source {
                    instance.source = Some(src);
                }
                match instances::save_manifest(&self.app, &inst.slug, &instance) {
                    Ok(_) => Ok(CfImportResult {
                        slug: inst.slug,
                        name: inst.name,
                        installed,
                        failed,
                        manual,
                    }),
                    Err(e) => Err(e),
                }
            }
            Err(e) => Err(e),
        };

        match result {
            Ok(r) => {
                log::info!(
                    "modpack: ImportCfZipJob done — slug='{}' installed={} failed={}",
                    r.slug, r.installed, r.failed
                );
                match serde_json::to_value(&r) {
                    Ok(v) => ctx.finish_done_with_result(v).await,
                    Err(e) => ctx.finish_failed(format!("serialize result: {e}")).await,
                }
            }
            Err(e) => ctx.finish_failed(e).await,
        }
    }
}

/// Inner fn shared by `import_curseforge_zip` and `install_modpack` (CF branch).
async fn enqueue_import_cf_zip(
    app: &tauri::AppHandle,
    manager: &tauri::State<'_, TaskManager>,
    bytes: Vec<u8>,
    name_override: Option<String>,
    pack_source: Option<instances::Source>,
) -> Result<u64, String> {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    let pack_name = modpack::read_cf_manifest(&bytes)
        .map(|m| m.name)
        .unwrap_or_else(|_| "Modpack".to_string());

    let task_id_cell: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
    let task_id_cell_job = Arc::clone(&task_id_cell);

    let id = manager.enqueue(TaskSpec {
        kind: TaskKind::PackInstall,
        parent_label: format!("Install {pack_name}"),
        job: Box::new(ImportCfZipJob {
            app: app.clone(),
            task_id_cell: task_id_cell_job,
            bytes,
            name_override,
            pack_source,
        }),
    }).await;

    task_id_cell.store(id, Ordering::SeqCst);
    Ok(id)
}

/// Enqueue a CurseForge `.zip` import task and return its id synchronously.
#[tauri::command]
#[specta::specta]
async fn import_curseforge_zip(
    app: tauri::AppHandle,
    manager: tauri::State<'_, TaskManager>,
    cf_zip_path: String,
    name_override: Option<String>,
) -> Result<u64, String> {
    let bytes = std::fs::read(&cf_zip_path)
        .map_err(|e| format!("could not read CurseForge zip '{cf_zip_path}': {e}"))?;
    enqueue_import_cf_zip(&app, &manager, bytes, name_override, None).await
}

// ---------------------------------------------------------------------------
// C3: Browse → one-click install
// ---------------------------------------------------------------------------

/// Tagged result returned to the frontend by `install_modpack`.
///
/// The `kind` field discriminates which branch occurred so the frontend can
/// render the correct toast without re-deriving the provider.
///
/// - `"mrpack"` — Modrinth pack installed successfully.
/// - `"curseforge"` — CurseForge pack installed; may carry a `manual` list for
///   distribution-disabled files.
/// - `"manual"` — the pack's primary file has `url: None` (distribution
///   disabled at the pack level); no instance was created. The frontend should
///   open `page_url` in the browser.
#[derive(Debug, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ModpackInstallResult {
    /// Modrinth `.mrpack` installed successfully.
    Mrpack(MrpackImportResult),
    /// CurseForge `.zip` installed (manual list may be non-empty).
    Curseforge(CfImportResult),
    /// Pack file is not distributable; frontend must open `page_url`.
    Manual {
        /// Provider project page URL (echoed from the `page_url` parameter).
        #[serde(rename = "pageUrl")]
        page_url: String,
        /// Filename as returned by the provider (useful for display).
        #[serde(rename = "fileName")]
        file_name: String,
    },
}

/// Install a modpack from a Browse `ProjectSummary` in one click.
///
/// Returns the enqueued task id. The terminal `task://update` event carries the
/// [`ModpackInstallResult`] JSON (Mrpack or Curseforge variant).
///
/// Special case: if the resolved pack file has `url: None` (CF distribution-
/// disabled), the command returns `Err("MANUAL:<json>")` immediately — no task
/// is created. The JSON payload encodes `{ "pageUrl": "...", "fileName": "..." }`
/// so CP-7 can detect and route the manual case without a task. This preserves
/// the invariant that `Ok(u64)` always means "a task was enqueued."
#[tauri::command]
#[specta::specta]
async fn install_modpack(
    app: tauri::AppHandle,
    manager: tauri::State<'_, TaskManager>,
    provider: String,
    project_id: String,
    page_url: Option<String>,
    version_id: Option<String>,
) -> Result<u64, String> {
    use core::modpack::resolve_pack_file;
    use core::providers::ProviderKind;

    log::info!("modpack: install_modpack provider={provider} project_id={project_id}");

    // 1. Resolve the pack file (network call — must happen before enqueue so we
    //    can detect the Manual case synchronously).
    let settings = settings::load(&app).unwrap_or_default();
    let target_ver = version_id.as_deref();
    let resolved = match provider.as_str() {
        "modrinth" => {
            let prov = ModrinthProvider;
            let http = ReqwestProviderClient(reqwest::Client::new());
            resolve_pack_file(&prov, &http, &project_id, target_ver)
                .await
                .map_err(|e| e.to_string())?
        }
        "curseForge" | "curseforge" => {
            let cf_key = cf_api_key_from(
                std::env::var(providers::CF_API_KEY_ENV).ok(),
                settings.curseforge_api_key,
                option_env!("MODLOADER_CF_API_KEY").map(str::to_string),
            );
            let prov = CurseForgeProvider::new(cf_key);
            let http = ReqwestProviderClient(reqwest::Client::new());
            resolve_pack_file(&prov, &http, &project_id, target_ver)
                .await
                .map_err(|e| e.to_string())?
        }
        other => {
            return Err(format!("unknown provider: {other}"));
        }
    };

    // 2. If no URL → manual case: no instance, no task. Encode as structured error
    //    so CP-7 can detect it and open page_url in the browser.
    let url = match resolved.url {
        Some(u) => u,
        None => {
            log::warn!(
                "modpack: install_modpack — pack file '{}' is not distributable; routing to manual",
                resolved.file_name
            );
            let manual_json = serde_json::json!({
                "pageUrl": page_url.unwrap_or_default(),
                "fileName": resolved.file_name,
            });
            return Err(format!("MANUAL:{manual_json}"));
        }
    };

    // 3. Download the archive to cache/installers/<file_name> synchronously
    //    (small; determines provider kind for job dispatch).
    let installers_dir = core::store::cache_installers_dir(&app)?;
    let dest_path = installers_dir.join(&resolved.file_name);
    let response = reqwest::Client::new()
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("could not fetch pack archive: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "pack archive download failed: HTTP {}",
            response.status()
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("could not read pack archive bytes: {e}"))?
        .to_vec();
    std::fs::write(&dest_path, &bytes)
        .map_err(|e| format!("could not write pack archive to cache: {e}"))?;

    // 4. Build provenance record.
    let provider_str = match resolved.provider {
        ProviderKind::Modrinth => "modrinth".to_string(),
        ProviderKind::CurseForge => "curseForge".to_string(),
    };
    let source = instances::Source {
        provider: provider_str,
        project_id: project_id.clone(),
        file_id: resolved.version_id.clone(),
        pack_version: resolved.version_name.clone(),
        recommended: None,
    };
    let name_override: Option<String> = if resolved.version_name.is_empty() {
        None
    } else {
        Some(resolved.version_name.clone())
    };

    // 5. Enqueue the correct job.
    match resolved.provider {
        ProviderKind::Modrinth => {
            enqueue_import_mrpack(&app, &manager, bytes, name_override, Some(source)).await
        }
        ProviderKind::CurseForge => {
            enqueue_import_cf_zip(&app, &manager, bytes, name_override, Some(source)).await
        }
    }
}

// ---------------------------------------------------------------------------
// D3: pack update command
// ---------------------------------------------------------------------------

/// Result returned to the frontend by `update_modpack`.
///
/// One struct for both providers (mrpack and CurseForge). `manual` is empty for
/// mrpack updates and carries distribution-disabled files for CF updates, reusing
/// the `CfManualFile` shape from slice B.
#[derive(Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PackUpdateResult {
    /// Number of pack mods newly added (file_name absent from the old pack mods).
    pub added: u32,
    /// Number of old pack mods removed (were `from_pack=true`, absent from new plan).
    pub removed: u32,
    /// Number of user-added mods kept (from_pack=false, not overwritten by new plan).
    pub kept: u32,
    /// Number of download failures (+ CF resolve failures for CF packs).
    pub failed: u32,
    /// Files the user must download manually (CF only; empty for mrpack).
    pub manual: Vec<core::modpack::CfManualFile>,
}

// ---------------------------------------------------------------------------
// CP-3: UpdateModpackJob
// ---------------------------------------------------------------------------

/// [`TaskJob`] that updates an existing modpack instance to a new version.
///
/// Stage-and-promote pattern identical to the install jobs:
/// - PLAN: load instance, resolve target version + download archive, build plan.
/// - DOWNLOAD: execute into staging with cancel support.
/// - APPLY: delete removed mods, promote staging, extract overrides, save manifest.
struct UpdateModpackJob {
    app: tauri::AppHandle,
    task_id_cell: std::sync::Arc<std::sync::atomic::AtomicU64>,
    slug: String,
    // Pre-fetched at enqueue time (network call in command before enqueue).
    bytes: Vec<u8>,
    resolved_version_id: String,
    resolved_version_name: String,
    resolved_provider: core::providers::ProviderKind,
}

#[async_trait::async_trait]
impl TaskJob for UpdateModpackJob {
    async fn run(self: Box<Self>, ctx: &core::task_manager::TaskContext) {
        use core::modpack::{
            extract_overrides, plan_pack_update, promote_staging, remap_to_staging,
            resolve_and_build_cf_plan, read_cf_manifest, read_mrpack,
        };
        use core::providers::ProviderKind;
        use std::sync::atomic::Ordering;

        let task_id = self.task_id_cell.load(Ordering::SeqCst);
        log::info!("modpack: UpdateModpackJob task_id={task_id} instance={}", self.slug);

        // --- PLAN ---
        ctx.enter_planning().await;

        // Load instance + require source.
        let mut instance = match instances::load_manifest(&self.app, &self.slug) {
            Ok(i) => i,
            Err(e) => { ctx.finish_failed(e).await; return; }
        };
        if instance.source.is_none() {
            ctx.finish_failed(
                "instance has no pack source — not updatable (installed locally)".to_string()
            ).await;
            return;
        }

        let inst_dir = match core::store::instances_dir(&self.app) {
            Ok(d) => d.join(&self.slug),
            Err(e) => { ctx.finish_failed(e).await; return; }
        };
        let mc_dir = inst_dir.join("mc");
        let staging_dir = staging_dir_for(&inst_dir, task_id);

        // Build new plan from the pre-fetched bytes.
        let settings = settings::load(&self.app).unwrap_or_default();
        let (new_mod_entries, download_items, manual, resolve_failed_count) =
            match self.resolved_provider {
                ProviderKind::Modrinth => {
                    match read_mrpack(&self.bytes, &mc_dir) {
                        Ok((_manifest, plan)) => (plan.mods, plan.items, vec![], 0u32),
                        Err(e) => { ctx.finish_failed(e.to_string()).await; return; }
                    }
                }
                ProviderKind::CurseForge => {
                    let cf_key = cf_api_key_from(
                        std::env::var(providers::CF_API_KEY_ENV).ok(),
                        settings.curseforge_api_key.clone(),
                        option_env!("MODLOADER_CF_API_KEY").map(str::to_string),
                    );
                    let manifest = match read_cf_manifest(&self.bytes) {
                        Ok(m) => m,
                        Err(e) => { ctx.finish_failed(e.to_string()).await; return; }
                    };
                    let provider = CurseForgeProvider::new(cf_key);
                    let http = ReqwestProviderClient(reqwest::Client::new());
                    match resolve_and_build_cf_plan(&provider, &http, &manifest, &mc_dir).await {
                        Ok(plan) => {
                            let failed_count = plan.failed.len() as u32;
                            (plan.mods, plan.items, plan.manual, failed_count)
                        }
                        Err(e) => { ctx.finish_failed(e.to_string()).await; return; }
                    }
                }
            };

        // Reconcile mods.
        let reconcile = plan_pack_update(&instance.mods, &new_mod_entries);

        // Counts (computed before consuming reconcile).
        let removed_count = reconcile.to_remove.len() as u32;
        let kept_count = reconcile.merged.iter().filter(|m| !m.from_pack).count() as u32;
        let old_pack_names: std::collections::HashSet<&str> = instance
            .mods
            .iter()
            .filter(|m| m.from_pack)
            .map(|m| m.file_name.as_str())
            .collect();
        let added_count = reconcile
            .merged
            .iter()
            .filter(|m| m.from_pack && !old_pack_names.contains(m.file_name.as_str()))
            .count() as u32;

        // Delete removed pack jars immediately (before downloading; removal is safe
        // even on cancel because they were already superseded by the new plan).
        let mods_dir = mc_dir.join("mods");
        for entry in &reconcile.to_remove {
            instances::remove_mod_from_disk_files(&mods_dir, &entry.file_name);
        }

        // Remap to staging.
        let staged_items = remap_to_staging(download_items, &mc_dir, &staging_dir);
        let children = children_from_items(&staged_items);

        // --- DOWNLOAD ---
        let staged_plan = DownloadPlan::new(staged_items);
        ctx.enter_downloading(children).await;

        let http_client = match download::build_client() {
            Ok(c) => c,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&staging_dir);
                ctx.finish_failed(e.to_string()).await;
                return;
            }
        };

        let dl_result = execute_plan_cancellable(
            &http_client,
            &staged_plan,
            &core::download::NoOpSink,
            8,
            ctx.cancel_token(),
        )
        .await;

        if dl_result.cancelled || ctx.is_cancelled() {
            let _ = std::fs::remove_dir_all(&staging_dir);
            ctx.finish_cancelled().await;
            return;
        }

        let (_, dl_failed, _) = tally_cancellable(&dl_result.outcomes);
        let download_failed = resolve_failed_count + dl_failed;

        // --- APPLY ---
        ctx.enter_applying().await;

        if let Err(e) = promote_staging(&staging_dir, &mc_dir) {
            let _ = std::fs::remove_dir_all(&staging_dir);
            ctx.finish_failed(format!("promote failed: {e}")).await;
            return;
        }
        let _ = std::fs::remove_dir_all(&staging_dir);

        // Extract overrides (pack wins on config/options collisions).
        {
            use std::io::Cursor;
            if let Err(e) = zip::ZipArchive::new(Cursor::new(&self.bytes))
                .map_err(|e| format!("could not re-open pack archive for overrides: {e}"))
                .and_then(|mut archive| {
                    extract_overrides(&mut archive, &mc_dir).map_err(|e| e.to_string())
                })
            {
                ctx.finish_failed(e).await;
                return;
            }
        }

        // Write updated manifest.
        instance.mods = reconcile.merged;
        {
            let src = instance.source.as_mut()
                .expect("source must be Some — checked at plan step");
            src.file_id = self.resolved_version_id.clone();
            src.pack_version = self.resolved_version_name.clone();
        }
        if let Err(e) = instances::save_manifest(&self.app, &self.slug, &instance) {
            ctx.finish_failed(e).await;
            return;
        }

        let result = PackUpdateResult {
            added: added_count,
            removed: removed_count,
            kept: kept_count,
            failed: download_failed,
            manual,
        };

        log::info!(
            "modpack: UpdateModpackJob done — added={} removed={} kept={} failed={}",
            result.added, result.removed, result.kept, result.failed
        );

        match serde_json::to_value(&result) {
            Ok(v) => ctx.finish_done_with_result(v).await,
            Err(e) => ctx.finish_failed(format!("serialize result: {e}")).await,
        }
    }
}

/// Update an existing modpack instance. Resolves + downloads the archive
/// synchronously, then enqueues the job and returns the task id.
///
/// The network resolution + archive download happen in the command body
/// (before enqueue) so that errors surface immediately. The heavy FS work
/// (plan execution, promote, overrides) runs in the task.
#[tauri::command]
#[specta::specta]
async fn update_modpack(
    app: tauri::AppHandle,
    manager: tauri::State<'_, TaskManager>,
    slug: String,
    version_id: Option<String>,
) -> Result<u64, String> {
    use core::modpack::resolve_pack_file;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    log::info!("modpack: update_modpack enqueue instance={slug}");

    // Load instance to get provider info (needed for resolve).
    let instance = instances::load_manifest(&app, &slug)?;
    let source = instance
        .source
        .ok_or_else(|| {
            "instance has no pack source — not updatable (installed locally)".to_string()
        })?;

    let settings = settings::load(&app).unwrap_or_default();
    let target_ver = version_id.as_deref();

    let resolved = match source.provider.as_str() {
        "modrinth" => {
            let prov = ModrinthProvider;
            let http = ReqwestProviderClient(reqwest::Client::new());
            resolve_pack_file(&prov, &http, &source.project_id, target_ver)
                .await
                .map_err(|e| e.to_string())?
        }
        "curseForge" | "curseforge" => {
            let cf_key = cf_api_key_from(
                std::env::var(providers::CF_API_KEY_ENV).ok(),
                settings.curseforge_api_key.clone(),
                option_env!("MODLOADER_CF_API_KEY").map(str::to_string),
            );
            let prov = CurseForgeProvider::new(cf_key);
            let http = ReqwestProviderClient(reqwest::Client::new());
            resolve_pack_file(&prov, &http, &source.project_id, target_ver)
                .await
                .map_err(|e| e.to_string())?
        }
        other => {
            return Err(format!("unknown provider in instance source: {other}"));
        }
    };

    let url = resolved.url.ok_or_else(|| {
        "pack archive is not distributable — cannot update automatically".to_string()
    })?;

    // Download the archive to cache/installers.
    let installers_dir = core::store::cache_installers_dir(&app)?;
    let dest_path = installers_dir.join(&resolved.file_name);
    let response = reqwest::Client::new()
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("could not fetch pack archive: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "pack archive download failed: HTTP {}",
            response.status()
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("could not read pack archive bytes: {e}"))?
        .to_vec();
    std::fs::write(&dest_path, &bytes)
        .map_err(|e| format!("could not write pack archive to cache: {e}"))?;

    let task_id_cell: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
    let task_id_cell_job = Arc::clone(&task_id_cell);

    let id = manager.enqueue(TaskSpec {
        kind: TaskKind::PackUpdate,
        parent_label: format!("Update {slug}"),
        job: Box::new(UpdateModpackJob {
            app: app.clone(),
            task_id_cell: task_id_cell_job,
            slug: slug.clone(),
            bytes,
            resolved_version_id: resolved.version_id,
            resolved_version_name: resolved.version_name,
            resolved_provider: resolved.provider,
        }),
    }).await;

    task_id_cell.store(id, Ordering::SeqCst);
    Ok(id)
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;

// Export test — gated out on Windows because constructing the command surface
// creates function pointers to Tauri commands which pull in WebView2 GUI DLLs,
// crashing the Windows test binary (STATUS_ENTRYPOINT_NOT_FOUND). On Linux (with
// GTK/WebKit installed, e.g. future CI), the test runs natively via
// `cargo test --manifest-path src-tauri/Cargo.toml export_bindings` and exports
// through the SAME `make_builder()` the app uses. On Windows, regenerate
// bindings.ts by running `scripts/build.sh dev` once (the `#[cfg(debug_assertions)]`
// block in `run()` writes the file at startup via that same builder).
#[cfg(all(test, not(target_os = "windows")))]
#[path = "bindings_export_tests.rs"]
mod bindings_export_tests;

/// Typed union of all terminal task results.
///
/// Carried by `Task.result` (via specta type override) and surfaced in the
/// generated `bindings.ts` so the frontend can read `task.result` as a typed
/// union rather than `any`. The runtime field in `Task` remains
/// `Option<serde_json::Value>` (see `task_manager.rs`) and `finish_done_with_result`
/// continues to accept `serde_json::Value`; this enum drives the TS type only.
///
/// Wire shapes of the 6 variants match what `serde_json::to_value(&result)` writes
/// for each corresponding result struct — no format change.
#[derive(Debug, Serialize, Deserialize, specta::Type)]
#[serde(untagged)]
pub enum TaskResult {
    /// Result of `import_mrpack` / `install_modpack` (mrpack branch).
    MrpackImport(MrpackImportResult),
    /// Result of `import_curseforge_zip` / `install_modpack` (CF branch).
    CfImport(CfImportResult),
    /// Result of `install_modpack` (typed tagged union with `kind` field).
    ModpackInstall(ModpackInstallResult),
    /// Result of `update_modpack`.
    PackUpdate(PackUpdateResult),
    /// Result of `add_mod`.
    AddMod(AddModResult),
    /// Result of `update_mod`.
    UpdateMod(UpdateModResult),
}

/// Construct the tauri-specta Builder with all commands registered.
///
/// The single source-of-truth for the command surface. Every consumer routes
/// through this one list: the invoke handler wired into the real Tauri app
/// (`run()`), the debug-startup type export, and the Linux export test. There is
/// exactly one `collect_commands!` — adding a command here updates all three, so
/// the registered surface and the exported surface cannot drift.
pub(crate) fn make_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .dangerously_cast_bigints_to_number()
        .commands(collect_commands![
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
            kill_instance,
            list_running,
            get_run_state,
            get_run_logs,
            list_tasks,
            cancel_task,
            begin_login,
            cancel_login,
            get_account,
            logout,
            search_mods,
            get_mod_versions,
            get_pack_info,
            add_mod,
            set_mod_enabled,
            remove_mod,
            update_mod,
            set_pack_lock,
            set_instance_java,
            validate_java_path,
            import_mrpack,
            import_curseforge_zip,
            install_modpack,
            update_modpack,
        ])
        .events(collect_events![
            DeviceCodePayload,
            ProgressPayload,
            LaunchLogPayload,
            LaunchExitPayload,
            RunUpdatePayload,
            TaskProgressPayload,
            TaskUpdatePayload,
            InstallLogPayload,
        ])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // The running instance registry is the first managed state in this app.
    // Managed as Arc<RunningRegistry> so the monitor background tasks can clone
    // the Arc and still access the shared map after the command returns.
    let registry: Arc<RunningRegistry> = Arc::new(launch::new_running_registry());

    // Single-permit semaphore serializing the blocking launch prep across runs.
    let prep_semaphore: launch::PrepSemaphore = launch::new_prep_semaphore();

    // Cancel token for in-flight begin_login.
    let cancel_token: CancelToken = std::sync::Mutex::new(None);

    let builder = make_builder();

    // In debug builds, export `src/lib/bindings.ts` at startup so the generated
    // file stays current as commands and types evolve. This is the standard
    // tauri-specta regeneration pattern for Windows (where the export test cannot
    // run due to WebView2 DLL linking in the test binary). Run `scripts/build.sh
    // dev`, wait for the app to start, then Ctrl-C; the file is written synchronously
    // before the webview loads. The path is relative to the CWD at app launch,
    // which is the project root when invoked via `tauri dev`.
    #[cfg(debug_assertions)]
    {
        if let Err(e) = builder.export(
            specta_typescript::Typescript::default(),
            "../src/lib/bindings.ts",
        ) {
            eprintln!("[bindings] failed to export bindings.ts: {e}");
        } else {
            eprintln!("[bindings] exported src/lib/bindings.ts");
        }
    }

    // Clone the builder so `mount_events` (setup closure) and `invoke_handler`
    // (chained after setup) can each own a copy. Builder::clone is cheap —
    // it clones the shared command list Arc and the BuilderConfiguration.
    let builder_for_setup = builder.clone();

    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("apex".to_string()),
                    }),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Webview),
                ])
                .level(log::LevelFilter::Info)
                .format(|out, message, record| {
                    out.finish(format_args!(
                        "[{} {}] {}",
                        record.level(),
                        record.target(),
                        message
                    ))
                })
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(registry)
        .manage(prep_semaphore)
        .manage(cancel_token)
        .setup(move |app| {
            // Mount typed event registry so Event::listen / Event::emit resolve
            // channel names at runtime (required when collect_events! is used).
            builder_for_setup.mount_events(app);

            // T7 maximize_on_start: load settings and maximize the main window
            // when the flag is true (default true — preserves existing behavior).
            // `"maximized": true` has been removed from tauri.conf.json; this is
            // the dynamic replacement.
            let setup_settings = settings::load(app.handle()).unwrap_or_default();
            if setup_settings.maximize_on_start {
                if let Some(win) = app.get_webview_window("main") {
                    if let Err(e) = win.maximize() {
                        log::warn!("setup: could not maximize main window: {e}");
                    }
                }
            }

            // Initialize the account store now that the AppHandle is available,
            // so the real on-disk path can be resolved.
            //
            // If the keyring is unavailable at runtime (e.g. WSL without
            // secret-service), login will fail loud — no silent plaintext fallback.
            let account_path = core::store::account_file(app.handle())
                .expect("failed to resolve account file path");
            let store = AccountStore::load(account_path.clone(), Box::new(auth::SystemKeyringBackend))
                .unwrap_or_else(|e| {
                    // Load failed (e.g. corrupt account.json). Start with an empty
                    // in-memory store so the app boots without panicking. The path is
                    // kept so future writes (on next successful login) go to the right
                    // location.
                    log::warn!("auth: could not load account.json ({e}); starting with empty store");
                    AccountStore::new_empty(account_path, Box::new(auth::SystemKeyringBackend))
                });
            let shared: SharedAccountStore = Arc::new(tokio::sync::Mutex::new(store));
            app.manage(shared);

            // Download Manager: serial FIFO task queue. Constructed here (not in
            // run()) because its observer needs the AppHandle to emit task://*
            // events. `new` spawns the single worker task on the Tauri runtime.
            let task_observer = Arc::new(TauriTaskObserver {
                app: app.handle().clone(),
            });
            let task_manager = TaskManager::new(task_observer);
            app.manage(task_manager);
            Ok(())
        })
        .invoke_handler(builder.invoke_handler())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
