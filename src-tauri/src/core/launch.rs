//! Vanilla launch support — Phase 2, slice D.
//!
//! CP1: pure argv assembler. Takes a [`LaunchMeta`] + resolved paths +
//! offline identity, substitutes all `${...}` placeholders, and produces the
//! final `Vec<String>` argv for the JVM. No process spawn (CP3).
//!
//! CP2: natives extraction. Each jar in `LaunchMeta.natives` is unpacked
//! into a per-instance natives dir. `META-INF/` entries and directory entries
//! are skipped. Any entry whose resolved path escapes the target dir is refused
//! (zip-slip / `../` traversal guard).
//!
//! CP3: tokio::process spawn + log streaming + running registry + kill.
//! The core is Tauri-free and generic over a [`LaunchSink`] trait. Callers in
//! `lib.rs` supply a `TauriLaunchSink` that emits `launch://log` events; tests
//! use a `CapturingLaunchSink`.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::core::auth::{AccountStore, AuthError, AuthHttpClient};
use crate::core::resolver::LaunchMeta;

// ---------------------------------------------------------------------------
// Offline identity
// ---------------------------------------------------------------------------

/// Fixed offline player name (Phase 3 replaces with real auth).
pub const OFFLINE_PLAYER_NAME: &str = "Player";

/// Derive a deterministic UUID for offline use.
///
/// Uses UUID v3 (MD5-based namespace) over the string `"OfflinePlayer:Player"`.
/// This matches the standard Java offline convention — `Uuid::NIL` as namespace,
/// name bytes = `b"OfflinePlayer:Player"`.
///
/// Phase 3 replaces this with the real Mojang/MSA UUID.
pub fn offline_uuid() -> uuid::Uuid {
    // NIL UUID (all-zeros) as the namespace, matching the standard offline convention.
    let nil = uuid::Uuid::from_u128(0);
    uuid::Uuid::new_v3(&nil, b"OfflinePlayer:Player")
}

// ---------------------------------------------------------------------------
// Resolved path inputs for the assembler
// ---------------------------------------------------------------------------

/// All file-system paths the argv assembler needs to substitute placeholders.
///
/// The caller (CP3 spawn logic) fills these in from the instance manifest and
/// app data directory before invoking [`build_argv`].
pub struct LaunchPaths {
    /// Absolute path to `<instances>/<slug>/mc/` — the Minecraft working dir.
    pub game_directory: PathBuf,
    /// Absolute path to `<cache>/assets/`.
    /// `--assetsDir` always points here (assets stay in the shared cache; see
    /// storage-auth-reorg design Recommendation A).
    pub assets_root: PathBuf,
    /// Absolute path to the per-instance natives extraction dir.
    /// CP2 will extract native jars here before launch.
    pub natives_directory: PathBuf,
    /// For legacy assets: absolute path to the virtual/legacy asset tree
    /// (e.g. `<data>/assets/virtual/legacy`). Only used when
    /// `LaunchMeta.assets_legacy` is true. Legacy-asset materialization is
    /// handled by CP2; this field selects the path without building it.
    pub legacy_assets_root: PathBuf,
    /// Absolute path to `<instances>/<slug>/libraries/` — the per-instance
    /// materialized library tree. After C2, `${library_directory}` substitutes
    /// to this path (used by Forge/NeoForge `-DlibraryDirectory=`).
    pub library_directory: PathBuf,
}

impl LaunchPaths {
    /// Construct standard paths from the cache dir and instance slug.
    ///
    /// `cache_dir` — the launcher cache directory (`<data>/cache/`); assets live here.
    /// `instances_dir` — the directory that holds all instance subdirs.
    /// `slug` — the instance slug (subdirectory name under `instances_dir`).
    pub fn new(cache_dir: &Path, instances_dir: &Path, slug: &str) -> Self {
        let inst_dir = instances_dir.join(slug);
        Self {
            game_directory: inst_dir.join("mc"),
            assets_root: cache_dir.join("assets"),
            natives_directory: inst_dir.join("natives"),
            legacy_assets_root: cache_dir.join("assets").join("virtual").join("legacy"),
            library_directory: inst_dir.join("libraries"),
        }
    }
}

// ---------------------------------------------------------------------------
// Argv assembler
// ---------------------------------------------------------------------------

/// Launch identity injected into `build_argv`.
///
/// The caller resolves which identity to use (online vs. offline, refresh if
/// needed) before calling `build_argv`; this struct holds the resolved values.
pub struct LaunchIdentity {
    /// Minecraft player name.
    pub player_name: String,
    /// Minecraft profile UUID (hyphenated string).
    pub uuid: String,
    /// MC access token. Use `"0"` for offline.
    pub access_token: String,
    /// Xbox user ID. Use `""` or `"0"` for offline.
    pub xuid: String,
    /// Auth user type, e.g. `"msa"`.
    pub user_type: String,
    /// MSA application client ID, substituted into `${clientid}` (the
    /// `--clientId` telemetry arg in MS-auth version JSONs). Empty for offline.
    pub client_id: String,
}

