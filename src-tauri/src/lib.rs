use serde::Serialize;
use std::sync::Arc;
use tauri::Manager as _;

// `pub` so integration tests in `tests/` can reach the pure helpers
// (e.g. `store::cache_subdir_path`). This is an internal app lib, not a
// published crate, so widening visibility for testing is harmless.
pub mod core;
use core::auth::{self, AccountMeta, AccountStore, AuthError};
use core::download::{self, DownloadPlan, ItemOutcome, ItemStatus, PlanResult, ProgressSink, ProgressUpdate};
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
    self, cf_api_key_from, ModProvider, ProjectType, ProviderError, ReqwestProviderClient,
    SearchParams, SearchResult, ProjectVersion,
};
use core::settings::{self, Settings};
use core::versions::{self, McVersion};

// --- Phase 3: authentication ---

/// Serializable command error wrapping [`AuthError`] (F-5).
///
/// `AuthError::Http` contains `reqwest::Error` which is not `Serialize`.
/// This wrapper converts every variant to a string at the command boundary so
/// the frontend always receives a JSON-serializable `{"error": "..."}` on
/// failure. The named variant is preserved in the `kind` field for typed
/// handling in the UI.
#[derive(Debug, Serialize)]
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
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
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
async fn begin_login(
    app: tauri::AppHandle,
    store_state: tauri::State<'_, SharedAccountStore>,
    cancel_state: tauri::State<'_, CancelToken>,
) -> Result<AccountMeta, AuthCommandError> {
    use tauri::Emitter as _;
    use tokio::time::{sleep, Duration};

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

    Ok(meta)
}

/// Cancel an in-flight `begin_login` without panic or deadlock.
///
/// If no login is in progress, this is a no-op.
#[tauri::command]
fn cancel_login(cancel_state: tauri::State<'_, CancelToken>) {
    let mut guard = cancel_state.lock().unwrap();
    if let Some(tx) = guard.take() {
        // Receiver dropped = login already completed; silently ignore.
        let _ = tx.send(());
    }
}

/// Get the current account metadata, or `None` if not logged in.
#[tauri::command]
async fn get_account(
    store_state: tauri::State<'_, SharedAccountStore>,
) -> Result<Option<AccountMeta>, AuthCommandError> {
    let guard = store_state.lock().await;
    Ok(guard.get_account().cloned())
}

/// Log out: clear `account.json` and the keyring entry.
#[tauri::command]
async fn logout(
    store_state: tauri::State<'_, SharedAccountStore>,
) -> Result<(), AuthCommandError> {
    let mut guard = store_state.lock().await;
    guard.logout().map_err(AuthCommandError::from)
}

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

/// Payload emitted on the `install://log` Tauri event channel.
///
/// Distinct from `launch://log` — installer output is log-shaped (one line at
/// a time from the installer's stdout/stderr), not item-progress-shaped.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallLogPayload {
    /// `"stdout"` or `"stderr"`.
    stream: String,
    line: String,
}

/// An [`InstallSink`] that emits `install://log` events on the Tauri event channel.
struct TauriInstallSink {
    app: tauri::AppHandle,
}

