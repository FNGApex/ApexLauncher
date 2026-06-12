use serde::Serialize;
use std::sync::Arc;
use tauri::Manager as _;

mod core;
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
use core::modrinth::ModrinthProvider;
use core::providers::{
    self, cf_api_key_from, ModProvider, ProviderError, ReqwestProviderClient, SearchParams,
    SearchResult, ProjectVersion,
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
    let paths = launch::LaunchPaths::new(&cache_dir, &instances_dir, &inst.slug);

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
#[tauri::command]
async fn search_mods(
    app: tauri::AppHandle,
    provider: String,
    query: String,
    mc_version: Option<String>,
    loader: Option<String>,
    offset: u32,
    limit: u32,
) -> Result<SearchResult, ProviderCommandError> {
    let params = SearchParams { query, mc_version, loader, offset, limit };
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
            );
            let p = CurseForgeProvider::new(key);
            p.get_versions(&http, &project_id, mc_version.as_deref(), loader.as_deref())
                .await
                .map_err(ProviderCommandError::from)
        }
        other => Err(unknown_provider_err(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // These tests encode the Rust↔TS IPC contract: the `kind` strings emitted by
    // `From<ProviderError>` and the unknown-provider path MUST match the values
    // documented in `ipc.ts`'s `ProviderCommandError` comment. If a string here
    // changes, the frontend `kind` checks in Browse.tsx will silently stop working.

    #[test]
    fn provider_error_key_missing_kind() {
        let cmd = ProviderCommandError::from(ProviderError::KeyMissing);
        assert_eq!(cmd.kind, "key_missing");
    }

    #[test]
    fn provider_error_network_kind() {
        // Transport failure (connection refused, TLS error, timeout) maps to "network",
        // NOT "bad_response". The frontend uses this kind to show a connectivity message.
        let cmd = ProviderCommandError::from(ProviderError::Network("connection refused".to_string()));
        assert_eq!(cmd.kind, "network");
        assert_ne!(cmd.kind, "bad_response");
    }

    #[test]
    fn provider_error_http_status_kind() {
        let cmd = ProviderCommandError::from(ProviderError::HttpStatus {
            status: 403,
            body: "Forbidden".to_string(),
        });
        assert_eq!(cmd.kind, "http_status");
    }

    #[test]
    fn provider_error_bad_response_kind() {
        let cmd = ProviderCommandError::from(ProviderError::BadResponse("parse failed".to_string()));
        assert_eq!(cmd.kind, "bad_response");
    }

    #[test]
    fn unknown_provider_kind_is_distinct() {
        // The unknown-provider path must NOT reuse "bad_response" — the frontend
        // uses the kind to decide whether to show "API key required" vs a generic
        // error, and "bad_response" would mask a misconfigured provider string.
        // Exercises the real helper so a regression (e.g. returning "bad_response")
        // fails here, not just in production.
        let cmd = unknown_provider_err("bogus");
        assert_eq!(cmd.kind, "unknown_provider");
        assert_ne!(cmd.kind, "bad_response");
        assert!(cmd.message.contains("bogus"), "message should name the bad provider: {}", cmd.message);
    }
}

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
            get_mod_versions
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