impl LaunchIdentity {
    /// Standard offline identity matching the pre-Phase-3 hardcoded values.
    pub fn offline() -> Self {
        LaunchIdentity {
            player_name: OFFLINE_PLAYER_NAME.to_string(),
            uuid: offline_uuid().as_hyphenated().to_string(),
            access_token: "0".to_string(),
            xuid: "0".to_string(),
            user_type: "msa".to_string(),
            client_id: String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Identity resolution (CP4 — injected seam for testability)
// ---------------------------------------------------------------------------

/// Resolve the launch identity from the account store and the offline flag.
///
/// - `offline = true` → always returns [`LaunchIdentity::offline()`].
/// - No active account → offline identity (user hasn't logged in yet).
/// - Active account found → retrieves the stored MS refresh token from the
///   keyring, runs `refresh_ms_token` → `xbox_chain` to obtain a fresh MC
///   access token, updates the store, and returns an online identity.
///
/// The MC access token is never cached on disk (CP3 decision), so a full
/// refresh is always performed when an online account is present.
///
/// Injectable seams: the store is passed by reference (not built inside this
/// function) and the HTTP client is injected as a trait object — tests supply
/// a mock client and a fake-keyring-backed store.
///
/// The caller (normally `launch_instance` in `lib.rs`) is responsible for
/// persisting any changes made to the store (via `set_account`) and for
/// holding any lock around the store for the duration of this call.
pub async fn resolve_launch_identity(
    store: &mut AccountStore,
    http: &dyn AuthHttpClient,
    offline: bool,
) -> Result<LaunchIdentity, AuthError> {
    if offline {
        return Ok(LaunchIdentity::offline());
    }

    if store.get_account().is_none() {
        return Ok(LaunchIdentity::offline());
    }

    // MC token is never persisted — always re-derive via refresh.
    let refresh_token = store.get_refresh_token()?;

    let ms_tokens = crate::core::auth::refresh_ms_token(http, crate::core::auth::MS_TOKEN_URL, &refresh_token).await?;
    let new_ms_refresh = ms_tokens.refresh_token.clone();
    let account = crate::core::auth::xbox_chain(http, ms_tokens).await?;

    // Update the store with the refreshed metadata + new MS refresh token.
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let updated_meta = crate::core::auth::AccountMeta::from_account(&account, now_secs);
    store.set_account(updated_meta, &new_ms_refresh)?;

    Ok(LaunchIdentity {
        player_name: account.username,
        uuid: account.id,
        access_token: account.mc_access_token,
        xuid: account.xuid,
        user_type: "msa".to_string(),
        client_id: crate::core::auth::ms_client_id(),
    })
}

/// Errors from the argv assembler.
#[derive(Debug, PartialEq, Eq)]
pub enum AssembleError {
    /// One or more `${...}` placeholders could not be resolved.
    UnsubstitutedPlaceholders(Vec<String>),
}

impl std::fmt::Display for AssembleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsubstitutedPlaceholders(ps) => {
                write!(f, "unsubstituted placeholders: {}", ps.join(", "))
            }
        }
    }
}

/// Build the full JVM argv for the given `LaunchMeta` + resolved paths + identity.
///
/// Returns `[-Xmx, <-Xms if set>, <extra_args>, <substituted jvm_args>, main_class, <substituted game_args>]`.
///
/// The heap and extra args from `effective` are emitted literally (no `${...}` substitution)
/// and precede the manifest jvm_args so heap/GC settings lead the argument list.
///
/// Any `${...}` placeholder not in the substitution table causes an
/// [`AssembleError::UnsubstitutedPlaceholders`] rather than passing raw text
/// to the JVM.
///
/// When `launch.jvm_args` is empty (legacy manifests), a minimal set of
/// default JVM args is prepended so the JVM can start.
pub fn build_argv(launch: &LaunchMeta, paths: &LaunchPaths, identity: &LaunchIdentity, effective: &crate::core::java_resolve::EffectiveJava) -> Result<Vec<String>, AssembleError> {
    let classpath = build_classpath(&launch.classpath);

    // Choose the asset root: legacy branch points at the virtual tree.
    let effective_assets_root = if launch.assets_legacy {
        paths.legacy_assets_root.to_string_lossy().into_owned()
    } else {
        paths.assets_root.to_string_lossy().into_owned()
    };

    // Build the substitution table — every known vanilla placeholder.
    let subs: &[(&str, String)] = &[
        ("${classpath}", classpath),
        ("${classpath_separator}", {
            #[cfg(target_os = "windows")]
            { ";".to_string() }
            #[cfg(not(target_os = "windows"))]
            { ":".to_string() }
        }),
        ("${natives_directory}", paths.natives_directory.to_string_lossy().into_owned()),
        // ${library_directory} is used by Forge/NeoForge (-DlibraryDirectory=).
        // After C2 materialization, this points at the per-instance libraries dir
        // so Forge reads hardlinked jars from the isolated instance tree.
        ("${library_directory}", paths.library_directory.to_string_lossy().into_owned()),
        ("${launcher_name}", "modloader".to_string()),
        ("${launcher_version}", env!("CARGO_PKG_VERSION").to_string()),
        ("${game_directory}", paths.game_directory.to_string_lossy().into_owned()),
        ("${assets_root}", effective_assets_root.clone()),
        // ${game_assets} is the legacy alias for ${assets_root}.
        ("${game_assets}", effective_assets_root),
        ("${assets_index_name}", launch.asset_index_id.clone()),
        ("${version_name}", launch.version_id.clone()),
        ("${version_type}", launch.version_type.clone()),
        ("${auth_player_name}", identity.player_name.clone()),
        ("${auth_uuid}", identity.uuid.clone()),
        ("${auth_access_token}", identity.access_token.clone()),
        ("${auth_xuid}", identity.xuid.clone()),
        // ${clientid} = MSA application client ID (the `--clientId` telemetry arg
        // in MS-auth version JSONs). Empty for offline identities.
        ("${clientid}", identity.client_id.clone()),
        ("${user_type}", identity.user_type.clone()),
        // ${path} for log4j config — handled specially below (omitted when None).
    ];

    // Effective JVM args: legacy manifests have an empty list — supply defaults.
    let jvm_args = if launch.jvm_args.is_empty() {
        default_jvm_args(&launch.asset_index_id, &paths.natives_directory, &build_classpath(&launch.classpath))
    } else {
        launch.jvm_args.clone()
    };

    // Filter out the log4j arg when logging_config is None; substitute ${path} when Some.
    let jvm_args_filtered = apply_logging_config(jvm_args, &launch.logging_config);

    let mut argv: Vec<String> = Vec::new();
    let mut unresolved: Vec<String> = Vec::new();

    // Emit heap + extra args from effective config (literal, no ${} substitution).
    // These lead the JVM argv so heap/GC flags take precedence over manifest defaults.
    argv.push(format!("-Xmx{}M", effective.xmx_mb));
    if let Some(xms) = effective.xms_mb {
        argv.push(format!("-Xms{}M", xms));
    }
    argv.extend(effective.extra_args.iter().cloned());

    // Substitute + collect jvm_args.
    for arg in &jvm_args_filtered {
        let substituted = substitute(arg, subs);
        collect_unresolved(&substituted, &mut unresolved);
        argv.push(substituted);
    }

    // main_class is literal — no substitution needed.
    argv.push(launch.main_class.clone());

    // Substitute + collect game_args.
    for arg in &launch.game_args {
        let substituted = substitute(arg, subs);
        collect_unresolved(&substituted, &mut unresolved);
        argv.push(substituted);
    }

    if !unresolved.is_empty() {
        return Err(AssembleError::UnsubstitutedPlaceholders(unresolved));
    }

    Ok(argv)
}