impl InstallSink for TauriInstallSink {
    fn log(&self, stream: &str, line: &str) {
        use tauri::Emitter as _;
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
async fn launch_instance(
    app: tauri::AppHandle,
    registry_state: tauri::State<'_, Arc<RunningRegistry>>,
    store_state: tauri::State<'_, SharedAccountStore>,
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

        let install_sink = TauriInstallSink { app: app.clone() };
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

    // --- 5. Ensure Java (reuse from installer step if forge/neoforge). ---
    let java_inst = match java_inst_opt {
        Some(j) => j,
        None => core::java::ensure_java(&app, launch_meta.java_major).await?,
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
    // Load settings to check the offline-mode toggle, then delegate to the
    // seam-injectable resolver. The MC token is never cached (CP3 decision), so
    // the resolver always performs a full MS refresh → Xbox chain when an active
    // account is present. Lock is held for the duration of the async call; the
    // resolver drops no sub-locks internally — single lock site here.
    let app_settings = settings::load(&app).unwrap_or_default();
    let http = auth::ReqwestAuthClient(reqwest::Client::new());
    let identity = {
        let mut store_guard = store_state.lock().await;
        launch::resolve_launch_identity(&mut store_guard, &http, app_settings.offline_mode)
            .await
            .map_err(|e| format!("identity resolution failed: {e}"))?
    };

    // --- 8b. Build argv. ---
    let argv = launch::build_argv(&launch_meta, &paths, &identity)
        .map_err(|e| format!("argv assembly failed: {e}"))?;

    // --- 9. Spawn (registry keyed by slug). ---
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

// --- Phase 5: provider browse (search + versions) ---

/// Serializable command error wrapping [`ProviderError`].
///
/// Mirrors `AuthCommandError` above. Every variant maps to a camelCase `kind`
/// string; `"key_missing"` lets the frontend show the "API key required" state.
#[derive(Debug, Serialize)]
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
#[tauri::command]
async fn add_mod(
    app: tauri::AppHandle,
    provider: String,
    project_id: String,
    version_id: String,
    slug: String,
    mc_version: String,
    loader: String,
) -> Result<AddModResult, String> {
    use core::download::{self as dl, DownloadPlan, NoOpSink};
    use core::mod_install::{attribute_outcomes, build_download_items, merge_mod_entries, partition_by_file_name, resolve_install};
    use std::collections::HashSet;

    // 1. Build provider + HTTP client.
    let http = ReqwestProviderClient(reqwest::Client::new());
    let settings = settings::load(&app).unwrap_or_default();
    // Resolve CF key once; reused for both version fetch and dep resolution.
    let cf_key = cf_api_key_from(
        std::env::var(providers::CF_API_KEY_ENV).ok(),
        settings.curseforge_api_key,
        option_env!("MODLOADER_CF_API_KEY").map(str::to_string),
    );

    // Resolve the root version by fetching all compatible versions and picking
    // the one matching `version_id`.
    let versions = match provider.as_str() {
        "modrinth" => {
            let p = ModrinthProvider;
            p.get_versions(&http, &project_id, Some(&mc_version), Some(&loader))
                .await
                .map_err(|e| e.to_string())?
        }
        "curseforge" => {
            let p = CurseForgeProvider::new(cf_key.clone());
            p.get_versions(&http, &project_id, Some(&mc_version), Some(&loader))
                .await
                .map_err(|e| e.to_string())?
        }
        other => return Err(format!("unknown provider: {other}")),
    };

    let root_version = versions
        .into_iter()
        .find(|v| v.id == version_id)
        .ok_or_else(|| format!("version '{version_id}' not found for project '{project_id}'"))?;

    // 2. Load the instance manifest; derive already-installed project_ids.
    let mut instance = instances::load_manifest(&app, &slug)?;
    let already_installed: HashSet<String> = instance.mods.iter().map(|m| m.project_id.clone()).collect();

    // 3. Run the planner.
    // Pass `&project_id` as the root slug so the root mod's page_url uses the
    // mod's own project identifier, not the instance slug.
    let plan = match provider.as_str() {
        "modrinth" => {
            let p = ModrinthProvider;
            resolve_install(&p, &http, root_version, &project_id, &project_id, &mc_version, &loader, &already_installed).await
        }
        "curseforge" => {
            let p = CurseForgeProvider::new(cf_key);
            resolve_install(&p, &http, root_version, &project_id, &project_id, &mc_version, &loader, &already_installed).await
        }
        other => return Err(format!("unknown provider: {other}")),
    };

    // 4. Execute downloads into `<instances>/<slug>/mc/mods/`.
    // Validate the slug here at the join site — `load_manifest` already verified it,
    // but making the invariant explicit prevents silent breakage on future reorder.
    instances::validate_slug(&slug).map_err(|e| format!("invalid instance slug: {e}"))?;
    let inst_dir = core::store::instances_dir(&app)?.join(&slug);
    let mods_dir = inst_dir.join("mc").join("mods");
    std::fs::create_dir_all(&mods_dir).map_err(|e| format!("could not create mods dir: {e}"))?;

    // Validate provider-supplied file names before building DownloadItems.
    // A compromised/malicious provider response with `file_name` containing `../`,
    // `/`, `\`, a drive prefix, or a non-`.jar` extension must never reach the FS.
    // Invalid entries go directly to `result.failed`; valid entries proceed normally.
    let (valid_downloads, invalid_downloads) = partition_by_file_name(plan.downloads.clone());

    let download_plan = DownloadPlan::new(build_download_items(
        &core::mod_install::InstallPlan {
            downloads: valid_downloads.clone(),
            ..Default::default()
        },
        &mods_dir,
    ));
    let mut result = AddModResult {
        manual: plan.manual.clone(),
        unresolved: plan.unresolved.clone(),
        suggestions: plan.suggestions.clone(),
        warnings: plan.warnings.clone(),
        failed: invalid_downloads,
        ..Default::default()
    };

    if !download_plan.items.is_empty() {
        let client = dl::build_client().map_err(|e| e.to_string())?;
        let n = 8_usize;
        let plan_result = dl::execute_plan(&client, &download_plan, &NoOpSink, n).await;

        // Attribute outcomes by URL (order-independent) — execute_plan uses
        // FuturesUnordered and prepends already-skipped items, so index matching is wrong.
        let (added, mut failed) = attribute_outcomes(&valid_downloads, &plan_result);
        result.added = added;
        result.failed.append(&mut failed);
    }

    // 5. Merge added entries into manifest and persist.
    merge_mod_entries(&mut instance.mods, result.added.clone());
    instances::save_manifest(&app, &slug, &instance)?;

    Ok(result)
}

// ---------------------------------------------------------------------------
// CP3: Mod state operations
// ---------------------------------------------------------------------------

/// Enable or disable an installed mod by renaming its file and flipping the
/// `enabled` flag in the manifest.
///
/// `file_name` is the base `.jar` name (no `.disabled` suffix). Both `slug`
/// and `file_name` are validated for path traversal before any FS access.
#[tauri::command]
fn set_mod_enabled(
    app: tauri::AppHandle,
    slug: String,
    file_name: String,
    enabled: bool,
) -> Result<(), String> {
    instances::set_mod_enabled(&app, &slug, &file_name, enabled)
}

/// Delete a mod's on-disk file (enabled or disabled form) and drop its
/// manifest entry. Missing file is not an error — system converges to "gone".
///
/// `file_name` is the base `.jar` name (no `.disabled` suffix). Both `slug`
/// and `file_name` are validated for path traversal before any FS access.
#[tauri::command]
fn remove_mod(app: tauri::AppHandle, slug: String, file_name: String) -> Result<(), String> {
    instances::remove_mod(&app, &slug, &file_name)
}

// ---------------------------------------------------------------------------
// CP4: Update a single installed mod
// ---------------------------------------------------------------------------

/// Update a single tracked mod to its newest compatible version.
///
/// # Flow
/// 1. Load manifest; find the `ModEntry` whose `project_id` matches — error if absent.
/// 2. Build provider + HTTP client (mirrors `add_mod` / `search_mods`).
/// 3. Resolve the newest compatible version via `fetch_newest_compatible` (CP1 planner).
/// 4. `decide_update` decides what to do:
///    - `UpToDate` / `Unresolved` / `Manual` → no disk change; return status.
///    - `Swap` → download the new file into `mc/mods/`:
///      a. On success: delete old file (both `.jar` and `.jar.disabled`), apply
///         `apply_swap` to update the entry in place (`version_id`/`file_name`/`hashes`),
///         **preserve `enabled`** (disabled mod → new file placed with `.disabled` suffix).
///      b. On failure: leave old file + entry untouched; return `"failed"`.
/// 5. Save manifest on swap success; return `UpdateModResult`.
#[tauri::command]
async fn update_mod(
    app: tauri::AppHandle,
    slug: String,
    project_id: String,
) -> Result<UpdateModResult, String> {
    use core::download::{self as dl, DownloadItem, DownloadPlan, ExpectedHash, NoOpSink};
    use core::mod_install::{apply_swap, decide_update, fetch_newest_compatible, page_url_for, UpdateAction};

    // 1. Load manifest; find the entry.
    let mut instance = instances::load_manifest(&app, &slug)?;
    let entry_idx = instance
        .mods
        .iter()
        .position(|m| m.project_id == project_id)
        .ok_or_else(|| format!("mod with project_id '{project_id}' not found in instance '{slug}'"))?;

    let mc_version = instance.minecraft.clone();
    let loader = instance.loader.kind.clone();

    // 2. Build provider + HTTP client (mirror of add_mod).
    let http = ReqwestProviderClient(reqwest::Client::new());
    let settings = settings::load(&app).unwrap_or_default();
    let cf_key = cf_api_key_from(
        std::env::var(providers::CF_API_KEY_ENV).ok(),
        settings.curseforge_api_key,
        option_env!("MODLOADER_CF_API_KEY").map(str::to_string),
    );

    let provider_str = instance.mods[entry_idx].provider.clone();

    // 3. Resolve newest compatible version.
    let newest = match provider_str.as_str() {
        "modrinth" => {
            let p = ModrinthProvider;
            fetch_newest_compatible(&p, &http, &project_id, &mc_version, &loader, None).await
        }
        "curseforge" => {
            let p = CurseForgeProvider::new(cf_key);
            fetch_newest_compatible(&p, &http, &project_id, &mc_version, &loader, None).await
        }
        other => return Err(format!("unknown provider '{other}' in manifest")),
    };

    // 4. Decide action.
    let action = decide_update(&instance.mods[entry_idx], newest.as_ref());

    match action {
        UpdateAction::UpToDate => Ok(UpdateModResult {
            status: "upToDate".to_string(),
            entry: None,
            page_url: None,
            error: None,
        }),
        UpdateAction::Unresolved => Ok(UpdateModResult {
            status: "unresolved".to_string(),
            entry: None,
            page_url: None,
            error: None,
        }),
        UpdateAction::Manual { page_url: _, file_name: _ } => {
            // page_url from decide_update is empty placeholder; build the real one here.
            let newest_version = newest.as_ref().unwrap(); // safe: Manual arm means newest is Some
            let new_file = newest_version
                .files
                .iter()
                .find(|f| f.primary)
                .or_else(|| newest_version.files.first())
                .unwrap(); // safe: Manual arm means files is non-empty
            let real_page_url = page_url_for(newest_version.provider, &project_id);
            Ok(UpdateModResult {
                status: "manual".to_string(),
                entry: None,
                page_url: Some(real_page_url),
                error: Some(format!(
                    "distribution disabled; download '{}' manually",
                    new_file.file_name
                )),
            })
        }
        UpdateAction::Swap { new_file, new_version_id } => {
            // Build the destination path.
            instances::validate_slug(&slug)
                .map_err(|e| format!("invalid instance slug: {e}"))?;
            let inst_dir = core::store::instances_dir(&app)?.join(&slug);
            let mods_dir = inst_dir.join("mc").join("mods");
            std::fs::create_dir_all(&mods_dir)
                .map_err(|e| format!("could not create mods dir: {e}"))?;

            let url = new_file.url.clone().unwrap(); // safe: Swap arm guarantees url is Some

            // Validate provider-supplied file name before any FS write.
            // A malicious or compromised provider response could supply `"../evil.jar"`,
            // an absolute path, or a non-jar name — reject before joining to mods_dir.
            if let Err(e) = instances::validate_mod_file_name(&new_file.file_name) {
                return Ok(UpdateModResult {
                    status: "failed".to_string(),
                    entry: None,
                    page_url: None,
                    error: Some(format!("invalid provider file name: {e}")),
                });
            }

            // Choose expected hash: sha512 > sha1 > None.
            let expected_hash = if let Some(h) = new_file.hashes.get("sha512") {
                Some(ExpectedHash::Sha512(h.clone()))
            } else if let Some(h) = new_file.hashes.get("sha1") {
                Some(ExpectedHash::Sha1(h.clone()))
            } else {
                None
            };

            // If the mod is disabled, place the new file with the `.disabled` suffix
            // so disk state matches the `enabled` flag after the swap.
            let enabled = instance.mods[entry_idx].enabled;
            let dest_file_name = if enabled {
                new_file.file_name.clone()
            } else {
                format!("{}.disabled", new_file.file_name)
            };

            let download_plan = DownloadPlan::new(vec![DownloadItem {
                url: url.clone(),
                dest: mods_dir.join(&dest_file_name),
                expected_hash,
                size: new_file.size,
            }]);

            let client = dl::build_client().map_err(|e| e.to_string())?;
            let plan_result = dl::execute_plan(&client, &download_plan, &NoOpSink, 1).await;

            // Check download outcome.
            use core::download::ItemStatus;
            let outcome = plan_result.outcomes.first();
            let download_ok = matches!(
                outcome.map(|o| &o.status),
                Some(ItemStatus::Ok) | Some(ItemStatus::Skipped)
            );

            if !download_ok {
                let err = outcome
                    .and_then(|o| match &o.status {
                        ItemStatus::Failed { error } => Some(error.clone()),
                        _ => None,
                    })
                    .unwrap_or_else(|| "download failed: no outcome recorded".to_string());
                return Ok(UpdateModResult {
                    status: "failed".to_string(),
                    entry: None,
                    page_url: None,
                    error: Some(err),
                });
            }

            // Download succeeded: delete old file (both variants).
            let old_file_name = instance.mods[entry_idx].file_name.clone();
            instances::remove_mod_from_disk_files(&mods_dir, &old_file_name);

            // Update the entry in place.
            apply_swap(&mut instance.mods[entry_idx], &new_file, &new_version_id);
            // Save manifest.
            instances::save_manifest(&app, &slug, &instance)?;

            Ok(UpdateModResult {
                status: "updated".to_string(),
                entry: Some(instance.mods[entry_idx].clone()),
                page_url: None,
                error: None,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// CP4: Modpack import
// ---------------------------------------------------------------------------

/// Result returned to the frontend after a successful `.mrpack` import.
#[derive(Debug, Serialize)]
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

/// Import a local Modrinth `.mrpack` file into a new instance.
///
/// # Flow
/// 1. Read `mrpack_path` bytes from disk.
/// 2. Call `read_mrpack` to parse `modrinth.index.json` and build a [`PackPlan`].
/// 3. Create an instance via `instances::create` (name = `name_override` or manifest.name;
///    mc + loader from manifest).
/// 4. Execute the download plan (`execute_plan`, `NoOpSink`); tally installed/failed.
/// 5. Extract overrides (`extract_overrides`) into the instance `mc/` dir.
/// 6. Merge `PackPlan.mods` into the instance manifest; persist.
/// 7. Return [`MrpackImportResult`].
///
/// All errors map to `String`.
#[tauri::command]
async fn import_mrpack(
    app: tauri::AppHandle,
    mrpack_path: String,
    name_override: Option<String>,
) -> Result<MrpackImportResult, String> {
    use core::download::{self as dl, DownloadPlan, ItemStatus, NoOpSink};
    use core::modpack::extract_overrides;

    // 1. Read file bytes.
    let bytes = std::fs::read(&mrpack_path)
        .map_err(|e| format!("could not read mrpack file '{mrpack_path}': {e}"))?;

    // 2. Parse manifest minimally to obtain name/mc/loader for instance creation.
    //    The full parse+plan goes through the tested `read_mrpack` seam below (step 4).
    let pre_create_manifest = {
        use std::io::{Cursor, Read};
        let mut archive = zip::ZipArchive::new(Cursor::new(&bytes))
            .map_err(|e| format!("could not open mrpack zip: {e}"))?;
        let index_json = {
            let mut entry = archive
                .by_name("modrinth.index.json")
                .map_err(|_| "modrinth.index.json not found in archive".to_string())?;
            let mut buf = String::new();
            entry.read_to_string(&mut buf).map_err(|e| e.to_string())?;
            buf
        };
        modpack::parse_modrinth_index(&index_json).map_err(|e| e.to_string())?
    };

    // 3. Create the instance.
    let instance_name = name_override
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| pre_create_manifest.name.clone());

    let inst = instances::create(
        &app,
        CreateInstanceReq {
            name: instance_name,
            minecraft: pre_create_manifest.minecraft.clone(),
            loader: instances::Loader {
                kind: pre_create_manifest.loader.kind.clone(),
                version: pre_create_manifest.loader.version.clone(),
            },
        },
    )?;

    // Locate the instance mc/ directory.
    let inst_dir = core::store::instances_dir(&app)?.join(&inst.slug);
    let mc_dir = inst_dir.join("mc");

    // 4. Parse manifest + build plan through the tested seam.
    //    `read_mrpack` opens the zip, re-parses modrinth.index.json, and runs
    //    `build_pack_plan` — the same path exercised by the unit tests.
    let (_manifest, plan) = modpack::read_mrpack(&bytes, &mc_dir).map_err(|e| e.to_string())?;
    let skipped_count = plan.skipped.len() as u32;
    let mod_entries = plan.mods.clone();
    let download_plan = DownloadPlan::new(plan.items);

    // Execute downloads.
    let client = dl::build_client().map_err(|e| e.to_string())?;
    let plan_result = dl::execute_plan(&client, &download_plan, &NoOpSink, 8).await;

    let mut installed: u32 = 0;
    let mut failed: u32 = 0;
    let mut failed_files: Vec<String> = Vec::new();

    for outcome in &plan_result.outcomes {
        match &outcome.status {
            ItemStatus::Ok | ItemStatus::Skipped => installed += 1,
            ItemStatus::Failed { .. } => {
                failed += 1;
                failed_files.push(outcome.url.clone());
            }
        }
    }

    // 5. Extract overrides into mc_dir.
    {
        use std::io::Cursor;
        let cursor = Cursor::new(&bytes);
        let mut archive = zip::ZipArchive::new(cursor)
            .map_err(|e| format!("could not re-open mrpack zip for overrides: {e}"))?;
        extract_overrides(&mut archive, &mc_dir).map_err(|e| e.to_string())?;
    }

    // 6. Merge mod entries into manifest and persist.
    let mut instance = instances::load_manifest(&app, &inst.slug)?;
    instance.mods = mod_entries;
    instances::save_manifest(&app, &inst.slug, &instance)?;

    Ok(MrpackImportResult {
        slug: inst.slug,
        name: inst.name,
        installed,
        failed,
        skipped: skipped_count,
        failed_files,
    })
}

// ---------------------------------------------------------------------------
// CP B4: CurseForge `.zip` modpack import
// ---------------------------------------------------------------------------

/// Result returned to the frontend after a successful CurseForge `.zip` import.
#[derive(Debug, Serialize)]
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

/// Import a local CurseForge modpack `.zip` into a new instance.
///
/// # Flow
/// 1. Read `cf_zip_path` bytes from disk; parse `manifest.json` via `read_cf_manifest`.
/// 2. Create an instance via `instances::create` (name = `name_override` or manifest.name;
///    mc + loader from manifest).
/// 3. Resolve every `files[]` entry via `CurseForgeProvider::get_file` and build the
///    `CfPackPlan` (`resolve_and_build_cf_plan` — the tested seam).
/// 4. Execute the download plan (`execute_plan`, `NoOpSink`); tally installed/failed.
/// 5. Extract overrides (`extract_overrides`, reused unchanged) into the instance `mc/` dir.
/// 6. Merge `CfPackPlan.mods` into the instance manifest; persist.
/// 7. Return [`CfImportResult`].
///
/// All errors map to `String`.
#[tauri::command]
async fn import_curseforge_zip(
    app: tauri::AppHandle,
    cf_zip_path: String,
    name_override: Option<String>,
) -> Result<CfImportResult, String> {
    use core::download::{self as dl, DownloadPlan, ItemStatus, NoOpSink};
    use core::modpack::{extract_overrides, read_cf_manifest, resolve_and_build_cf_plan};

    // 1. Read file bytes; parse manifest.json.
    let bytes = std::fs::read(&cf_zip_path)
        .map_err(|e| format!("could not read CurseForge zip '{cf_zip_path}': {e}"))?;
    let manifest = read_cf_manifest(&bytes).map_err(|e| e.to_string())?;

    // 2. Create the instance.
    let instance_name = name_override
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| manifest.name.clone());

    let inst = instances::create(
        &app,
        CreateInstanceReq {
            name: instance_name,
            minecraft: manifest.minecraft.clone(),
            loader: instances::Loader {
                kind: manifest.loader.kind.clone(),
                version: manifest.loader.version.clone(),
            },
        },
    )?;

    let inst_dir = core::store::instances_dir(&app)?.join(&inst.slug);
    let mc_dir = inst_dir.join("mc");

    // 3. Resolve files (CF API) + build the plan through the tested seam.
    let settings = settings::load(&app).unwrap_or_default();
    let cf_key = cf_api_key_from(
        std::env::var(providers::CF_API_KEY_ENV).ok(),
        settings.curseforge_api_key,
        option_env!("MODLOADER_CF_API_KEY").map(str::to_string),
    );
    let provider = CurseForgeProvider::new(cf_key);
    let http = ReqwestProviderClient(reqwest::Client::new());

    let plan = resolve_and_build_cf_plan(&provider, &http, &manifest, &mc_dir)
        .await
        .map_err(|e| e.to_string())?;

    let manual = plan.manual.clone();
    let mod_entries = plan.mods.clone();
    let resolve_failed_count = plan.failed.len() as u32;
    let download_plan = DownloadPlan::new(plan.items);

    // 4. Execute downloads.
    let client = dl::build_client().map_err(|e| e.to_string())?;
    let plan_result = dl::execute_plan(&client, &download_plan, &NoOpSink, 8).await;

    let mut installed: u32 = 0;
    let mut failed: u32 = resolve_failed_count;

    for outcome in &plan_result.outcomes {
        match &outcome.status {
            ItemStatus::Ok | ItemStatus::Skipped => installed += 1,
            ItemStatus::Failed { .. } => failed += 1,
        }
    }

    // 5. Extract overrides into mc_dir (reused unchanged).
    {
        use std::io::Cursor;
        let cursor = Cursor::new(&bytes);
        let mut archive = zip::ZipArchive::new(cursor)
            .map_err(|e| format!("could not re-open CF zip for overrides: {e}"))?;
        extract_overrides(&mut archive, &mc_dir).map_err(|e| e.to_string())?;
    }

    // 6. Merge mod entries into manifest and persist.
    let mut instance = instances::load_manifest(&app, &inst.slug)?;
    instance.mods = mod_entries;
    instances::save_manifest(&app, &inst.slug, &instance)?;

    Ok(CfImportResult {
        slug: inst.slug,
        name: inst.name,
        installed,
        failed,
        manual,
    })
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // The running instance registry is the first managed state in this app.
    // Managed as Arc<RunningRegistry> so the monitor background tasks can clone
    // the Arc and still access the shared map after the command returns.
    let registry: Arc<RunningRegistry> = Arc::new(launch::new_running_registry());

    // Cancel token for in-flight begin_login.
    let cancel_token: CancelToken = std::sync::Mutex::new(None);

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(registry)
        .manage(cancel_token)
        .setup(|app| {
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
                    eprintln!("auth: could not load account.json ({e}); starting with empty store");
                    AccountStore::new_empty(account_path, Box::new(auth::SystemKeyringBackend))
                });
            let shared: SharedAccountStore = Arc::new(tokio::sync::Mutex::new(store));
            app.manage(shared);
            Ok(())
        })
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
            kill_instance,
            begin_login,
            cancel_login,
            get_account,
            logout,
            search_mods,
            get_mod_versions,
            add_mod,
            set_mod_enabled,
            remove_mod,
            update_mod,
            import_mrpack,
            import_curseforge_zip
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