/// Join the classpath entries with the OS classpath separator.
///
/// Classpath separator: `:` on unix, `;` on windows.
/// (Distinct from the file-system path separator `std::path::MAIN_SEPARATOR`.)
fn build_classpath(entries: &[String]) -> String {
    #[cfg(target_os = "windows")]
    let sep = ";";
    #[cfg(not(target_os = "windows"))]
    let sep = ":";
    entries.join(sep)
}

/// Perform placeholder substitution on a single arg string.
///
/// Replaces every key in `subs` with its value (left-to-right, one pass).
fn substitute(arg: &str, subs: &[(&str, String)]) -> String {
    let mut result = arg.to_owned();
    for (key, val) in subs {
        if result.contains(key) {
            result = result.replace(key, val);
        }
    }
    result
}

/// After substitution, scan for remaining `${...}` tokens and append to `out`.
fn collect_unresolved(arg: &str, out: &mut Vec<String>) {
    let mut s = arg;
    while let Some(start) = s.find("${") {
        if let Some(end) = s[start..].find('}') {
            let placeholder = &s[start..start + end + 1];
            if !out.contains(&placeholder.to_string()) {
                out.push(placeholder.to_string());
            }
            s = &s[start + end + 1..];
        } else {
            break;
        }
    }
}

/// Filter / substitute the `-Dlog4j.configurationFile=${path}` JVM arg.
///
/// When `logging_config` is `None`, any arg containing `${path}` is dropped.
/// When `Some(p)`, `${path}` is replaced with the config file's path.
fn apply_logging_config(args: Vec<String>, logging_config: &Option<String>) -> Vec<String> {
    match logging_config {
        None => args
            .into_iter()
            .filter(|a| !a.contains("${path}"))
            .collect(),
        Some(config_path) => args
            .into_iter()
            .map(|a| a.replace("${path}", config_path))
            .collect(),
    }
}

/// Minimal JVM args for legacy manifests that omit the `arguments.jvm` block.
///
/// Modern manifests supply these; legacy ones expect the launcher to provide
/// the classpath and natives path at minimum.
fn default_jvm_args(asset_index_id: &str, natives_dir: &Path, classpath: &str) -> Vec<String> {
    vec![
        format!("-Djava.library.path={}", natives_dir.to_string_lossy()),
        format!("-Dminecraft.launcher.brand=modloader"),
        format!("-Dminecraft.launcher.version={}", env!("CARGO_PKG_VERSION")),
        format!("-Dminecraft.client.jar={asset_index_id}"),
        "-cp".to_string(),
        classpath.to_string(),
    ]
}

// ---------------------------------------------------------------------------
// CP2 — Natives extraction
// ---------------------------------------------------------------------------

/// Unpack native entries from each jar in `native_jars` into `natives_dir`.
///
/// For each jar:
/// - Skip directory entries (name ends with `/`).
/// - Skip entries under `META-INF/`.
/// - Refuse (return `Err`) any entry whose resolved path would escape
///   `natives_dir` (zip-slip / `../` traversal attack).
/// - Extract everything else flat into `natives_dir`.
///
/// `natives_dir` is created if absent. It is keyed per-instance (callers
/// supply `<instances>/<slug>/natives/`) so concurrent launches of different
/// instances do not clash.
pub fn extract_natives(native_jars: &[String], natives_dir: &Path) -> Result<(), String> {
    use zip::ZipArchive;

    fs::create_dir_all(natives_dir)
        .map_err(|e| format!("failed to create natives dir {}: {e}", natives_dir.display()))?;
    let dir_canon = natives_dir
        .canonicalize()
        .map_err(|e| format!("failed to canonicalize natives dir {}: {e}", natives_dir.display()))?;

    for jar_path in native_jars {
        let jar = Path::new(jar_path);
        let file = fs::File::open(jar)
            .map_err(|e| format!("failed to open natives jar {}: {e}", jar.display()))?;
        let mut archive = ZipArchive::new(io::BufReader::new(file))
            .map_err(|e| format!("failed to read natives jar {}: {e}", jar.display()))?;

        for i in 0..archive.len() {
            let mut entry = archive
                .by_index(i)
                .map_err(|e| format!("failed to read entry {i} from {}: {e}", jar.display()))?;

            let entry_name = entry.name().to_owned();

            // Skip directory entries.
            if entry_name.ends_with('/') {
                continue;
            }

            // Skip META-INF/ entries.
            if entry_name.starts_with("META-INF/") || entry_name == "META-INF" {
                continue;
            }

            // Traversal guard: resolve against canon dir (using the basename only —
            // natives are extracted flat, ignoring any subdirectory structure inside
            // the jar). We still check the raw entry name for `..` components before
            // using just the basename.
            let entry_path = Path::new(&entry_name);

            // Check the full entry path for traversal components first.
            let full_target = dir_canon.join(entry_path);
            let full_target_norm = normalize_path_launch(&full_target);
            if !full_target_norm.starts_with(&dir_canon) {
                return Err(format!(
                    "traversal refused: entry '{entry_name}' would escape natives dir"
                ));
            }

            // Extract flat: use only the filename component.
            let file_name = entry_path
                .file_name()
                .ok_or_else(|| format!("entry '{entry_name}' has no filename component"))?;

            let out_path = dir_canon.join(file_name);

            let mut out = fs::File::create(&out_path).map_err(|e| {
                format!("failed to create {}: {e}", out_path.display())
            })?;
            io::copy(&mut entry, &mut out)
                .map_err(|e| format!("failed to write {}: {e}", out_path.display()))?;
        }
    }
    Ok(())
}

/// Normalize a path without requiring it to exist on disk.
///
/// Resolves `..` and `.` components lexically. Used for the traversal guard
/// before writing (we can't `canonicalize()` a path that doesn't exist yet).
///
/// Mirrors the `normalize_path` helper in `java.rs` — copied rather than
/// shared to avoid coupling modules across domains.
fn normalize_path_launch(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// CP3 — Spawn + log streaming + running registry + kill + playtime
// ---------------------------------------------------------------------------

/// Log/exit sink trait — mirrors `download::ProgressSink`.
///
/// Implementations: `TauriLaunchSink` (in `lib.rs`) emits `launch://log` and
/// `launch://exit` events; `CapturingLaunchSink` (below, test-only) collects
/// lines into a `Mutex<Vec>` for assertion.
///
/// All methods take `&self` (shared ref) so a single sink can be shared across
/// the stdout and stderr reader tasks.
pub trait LaunchSink: Send + Sync + 'static {
    /// Called for each line read from stdout or stderr.
    /// `stream` is `"stdout"` or `"stderr"`.
    fn log(&self, instance_id: &str, stream: &str, line: &str);

    /// Called once when the child exits (natural or killed).
    /// `code` is `None` if the exit code could not be determined.
    fn exited(&self, instance_id: &str, code: Option<i32>);

    /// Called on each run-status transition (Preparing / Running / terminal).
    /// Implementations emit `run://update`; tests capture the transition.
    ///
    /// Default no-op so existing sinks that only care about logs/exit need not
    /// implement it.
    fn status(&self, _instance_id: &str, _status: RunStatus, _exit_code: Option<i32>) {}
}


/// A [`LaunchSink`] that captures log lines and exit events for test assertion.
#[cfg(test)]
pub struct CapturingLaunchSink {
    pub lines: Mutex<Vec<(String, String, String)>>, // (instance_id, stream, line)
    pub exit_codes: Mutex<Vec<(String, Option<i32>)>>,
    pub statuses: Mutex<Vec<(String, RunStatus, Option<i32>)>>, // (instance_id, status, exit_code)
}

#[cfg(test)]
impl CapturingLaunchSink {
    pub fn new() -> Self {
        Self {
            lines: Mutex::new(Vec::new()),
            exit_codes: Mutex::new(Vec::new()),
            statuses: Mutex::new(Vec::new()),
        }
    }
}

#[cfg(test)]
impl LaunchSink for CapturingLaunchSink {
    fn log(&self, instance_id: &str, stream: &str, line: &str) {
        self.lines
            .lock()
            .unwrap()
            .push((instance_id.to_owned(), stream.to_owned(), line.to_owned()));
    }
    fn exited(&self, instance_id: &str, code: Option<i32>) {
        self.exit_codes
            .lock()
            .unwrap()
            .push((instance_id.to_owned(), code));
    }
    fn status(&self, instance_id: &str, status: RunStatus, exit_code: Option<i32>) {
        self.statuses
            .lock()
            .unwrap()
            .push((instance_id.to_owned(), status, exit_code));
    }
}

// ---------------------------------------------------------------------------
// Running registry
// ---------------------------------------------------------------------------

/// Maximum number of log lines retained per instance in the in-memory ring.
/// Older lines are dropped once this cap is reached (FIFO eviction). Bounds the
/// memory cost of buffering logs for many concurrent packs.
pub const LOG_RING_CAP: usize = 1000;

/// Lifecycle status of a tracked run.
///
/// Prep is serialized (only one `Preparing` at a time via the prep semaphore);
/// `Running` is N-concurrent. The terminal trio — `Exited` / `Killed` / `Failed`
/// — is **retained** in the registry so reads (`get_run_state` / `get_run_logs`)
/// can recover a missed exit code and replay buffered logs after the child is gone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunStatus {
    /// Holding the prep permit: resolve → download → materialize → natives.
    Preparing,
    /// JVM spawned; prep permit released. N of these may coexist.
    Running,
    /// Child exited on its own. `exit_code` carries the code (if known).
    Exited,
    /// Child was killed via `kill_instance`.
    Killed,
    /// Prep failed before the JVM spawned.
    Failed,
}

impl RunStatus {
    /// True for the terminal trio (`Exited` / `Killed` / `Failed`). Terminal
    /// entries are retained but excluded from `list_running`.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            RunStatus::Exited | RunStatus::Killed | RunStatus::Failed
        )
    }

    /// Stable lowercase tag for IPC payloads (`run://update`).
    pub fn as_str(self) -> &'static str {
        match self {
            RunStatus::Preparing => "preparing",
            RunStatus::Running => "running",
            RunStatus::Exited => "exited",
            RunStatus::Killed => "killed",
            RunStatus::Failed => "failed",
        }
    }
}

/// One buffered log line in a run's ring.
#[derive(Clone, Debug)]
pub struct LogLine {
    /// `"stdout"` or `"stderr"`.
    pub stream: String,
    pub line: String,
}

/// Per-instance run state stored in the registry (grown from the old `KillHandle`).
///
/// Holds the kill channel, the lifecycle `status`, the buffered `exit_code`, a
/// capped `log_ring` for replay, and the launch `started` instant for playtime /
/// elapsed reporting. The monitor task writes `status` / `exit_code` / `log_ring`;
/// reads clone the fields they need out of the lock and release it before any await.
pub struct RunState {
    /// Fires once to signal the monitor task to kill the child.
    ///
    /// Wrapped in `Option` so `kill_instance` can `take` the sender (consuming it
    /// to send the signal) while **leaving the registry entry in place**. The
    /// monitor task records the terminal status once the child has actually
    /// exited, eliminating the TOCTOU window where a concurrent `launch_instance`
    /// could re-spawn while the old child is still terminating.
    ///
    /// `None` while `Preparing` (no child yet) and after the kill signal fires.
    pub kill_tx: Option<tokio::sync::oneshot::Sender<()>>,
    /// Current lifecycle status.
    pub status: RunStatus,
    /// Buffered process exit code; `None` until a terminal status is reached (or
    /// if the code could not be determined).
    pub exit_code: Option<i32>,
    /// Capped ring of recent log lines (oldest dropped past [`LOG_RING_CAP`]).
    pub log_ring: std::collections::VecDeque<LogLine>,
    /// When the launch began (prep start). Used for playtime / elapsed reporting.
    pub started: Instant,
}

impl RunState {
    /// New `Preparing` state — no child yet, empty ring.
    pub fn new_preparing() -> Self {
        Self {
            kill_tx: None,
            status: RunStatus::Preparing,
            exit_code: None,
            log_ring: std::collections::VecDeque::new(),
            started: Instant::now(),
        }
    }

    /// Append a log line, evicting the oldest if at capacity.
    pub fn push_log(&mut self, stream: &str, line: &str) {
        if self.log_ring.len() >= LOG_RING_CAP {
            self.log_ring.pop_front();
        }
        self.log_ring.push_back(LogLine {
            stream: stream.to_owned(),
            line: line.to_owned(),
        });
    }
}

/// A cloned, lock-free snapshot of one run's state for read commands.
///
/// Excludes the kill channel and log ring (queried separately via
/// `get_run_logs`); carries just the queryable scalars plus elapsed time.
#[derive(Clone, Debug)]
pub struct RunInfo {
    pub slug: String,
    pub status: RunStatus,
    pub exit_code: Option<i32>,
    /// Milliseconds since the launch began.
    pub elapsed_ms: u128,
}

/// In-process running-instance registry.
///
/// Keyed by instance **slug** (the on-disk directory name), which is unique and
/// stable. Managed as Tauri state. Terminal entries are retained for recovery.
/// **Never hold this lock across an `.await` point.**
pub type RunningRegistry = Mutex<HashMap<String, RunState>>;

/// Construct an empty [`RunningRegistry`] for use with `.manage(...)`.
pub fn new_running_registry() -> RunningRegistry {
    Mutex::new(HashMap::new())
}

/// Prep serialization semaphore — a single permit gates the blocking launch prep
/// (resolve → download → materialize → natives). `launch_instance` holds the
/// permit only across prep and drops it once the JVM spawns, so a second launch
/// waits for prep but both packs then run concurrently. Managed as Tauri state.
pub type PrepSemaphore = Arc<tokio::sync::Semaphore>;

/// Construct a fresh single-permit [`PrepSemaphore`] for use with `.manage(...)`.
pub fn new_prep_semaphore() -> PrepSemaphore {
    Arc::new(tokio::sync::Semaphore::new(1))
}

/// Insert (or reset) a `Preparing` entry for `slug` and emit the status.
///
/// Called by `launch_instance` immediately after acquiring the prep permit, so
/// the (serialized) prep phase has a live registry entry whose log ring collects
/// prep-phase `install://log` lines (attributed to the sole preparing instance).
/// The lock is released before the sink call (never hold across emit).
pub fn mark_preparing<S: LaunchSink>(registry: &RunningRegistry, slug: &str, sink: &S) {
    {
        let mut guard = registry.lock().unwrap();
        guard.insert(slug.to_owned(), RunState::new_preparing());
    }
    sink.status(slug, RunStatus::Preparing, None);
}

/// Mark `slug` as `Failed` (prep error before the JVM spawned) and emit the
/// status. The entry is retained for recovery. Lock released before the emit.
pub fn mark_failed<S: LaunchSink>(registry: &RunningRegistry, slug: &str, sink: &S) {
    {
        let mut guard = registry.lock().unwrap();
        if let Some(state) = guard.get_mut(slug) {
            state.status = RunStatus::Failed;
            state.kill_tx = None;
        }
    }
    sink.status(slug, RunStatus::Failed, None);
}

/// Append a prep-phase log line to the sole `Preparing` instance's ring.
///
/// Prep-phase `install://log` lines carry no correlation id, but prep is
/// serialized (one `Preparing` at a time), so they attribute to whichever
/// instance is currently preparing. No-op if none is preparing.
pub fn record_prep_log(registry: &RunningRegistry, stream: &str, line: &str) {
    let mut guard = registry.lock().unwrap();
    if let Some((_, state)) = guard
        .iter_mut()
        .find(|(_, s)| s.status == RunStatus::Preparing)
    {
        state.push_log(stream, line);
    }
}

/// Snapshot the non-terminal (Preparing / Running) instances.
///
/// Clones scalars out of the lock and releases it before returning — no await
/// while held. Terminal entries are excluded (they are retained for recovery but
/// are not "running").
pub fn list_running(registry: &RunningRegistry) -> Vec<RunInfo> {
    let guard = registry.lock().unwrap();
    guard
        .iter()
        .filter(|(_, s)| !s.status.is_terminal())
        .map(|(slug, s)| RunInfo {
            slug: slug.clone(),
            status: s.status,
            exit_code: s.exit_code,
            elapsed_ms: s.started.elapsed().as_millis(),
        })
        .collect()
}

/// Snapshot a single instance's state (any status, incl. terminal). `None` if
/// the slug is not tracked.
pub fn get_run_state(registry: &RunningRegistry, slug: &str) -> Option<RunInfo> {
    let guard = registry.lock().unwrap();
    guard.get(slug).map(|s| RunInfo {
        slug: slug.to_owned(),
        status: s.status,
        exit_code: s.exit_code,
        elapsed_ms: s.started.elapsed().as_millis(),
    })
}

/// Replay an instance's buffered log lines (clones the ring out of the lock).
/// `None` if the slug is not tracked. Works after exit (entry is retained).
pub fn get_run_logs(registry: &RunningRegistry, slug: &str) -> Option<Vec<LogLine>> {
    let guard = registry.lock().unwrap();
    guard.get(slug).map(|s| s.log_ring.iter().cloned().collect())
}

// ---------------------------------------------------------------------------
// Core spawn
// ---------------------------------------------------------------------------

/// Spawn the JVM described by `argv` (first element = the java binary path),
/// cwd = `game_dir`, piped stdout+stderr.
///
/// Inserts a [`KillHandle`] into `registry` keyed by `slug`.
/// Returns immediately — a background task owns the child and calls
/// `LaunchSink::log` / `LaunchSink::exited` and records playtime on exit.
///
/// # Arguments
/// - `slug`        — instance slug (on-disk dir name); used as the registry key.
/// - `inst_dir`    — per-instance directory (`<instances>/<slug>/`); playtime is
///                   recorded here on exit via `instances::record_playtime`.
/// - `game_dir`    — cwd for the child process (`<instances>/<slug>/mc/`).
/// - `java_path`   — absolute path to the `java`/`java.exe` binary.
/// - `argv`        — full JVM arguments (NOT including the java binary itself).
/// - `registry`    — the shared [`RunningRegistry`] (e.g. from Tauri managed state).
/// - `sink`        — log/exit event sink.
///
/// Returns `Err` if the instance is already in the registry (already running) or
/// if `tokio::process::Command::spawn` fails.
pub async fn spawn_instance<S: LaunchSink>(
    slug: String,
    inst_dir: PathBuf,
    game_dir: PathBuf,
    java_path: PathBuf,
    argv: Vec<String>,
    registry: Arc<RunningRegistry>,
    sink: Arc<S>,
) -> Result<(), String> {
    use tokio::process::Command;

    // Reject only if an instance is already `Running` — acquire lock, check,
    // release before any await. A `Preparing` entry (inserted by `mark_preparing`
    // for this same launch) is the expected predecessor and is transitioned to
    // `Running` below; terminal leftovers are overwritten by the fresh run.
    {
        let guard = registry.lock().unwrap();
        if let Some(state) = guard.get(&slug) {
            if state.status == RunStatus::Running {
                return Err(format!("instance '{slug}' is already running"));
            }
        }
    }

    // Create the kill channel.
    let (kill_tx, kill_rx) = tokio::sync::oneshot::channel::<()>();

    // Create game dir if it doesn't exist yet (first launch).
    fs::create_dir_all(&game_dir)
        .map_err(|e| format!("failed to create game dir {}: {e}", game_dir.display()))?;

    // Spawn the child process.
    let mut child = Command::new(&java_path)
        .args(&argv)
        .current_dir(&game_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            log::error!("launch: failed to spawn java for '{slug}': {e}");
            format!("failed to spawn java: {e}")
        })?;

    let pid = child.id().unwrap_or(0);
    log::info!("launch: spawned '{slug}' (pid {pid})");

    // Take stdout/stderr pipes before moving `child` into the task.
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "failed to capture stdout".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "failed to capture stderr".to_string())?;

    // Transition to `Running` before spawning the monitor task.
    //
    // If a `Preparing` entry exists (the normal launch path via `mark_preparing`)
    // we keep its `started` instant and prep-phase log ring, flipping status to
    // `Running` and installing the kill channel. Direct callers (tests) that
    // skipped `mark_preparing` get a fresh `Running` state. Lock is released
    // before the status emit (never hold across the sink call).
    let started = {
        let mut guard = registry.lock().unwrap();
        match guard.get_mut(&slug) {
            Some(state) => {
                state.status = RunStatus::Running;
                state.kill_tx = Some(kill_tx);
                state.started
            }
            None => {
                let mut state = RunState::new_preparing();
                let started = state.started;
                state.status = RunStatus::Running;
                state.kill_tx = Some(kill_tx);
                guard.insert(slug.clone(), state);
                started
            }
        }
    };
    sink.status(&slug, RunStatus::Running, None);

    // Spawn the monitor task.
    let registry_clone = Arc::clone(&registry);
    let sink_clone = Arc::clone(&sink);
    let slug_clone = slug.clone();

    tokio::spawn(async move {
        monitor_child(
            slug_clone,
            inst_dir,
            child,
            stdout,
            stderr,
            kill_rx,
            started,
            registry_clone,
            sink_clone,
        )
        .await;
    });

    Ok(())
}

/// Monitor a running child: stream stdout+stderr into the log ring + sink, handle
/// natural exit or kill signal, record playtime, and record the terminal status.
///
/// This function is `async` and runs under `tokio::spawn`. It owns the `Child`.
/// It is the **sole owner of the terminal transition** — both the natural-exit and
/// the kill paths record `Exited` / `Killed` + the buffered exit code here, after
/// the child has actually exited. The entry is **retained** (not removed) so reads
/// can recover the exit code and replay logs after the child is gone.
async fn monitor_child<S: LaunchSink>(
    slug: String,
    inst_dir: PathBuf,
    mut child: tokio::process::Child,
    stdout: tokio::process::ChildStdout,
    stderr: tokio::process::ChildStderr,
    kill_rx: tokio::sync::oneshot::Receiver<()>,
    started: Instant,
    registry: Arc<RunningRegistry>,
    sink: Arc<S>,
) {
    use tokio::io::{AsyncBufReadExt, BufReader as AsyncBufReader};

    let mut stdout_reader = AsyncBufReader::new(stdout).lines();
    let mut stderr_reader = AsyncBufReader::new(stderr).lines();

    // Line-reading tasks: we use separate spawned tasks for each stream so
    // `select!` below can poll them without blocking on either pipe.
    //
    // We can't do `tokio::select!` on two `AsyncBufReadExt::lines()` calls and
    // `child.wait()` simultaneously in a clean way without moving the readers.
    // Solution: spawn two line-reader subtasks that send lines into channels;
    // the monitor selects on `child.wait()` and the kill signal.
    let (stdout_tx, mut stdout_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let (stderr_tx, mut stderr_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    // Stdout reader subtask.
    tokio::spawn(async move {
        while let Ok(Some(line)) = stdout_reader.next_line().await {
            if stdout_tx.send(line).is_err() {
                break;
            }
        }
    });

    // Stderr reader subtask.
    tokio::spawn(async move {
        while let Ok(Some(line)) = stderr_reader.next_line().await {
            if stderr_tx.send(line).is_err() {
                break;
            }
        }
    });

    // Pin the kill receiver so it can be polled in a loop without moving.
    tokio::pin!(kill_rx);

    // Drive line draining + child wait + kill signal together.
    //
    // We track which channels are open so closed channels don't spin the loop.
    // When both log channels close, only `child.wait()` and `kill_rx` remain.
    let mut stdout_open = true;
    let mut stderr_open = true;

    // Push a log line into the instance's ring (synchronous; lock never held
    // across an await) AND emit it on the sink. Keeps the buffered replay and the
    // live `launch://log` stream in lock-step.
    let push_and_emit = |stream: &str, line: &str| {
        {
            let mut guard = registry.lock().unwrap();
            if let Some(state) = guard.get_mut(&slug) {
                state.push_log(stream, line);
            }
        }
        sink.log(&slug, stream, line);
    };

    // Did the terminal transition arrive via an explicit kill?
    let mut was_killed = false;

    let exit_status = loop {
        tokio::select! {
            // Drain stdout lines — disabled once the channel is closed.
            line = stdout_rx.recv(), if stdout_open => {
                match line {
                    Some(l) => push_and_emit("stdout", &l),
                    None => stdout_open = false,
                }
            }

            // Drain stderr lines — disabled once the channel is closed.
            line = stderr_rx.recv(), if stderr_open => {
                match line {
                    Some(l) => push_and_emit("stderr", &l),
                    None => stderr_open = false,
                }
            }

            // Natural exit.
            status = child.wait() => {
                let code = status.ok().and_then(|s| s.code());
                break code;
            }

            // Kill signal from kill_instance command.
            // `&mut kill_rx` polls the pinned receiver without consuming it.
            _ = &mut kill_rx => {
                was_killed = true;
                // Signal the child to stop. start_kill() is non-blocking.
                let _ = child.start_kill();
                // Wait for the child to actually terminate.
                let status = child.wait().await.ok().and_then(|s| s.code());
                break status;
            }
        }
    };

    // Drain any remaining lines in the channels after exit.
    while let Ok(line) = stdout_rx.try_recv() {
        push_and_emit("stdout", &line);
    }
    while let Ok(line) = stderr_rx.try_recv() {
        push_and_emit("stderr", &line);
    }

    // Record playtime.
    let elapsed_secs = started.elapsed().as_secs();
    let now = chrono::Utc::now();
    if let Err(e) = crate::core::instances::record_playtime(&inst_dir, elapsed_secs, now) {
        // Best-effort — log but don't panic.
        log::warn!("launch: failed to record playtime for {slug}: {e}");
    }

    // Record the terminal status + exit code — monitor is the sole owner of this
    // transition (both exit paths). The entry is RETAINED so reads can recover the
    // exit code and replay logs after the child is gone. Lock released before emit.
    let terminal = if was_killed {
        RunStatus::Killed
    } else {
        RunStatus::Exited
    };
    {
        let mut guard = registry.lock().unwrap();
        if let Some(state) = guard.get_mut(&slug) {
            state.status = terminal;
            state.exit_code = exit_status;
            state.kill_tx = None;
        }
    }

    // Emit terminal status + the exit event.
    sink.status(&slug, terminal, exit_status);
    sink.exited(&slug, exit_status);
}

// ---------------------------------------------------------------------------
// C2 — classpath/natives rewrite helper (pure, no AppHandle)
// ---------------------------------------------------------------------------

/// Rewrite `launch_meta` classpath and natives entries so the JVM reads
/// materialized artifacts from the per-instance tree instead of the shared cache.
///
/// # What it does
/// For each entry in `classpath` and `natives` that is rooted under `cache_dir`
/// (any subdirectory, not just `libraries/` or `versions/`):
/// - Strips the `cache_dir` prefix to produce a relative path.
/// - Rewrites the entry to `instance_dir.join(rel)`.
/// - Collects `rel` into the returned `Vec<PathBuf>` (the list to pass to
///   [`crate::core::materialize::materialize`]).
///
/// Entries that do NOT live under `cache_dir` are passed through unchanged
/// (defensive — should be none in practice with the current resolver).
///
/// # Arguments
/// - `cache_dir`    — shared cache root (`<data>/cache/`).
/// - `instance_dir` — per-instance directory (`<instances>/<slug>/`).
/// - `launch_meta`  — mutable; classpath and natives fields are rewritten in place.
///
/// # Returns
/// The list of relative paths that must be materialized via
/// `materialize(cache_dir, instance_dir, &rel_paths)` before the JVM spawns.
///
/// This function is pure (no disk I/O, no AppHandle) so it is fully unit-testable.
pub fn rewrite_classpath_for_instance(
    cache_dir: &Path,
    instance_dir: &Path,
    launch_meta: &mut crate::core::resolver::LaunchMeta,
) -> Vec<PathBuf> {
    let mut rel_paths: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    for entry in launch_meta.classpath.iter_mut() {
        let p = Path::new(entry.as_str());
        if let Ok(rel) = p.strip_prefix(cache_dir) {
            let rel_owned = rel.to_path_buf();
            if seen.insert(rel_owned.clone()) {
                rel_paths.push(rel_owned);
            }
            *entry = instance_dir.join(rel).to_string_lossy().into_owned();
        }
        // Entries outside cache_dir are left unchanged (defensive pass-through).
    }

    for entry in launch_meta.natives.iter_mut() {
        let p = Path::new(entry.as_str());
        if let Ok(rel) = p.strip_prefix(cache_dir) {
            let rel_owned = rel.to_path_buf();
            if seen.insert(rel_owned.clone()) {
                rel_paths.push(rel_owned);
            }
            *entry = instance_dir.join(rel).to_string_lossy().into_owned();
        }
    }

    rel_paths
}

/// Send a kill signal to a running instance.
///
/// Fires the kill oneshot without changing the registry entry's status. The
/// monitor task is the sole owner of the terminal transition — it records the
/// `Killed` status + exit code after the child has actually exited (eliminating
/// the TOCTOU window), and the entry is retained for recovery.
///
/// Returns `Ok(())` if the signal was sent. Returns `Err` if the instance is not
/// in the registry or if the kill signal was already sent.
pub fn kill_instance(registry: &RunningRegistry, slug: &str) -> Result<(), String> {
    // Take the sender out of the Option while leaving the entry in the map.
    // The lock is released before any signal is sent (no await here anyway, but
    // honoring the "never hold lock across await" invariant as a style rule).
    let kill_tx = {
        let mut guard = registry.lock().unwrap();
        match guard.get_mut(slug) {
            Some(handle) => handle.kill_tx.take(),
            None => return Err(format!("instance '{slug}' is not running")),
        }
    };

    match kill_tx {
        Some(tx) => {
            // Receiver dropped = child already gone; silently ignore.
            let _ = tx.send(());
            Ok(())
        }
        // kill_tx already taken — kill was already sent; treat as success.
        None => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "launch_tests.rs"]
mod tests;
