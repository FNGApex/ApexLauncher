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

use std::collections::HashMap;
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
    pub assets_root: PathBuf,
    /// Absolute path to the per-instance natives extraction dir.
    /// CP2 will extract native jars here before launch.
    pub natives_directory: PathBuf,
    /// For legacy assets: absolute path to the virtual/legacy asset tree
    /// (e.g. `<data>/assets/virtual/legacy`). Only used when
    /// `LaunchMeta.assets_legacy` is true. Legacy-asset materialization is
    /// handled by CP2; this field selects the path without building it.
    pub legacy_assets_root: PathBuf,
}

impl LaunchPaths {
    /// Construct standard paths from the cache dir and instance slug.
    ///
    /// `cache_dir` — the launcher cache directory (`<data>/cache/`); assets live here.
    /// `instances_dir` — the directory that holds all instance subdirs.
    /// `slug` — the instance slug (subdirectory name under `instances_dir`).
    pub fn new(cache_dir: &Path, instances_dir: &Path, slug: &str) -> Self {
        Self {
            game_directory: instances_dir.join(slug).join("mc"),
            assets_root: cache_dir.join("assets"),
            natives_directory: instances_dir.join(slug).join("natives"),
            legacy_assets_root: cache_dir.join("assets").join("virtual").join("legacy"),
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
/// persisting any changes made to the store (via `add_account`) and for
/// holding any lock around the store for the duration of this call.
pub async fn resolve_launch_identity(
    store: &mut AccountStore,
    http: &dyn AuthHttpClient,
    offline: bool,
) -> Result<LaunchIdentity, AuthError> {
    if offline {
        return Ok(LaunchIdentity::offline());
    }

    let account_meta = match store.get_active_account() {
        None => return Ok(LaunchIdentity::offline()),
        Some(m) => m.clone(),
    };

    // MC token is never persisted — always re-derive via refresh.
    let refresh_token = store.get_refresh_token(&account_meta.id)?;

    let ms_tokens = crate::core::auth::refresh_ms_token(http, crate::core::auth::MS_TOKEN_URL, &refresh_token).await?;
    let new_ms_refresh = ms_tokens.refresh_token.clone();
    let account = crate::core::auth::xbox_chain(http, ms_tokens).await?;

    // Update the store with the refreshed metadata + new MS refresh token.
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let updated_meta = crate::core::auth::AccountMeta::from_account(&account, now_secs);
    store.add_account(updated_meta, &new_ms_refresh)?;

    Ok(LaunchIdentity {
        player_name: account.username,
        uuid: account.id,
        access_token: account.mc_access_token,
        xuid: account.xuid,
        user_type: "msa".to_string(),
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
/// Returns `[<substituted jvm_args>, main_class, <substituted game_args>]`.
///
/// Any `${...}` placeholder not in the substitution table causes an
/// [`AssembleError::UnsubstitutedPlaceholders`] rather than passing raw text
/// to the JVM.
///
/// When `launch.jvm_args` is empty (legacy manifests), a minimal set of
/// default JVM args is prepended so the JVM can start.
pub fn build_argv(launch: &LaunchMeta, paths: &LaunchPaths, identity: &LaunchIdentity) -> Result<Vec<String>, AssembleError> {
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
        // library_directory is not in vanilla manifests but included for safety.
        ("${library_directory}", paths.assets_root.parent()
            .map(|p| p.join("libraries").to_string_lossy().into_owned())
            .unwrap_or_default()),
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
}


/// A [`LaunchSink`] that captures log lines and exit events for test assertion.
#[cfg(test)]
pub struct CapturingLaunchSink {
    pub lines: Mutex<Vec<(String, String, String)>>, // (instance_id, stream, line)
    pub exit_codes: Mutex<Vec<(String, Option<i32>)>>,
}

#[cfg(test)]
impl CapturingLaunchSink {
    pub fn new() -> Self {
        Self {
            lines: Mutex::new(Vec::new()),
            exit_codes: Mutex::new(Vec::new()),
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
}

// ---------------------------------------------------------------------------
// Running registry
// ---------------------------------------------------------------------------

/// A handle stored per running instance in the registry.
pub struct KillHandle {
    /// Fires once to signal the monitor task to kill the child.
    ///
    /// Wrapped in `Option` so `kill_instance` can `take` the sender (consuming it
    /// to send the signal) while **leaving the registry entry in place**. The
    /// monitor task is the sole owner of registry removal — it removes the entry
    /// once the child has actually exited, eliminating the TOCTOU window where a
    /// concurrent `launch_instance` could re-spawn while the old child is still
    /// terminating.
    pub kill_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

/// In-process running-instance registry.
///
/// Keyed by instance **slug** (the on-disk directory name), which is unique and
/// stable. Managed as Tauri state.
/// **Never hold this lock across an `.await` point.**
pub type RunningRegistry = Mutex<HashMap<String, KillHandle>>;

/// Construct an empty [`RunningRegistry`] for use with `.manage(...)`.
pub fn new_running_registry() -> RunningRegistry {
    Mutex::new(HashMap::new())
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

    // Reject if already running — acquire lock, check, release before any await.
    {
        let guard = registry.lock().unwrap();
        if guard.contains_key(&slug) {
            return Err(format!("instance '{slug}' is already running"));
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
        .map_err(|e| format!("failed to spawn java: {e}"))?;

    // Take stdout/stderr pipes before moving `child` into the task.
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "failed to capture stdout".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "failed to capture stderr".to_string())?;

    let started = Instant::now();

    // Register before spawning the monitor task.
    {
        let mut guard = registry.lock().unwrap();
        guard.insert(slug.clone(), KillHandle { kill_tx: Some(kill_tx) });
    }

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

/// Monitor a running child: stream stdout+stderr, handle natural exit or kill signal,
/// record playtime on exit, deregister from the registry.
///
/// This function is `async` and runs under `tokio::spawn`. It owns the `Child`.
/// It is the **sole owner of registry removal** — both the natural-exit and the
/// kill paths call `registry.remove` here, after the child has actually exited.
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

    let exit_status = loop {
        tokio::select! {
            // Drain stdout lines — disabled once the channel is closed.
            line = stdout_rx.recv(), if stdout_open => {
                match line {
                    Some(l) => sink.log(&slug, "stdout", &l),
                    None => stdout_open = false,
                }
            }

            // Drain stderr lines — disabled once the channel is closed.
            line = stderr_rx.recv(), if stderr_open => {
                match line {
                    Some(l) => sink.log(&slug, "stderr", &l),
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
        sink.log(&slug, "stdout", &line);
    }
    while let Ok(line) = stderr_rx.try_recv() {
        sink.log(&slug, "stderr", &line);
    }

    // Record playtime.
    let elapsed_secs = started.elapsed().as_secs();
    let now = chrono::Utc::now();
    if let Err(e) = crate::core::instances::record_playtime(&inst_dir, elapsed_secs, now) {
        // Best-effort — log but don't panic.
        eprintln!("launch: failed to record playtime for {slug}: {e}");
    }

    // Deregister — monitor is the sole owner of this removal (both exit paths).
    {
        let mut guard = registry.lock().unwrap();
        guard.remove(&slug);
    }

    // Emit exit event.
    sink.exited(&slug, exit_status);
}

/// Send a kill signal to a running instance.
///
/// Fires the kill oneshot without removing the registry entry. The monitor task
/// is the sole owner of registry removal — it deregisters after the child has
/// actually exited (eliminating the TOCTOU window).
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
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Offline identity for tests that don't exercise identity routing.
    fn offline_identity() -> LaunchIdentity {
        LaunchIdentity::offline()
    }

    fn make_meta(
        version_type: &str,
        jvm_args: Vec<&str>,
        game_args: Vec<&str>,
        classpath: Vec<&str>,
        assets_legacy: bool,
        logging_config: Option<&str>,
    ) -> LaunchMeta {
        LaunchMeta {
            version_id: "1.21.1".to_string(),
            version_type: version_type.to_string(),
            main_class: "net.minecraft.client.main.Main".to_string(),
            jvm_args: jvm_args.into_iter().map(str::to_owned).collect(),
            game_args: game_args.into_iter().map(str::to_owned).collect(),
            asset_index_id: "17".to_string(),
            assets_legacy,
            java_major: 21,
            classpath: classpath.into_iter().map(str::to_owned).collect(),
            natives: vec!["/data/libraries/native.jar".to_string()],
            logging_config: logging_config.map(str::to_owned),
        }
    }

    fn make_paths() -> LaunchPaths {
        LaunchPaths {
            game_directory: PathBuf::from("/instances/my-world/mc"),
            assets_root: PathBuf::from("/data/assets"),
            natives_directory: PathBuf::from("/instances/my-world/natives"),
            legacy_assets_root: PathBuf::from("/data/assets/virtual/legacy"),
        }
    }

    // -----------------------------------------------------------------------
    // Offline UUID
    // -----------------------------------------------------------------------

    #[test]
    fn offline_uuid_is_deterministic() {
        let a = offline_uuid();
        let b = offline_uuid();
        assert_eq!(a, b);
    }

    #[test]
    fn offline_uuid_pinned_value() {
        // Pin the exact value so a dep change is caught immediately.
        // Computed from uuid::Uuid::new_v3(&Uuid::from_u128(0), b"OfflinePlayer:Player").
        let u = offline_uuid();
        assert_eq!(
            u.as_hyphenated().to_string(),
            "2e5dcd13-3805-3256-b49c-819167bf4871"
        );
    }

    #[test]
    fn offline_uuid_is_version_3() {
        let u = offline_uuid();
        assert_eq!(u.get_version_num(), 3);
    }

    // -----------------------------------------------------------------------
    // Classpath separator
    // -----------------------------------------------------------------------

    #[test]
    fn classpath_separator_matches_os() {
        let entries = vec!["/a/b.jar".to_string(), "/c/d.jar".to_string()];
        let cp = build_classpath(&entries);

        #[cfg(target_os = "windows")]
        assert!(cp.contains(';'), "Windows classpath must use ';'");
        #[cfg(not(target_os = "windows"))]
        assert!(cp.contains(':'), "non-Windows classpath must use ':'");
    }

    // -----------------------------------------------------------------------
    // Full argv assembly: modern manifest (explicit jvm_args)
    // -----------------------------------------------------------------------

    /// Build a LaunchMeta that covers every placeholder category and assert
    /// the exact argv produced by build_argv.
    #[test]
    fn build_argv_modern_all_placeholders_substituted() {
        let jvm_args = vec![
            "-Djava.library.path=${natives_directory}",
            "-cp",
            "${classpath}",
        ];
        let game_args = vec![
            "--username",
            "${auth_player_name}",
            "--version",
            "${version_name}",
            "--gameDir",
            "${game_directory}",
            "--assetsDir",
            "${assets_root}",
            "--assetIndex",
            "${assets_index_name}",
            "--uuid",
            "${auth_uuid}",
            "--accessToken",
            "${auth_access_token}",
            "--userType",
            "${user_type}",
            "--versionType",
            "${version_type}",
        ];
        let cp = vec!["/data/libraries/authlib.jar", "/data/versions/1.21.1/1.21.1.jar"];

        let meta = make_meta("release", jvm_args, game_args, cp, false, None);
        let paths = make_paths();

        let argv = build_argv(&meta, &paths, &offline_identity()).expect("no unresolved placeholders");

        // main_class must be present between jvm and game sections.
        let mc_idx = argv
            .iter()
            .position(|a| a == "net.minecraft.client.main.Main")
            .expect("main_class must be in argv");
        assert!(mc_idx > 0, "main_class must not be first");
        assert!(mc_idx < argv.len() - 1, "main_class must not be last");

        // jvm_args section (before main_class).
        let jvm_section = &argv[..mc_idx];
        assert!(
            jvm_section.iter().any(|a| a.contains("/instances/my-world/natives")),
            "natives_directory must be substituted: {:?}",
            jvm_section
        );
        let cp_idx = jvm_section.iter().position(|a| a == "-cp").expect("-cp must be present");
        let cp_val = &jvm_section[cp_idx + 1];
        assert!(cp_val.contains("authlib.jar"), "classpath must contain authlib.jar: {cp_val}");
        assert!(cp_val.contains("1.21.1.jar"), "classpath must contain client jar: {cp_val}");

        // game_args section (after main_class).
        let game_section = &argv[mc_idx + 1..];
        let game_str = game_section.join(" ");
        assert!(game_str.contains("Player"), "${{auth_player_name}} not substituted");
        assert!(game_str.contains("1.21.1"), "${{version_name}} not substituted");
        assert!(game_str.contains("/instances/my-world/mc"), "${{game_directory}} not substituted");
        assert!(game_str.contains("/data/assets"), "${{assets_root}} not substituted");
        assert!(game_str.contains("17"), "${{assets_index_name}} not substituted");
        assert!(
            game_str.contains("2e5dcd13-3805-3256-b49c-819167bf4871"),
            "${{auth_uuid}} not substituted"
        );
        assert!(game_str.contains('0'), "${{auth_access_token}} not substituted");
        assert!(game_str.contains("msa"), "${{user_type}} not substituted");
        assert!(game_str.contains("release"), "${{version_type}} not substituted");

        // No raw placeholder tokens must survive.
        for arg in &argv {
            assert!(
                !arg.contains("${"),
                "raw placeholder survived in argv: {arg}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Unsubstituted placeholder surfaced as error
    // -----------------------------------------------------------------------

    #[test]
    fn build_argv_unsubstituted_placeholder_is_error() {
        let jvm_args = vec!["${unknown_token}"];
        let meta = make_meta("release", jvm_args, vec![], vec![], false, None);
        let paths = make_paths();

        let err = build_argv(&meta, &paths, &offline_identity()).expect_err("must error on unknown placeholder");
        match err {
            AssembleError::UnsubstitutedPlaceholders(ps) => {
                assert!(
                    ps.iter().any(|p| p == "${unknown_token}"),
                    "error must name the placeholder: {ps:?}"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // Logging config: omit when None; substitute when Some
    // -----------------------------------------------------------------------

    #[test]
    fn build_argv_logging_config_none_omits_path_arg() {
        let jvm_args = vec![
            "-Dlog4j.configurationFile=${path}",
            "-cp",
            "${classpath}",
        ];
        let meta = make_meta(
            "release",
            jvm_args,
            vec![],
            vec!["/data/versions/1.21.1/1.21.1.jar"],
            false,
            None, // no logging config
        );
        let paths = make_paths();

        let argv = build_argv(&meta, &paths, &offline_identity()).expect("no error expected");
        assert!(
            !argv.iter().any(|a| a.contains("log4j")),
            "log4j arg must be omitted when logging_config is None: {argv:?}"
        );
        assert!(
            !argv.iter().any(|a| a.contains("${path}")),
            "raw ${{path}} must not appear in argv: {argv:?}"
        );
    }

    #[test]
    fn build_argv_logging_config_some_substitutes_path() {
        let jvm_args = vec![
            "-Dlog4j.configurationFile=${path}",
            "-cp",
            "${classpath}",
        ];
        let meta = make_meta(
            "release",
            jvm_args,
            vec![],
            vec!["/data/versions/1.21.1/1.21.1.jar"],
            false,
            Some("/data/assets/log_configs/log4j2.xml"),
        );
        let paths = make_paths();

        let argv = build_argv(&meta, &paths, &offline_identity()).expect("no error expected");
        assert!(
            argv.iter().any(|a| a.contains("/data/assets/log_configs/log4j2.xml")),
            "log4j path must be substituted: {argv:?}"
        );
        assert!(
            !argv.iter().any(|a| a.contains("${path}")),
            "raw ${{path}} must not appear: {argv:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Legacy manifest: default JVM args supplied when jvm_args is empty
    // -----------------------------------------------------------------------

    #[test]
    fn build_argv_legacy_manifest_gets_default_jvm_args() {
        // Legacy: jvm_args empty, game_args from minecraftArguments.
        let game_args = vec![
            "--username",
            "${auth_player_name}",
            "--version",
            "${version_name}",
            "--gameDir",
            "${game_directory}",
            "--assetsDir",
            "${assets_root}",
            "--assetIndex",
            "${assets_index_name}",
            "--uuid",
            "${auth_uuid}",
            "--accessToken",
            "${auth_access_token}",
            "--userType",
            "${user_type}",
        ];
        let meta = make_meta(
            "release",
            vec![], // empty jvm_args → legacy
            game_args,
            vec!["/data/libraries/a.jar", "/data/versions/1.8.9/1.8.9.jar"],
            false,
            None,
        );
        let paths = make_paths();

        let argv = build_argv(&meta, &paths, &offline_identity()).expect("no error expected");

        // Defaults must include -cp and classpath.
        let cp_idx = argv.iter().position(|a| a == "-cp").expect("-cp must be injected");
        let cp_val = &argv[cp_idx + 1];
        assert!(
            cp_val.contains("1.8.9.jar"),
            "default classpath must include client jar: {cp_val}"
        );

        // Defaults must include natives dir.
        assert!(
            argv.iter().any(|a| a.contains("natives")),
            "default jvm_args must include natives dir: {argv:?}"
        );

        // No raw placeholders survive.
        for arg in &argv {
            assert!(
                !arg.contains("${"),
                "raw placeholder in legacy argv: {arg}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // assets_legacy branch
    // -----------------------------------------------------------------------

    #[test]
    fn build_argv_assets_legacy_uses_virtual_root() {
        let game_args = vec!["--assetsDir", "${assets_root}"];
        let meta = make_meta(
            "release",
            vec!["-cp", "${classpath}"],
            game_args,
            vec!["/data/versions/1.8.9/1.8.9.jar"],
            true, // legacy
            None,
        );
        let paths = make_paths();

        let argv = build_argv(&meta, &paths, &offline_identity()).expect("no error");
        assert!(
            argv.iter().any(|a| a.contains("virtual/legacy")),
            "legacy assets must point at virtual/legacy: {argv:?}"
        );
    }

    #[test]
    fn build_argv_assets_modern_uses_regular_root() {
        let game_args = vec!["--assetsDir", "${assets_root}"];
        let meta = make_meta(
            "release",
            vec!["-cp", "${classpath}"],
            game_args,
            vec!["/data/versions/1.21.1/1.21.1.jar"],
            false, // modern
            None,
        );
        let paths = make_paths();

        let argv = build_argv(&meta, &paths, &offline_identity()).expect("no error");
        // Modern: uses /data/assets, NOT /data/assets/virtual/legacy.
        assert!(
            argv.iter().any(|a| a == "/data/assets"),
            "modern assets must use /data/assets: {argv:?}"
        );
        assert!(
            !argv.iter().any(|a| a.contains("virtual/legacy")),
            "modern assets must NOT use virtual/legacy: {argv:?}"
        );
    }

    // -----------------------------------------------------------------------
    // version_type field: resolver populates it
    // -----------------------------------------------------------------------

    #[test]
    fn version_type_snapshot_propagates() {
        let jvm_args = vec!["-cp", "${classpath}"];
        let game_args = vec!["--versionType", "${version_type}"];
        let meta = make_meta(
            "snapshot",
            jvm_args,
            game_args,
            vec!["/data/versions/24w01a/24w01a.jar"],
            false,
            None,
        );
        let paths = make_paths();

        let argv = build_argv(&meta, &paths, &offline_identity()).expect("no error");
        assert!(
            argv.iter().any(|a| a == "snapshot"),
            "snapshot version_type must appear in argv: {argv:?}"
        );
    }

    // -----------------------------------------------------------------------
    // CP2 (neoforge-forge-launch): forge placeholder regression
    //
    // Confirms that the three placeholders used in Forge/NeoForge JVM args but
    // not present in vanilla manifests are correctly handled by the existing
    // substitution table (launch.rs:219-244).  No new logic — pure regression.
    // -----------------------------------------------------------------------

    #[test]
    fn build_argv_forge_library_directory_substituted() {
        // Forge JVM args include: -DlibraryDirectory=${library_directory}
        // ${library_directory} is resolved to <assets_root_parent>/libraries.
        let jvm_args = vec![
            "-DlibraryDirectory=${library_directory}",
            "-cp",
            "${classpath}",
        ];
        let meta = make_meta(
            "release",
            jvm_args,
            vec![],
            vec!["/data/versions/1.21.1/1.21.1.jar"],
            false,
            None,
        );
        let paths = make_paths();
        // assets_root = /data/assets → parent = /data → libraries = /data/libraries

        let argv = build_argv(&meta, &paths, &offline_identity()).expect("no unresolved");

        let lib_dir_arg = argv
            .iter()
            .find(|a| a.starts_with("-DlibraryDirectory="))
            .expect("-DlibraryDirectory arg must be in argv");
        assert!(
            lib_dir_arg.ends_with("libraries"),
            "${{library_directory}} must resolve to .../libraries: {lib_dir_arg}"
        );
        assert!(
            !lib_dir_arg.contains("${library_directory}"),
            "${{library_directory}} must be fully substituted: {lib_dir_arg}"
        );
    }

    #[test]
    fn build_argv_forge_classpath_separator_substituted() {
        // Forge JVM args can include ${classpath_separator} for manual classpath assembly.
        let jvm_args = vec![
            "-DcpSep=${classpath_separator}",
            "-cp",
            "${classpath}",
        ];
        let meta = make_meta(
            "release",
            jvm_args,
            vec![],
            vec!["/data/versions/1.21.1/1.21.1.jar"],
            false,
            None,
        );
        let paths = make_paths();

        let argv = build_argv(&meta, &paths, &offline_identity()).expect("no unresolved");

        let sep_arg = argv
            .iter()
            .find(|a| a.starts_with("-DcpSep="))
            .expect("-DcpSep arg must be in argv");
        assert!(
            !sep_arg.contains("${classpath_separator}"),
            "${{classpath_separator}} must be fully substituted: {sep_arg}"
        );
        // Value must be OS-appropriate: ':' on non-Windows, ';' on Windows.
        #[cfg(target_os = "windows")]
        assert_eq!(sep_arg, "-DcpSep=;");
        #[cfg(not(target_os = "windows"))]
        assert_eq!(sep_arg, "-DcpSep=:");
    }

    #[test]
    fn build_argv_forge_version_name_substituted() {
        // Forge game args include ${version_name} for FML bootstrap.
        let game_args = vec!["--fml.mcVersion", "${version_name}"];
        let meta = make_meta(
            "release",
            vec!["-cp", "${classpath}"],
            game_args,
            vec!["/data/versions/1.21.1/1.21.1.jar"],
            false,
            None,
        );
        let paths = make_paths();

        let argv = build_argv(&meta, &paths, &offline_identity()).expect("no unresolved");

        let mc_version = argv
            .iter()
            .position(|a| a == "--fml.mcVersion")
            .and_then(|i| argv.get(i + 1))
            .expect("--fml.mcVersion value must be present in argv");
        assert_eq!(
            mc_version, "1.21.1",
            "${{version_name}} must resolve to version_id"
        );
    }

    // -----------------------------------------------------------------------
    // CP2 — extract_natives
    // -----------------------------------------------------------------------

    /// Build an in-memory zip with three kinds of entries:
    ///   - a normal native file (`libfoo.so`)
    ///   - a `META-INF/MANIFEST.MF` entry (must be skipped)
    ///   - a directory entry `natives/` (must be skipped)
    ///
    /// Written to `dest_file` on disk.
    fn make_natives_jar(dest_file: &std::path::Path) {
        use std::io::Write as _;
        use zip::write::{FileOptions, ZipWriter};
        use zip::CompressionMethod;

        let file = fs::File::create(dest_file).unwrap();
        let mut zip = ZipWriter::new(file);
        let opts: FileOptions<'_, ()> =
            FileOptions::default().compression_method(CompressionMethod::Deflated);

        // Normal native binary.
        zip.start_file("libfoo.so", opts).unwrap();
        zip.write_all(b"\x7fELF native content").unwrap();

        // META-INF directory entry.
        zip.add_directory("META-INF/", opts).unwrap();

        // META-INF/MANIFEST.MF — must be skipped.
        zip.start_file("META-INF/MANIFEST.MF", opts).unwrap();
        zip.write_all(b"Manifest-Version: 1.0\n").unwrap();

        // Plain directory entry inside the jar — must be skipped.
        zip.add_directory("natives/", opts).unwrap();

        zip.finish().unwrap();
    }

    /// Build a zip with a traversal entry (`../escape.so`).
    fn make_malicious_natives_jar(dest_file: &std::path::Path) {
        use std::io::Write as _;
        use zip::write::{FileOptions, ZipWriter};
        use zip::CompressionMethod;

        let file = fs::File::create(dest_file).unwrap();
        let mut zip = ZipWriter::new(file);
        let opts: FileOptions<'_, ()> =
            FileOptions::default().compression_method(CompressionMethod::Deflated);

        zip.start_file("../escape.so", opts).unwrap();
        zip.write_all(b"evil payload").unwrap();

        zip.finish().unwrap();
    }

    #[test]
    fn extract_natives_normal_entry_lands_in_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let jar_path = tmp.path().join("natives.jar");
        make_natives_jar(&jar_path);

        let natives_dir = tmp.path().join("natives");
        extract_natives(&[jar_path.to_string_lossy().into_owned()], &natives_dir).unwrap();

        // libfoo.so must be extracted.
        let extracted = natives_dir.join("libfoo.so");
        assert!(extracted.exists(), "libfoo.so must be extracted: {:?}", extracted);

        // Content must match.
        let content = fs::read(&extracted).unwrap();
        assert_eq!(content, b"\x7fELF native content");
    }

    #[test]
    fn extract_natives_meta_inf_skipped() {
        let tmp = tempfile::TempDir::new().unwrap();
        let jar_path = tmp.path().join("natives.jar");
        make_natives_jar(&jar_path);

        let natives_dir = tmp.path().join("natives");
        extract_natives(&[jar_path.to_string_lossy().into_owned()], &natives_dir).unwrap();

        // META-INF/MANIFEST.MF must NOT be extracted.
        assert!(
            !natives_dir.join("META-INF").exists(),
            "META-INF dir must not be created"
        );
        assert!(
            !natives_dir.join("MANIFEST.MF").exists(),
            "MANIFEST.MF must not be extracted even flat"
        );
    }

    #[test]
    fn extract_natives_traversal_refused() {
        let tmp = tempfile::TempDir::new().unwrap();
        let jar_path = tmp.path().join("malicious.jar");
        make_malicious_natives_jar(&jar_path);

        let natives_dir = tmp.path().join("natives");
        let result = extract_natives(&[jar_path.to_string_lossy().into_owned()], &natives_dir);

        assert!(result.is_err(), "traversal entry must be refused");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("traversal refused"),
            "error must mention 'traversal refused': {msg}"
        );

        // The file must not have been written outside the target dir.
        let escape_target = tmp.path().join("escape.so");
        assert!(
            !escape_target.exists(),
            "malicious file must not exist outside natives_dir"
        );
    }

    // -----------------------------------------------------------------------
    // CP3 — playtime accounting (unit, no JVM)
    // -----------------------------------------------------------------------

    /// Helper: write a minimal instance.json into a TempDir and return the dir.
    fn make_instance_dir(tmp: &tempfile::TempDir, initial_playtime: u64) -> std::path::PathBuf {
        use crate::core::instances::{Instance, JavaCfg, Loader, SCHEMA_VERSION};
        use std::io::Write as _;

        let inst = Instance {
            schema: SCHEMA_VERSION,
            id: "test-id-1234".to_string(),
            name: "Test Instance".to_string(),
            slug: "test-instance".to_string(),
            icon: None,
            minecraft: "1.21.1".to_string(),
            loader: Loader {
                kind: "vanilla".to_string(),
                version: None,
            },
            java: JavaCfg {
                major: None,
                args_override: None,
                memory_mb: 2048,
            },
            source: None,
            mods: vec![],
            created: "2024-01-01T00:00:00+00:00".to_string(),
            last_played: None,
            total_playtime_sec: initial_playtime,
        };

        let json = serde_json::to_string_pretty(&inst).unwrap();
        let path = tmp.path().join("instance.json");
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(json.as_bytes()).unwrap();
        tmp.path().to_path_buf()
    }

    #[test]
    fn playtime_record_increments_and_sets_last_played() {
        use crate::core::instances::{record_playtime, read_manifest_pub};

        let tmp = tempfile::TempDir::new().unwrap();
        let inst_dir = make_instance_dir(&tmp, 100);

        let fake_now = chrono::TimeZone::timestamp_opt(&chrono::Utc, 1_700_000_000, 0).unwrap();
        let elapsed = 3661u64; // 1h 1m 1s

        record_playtime(&inst_dir, elapsed, fake_now).expect("record_playtime failed");

        let inst = read_manifest_pub(&inst_dir.join("instance.json"))
            .expect("manifest must be readable after record");

        assert_eq!(
            inst.total_playtime_sec,
            100 + 3661,
            "total_playtime_sec must have incremented"
        );
        assert_eq!(
            inst.last_played.as_deref(),
            Some("2023-11-14T22:13:20+00:00"),
            "last_played must be set to the injected now"
        );
    }

    #[test]
    fn playtime_record_accumulates_across_calls() {
        use crate::core::instances::{record_playtime, read_manifest_pub};

        let tmp = tempfile::TempDir::new().unwrap();
        let inst_dir = make_instance_dir(&tmp, 0);
        let fake_now = chrono::TimeZone::timestamp_opt(&chrono::Utc, 1_700_000_000, 0).unwrap();

        record_playtime(&inst_dir, 60, fake_now).unwrap();
        record_playtime(&inst_dir, 120, fake_now).unwrap();

        let inst = read_manifest_pub(&inst_dir.join("instance.json")).unwrap();
        assert_eq!(inst.total_playtime_sec, 180, "two calls must accumulate");
    }

    // -----------------------------------------------------------------------
    // CP3 — spawn/monitor smoke (no real JVM, trivial process)
    // -----------------------------------------------------------------------

    /// Run the full spawn_instance → monitor → playtime cycle with a trivial process.
    /// On Windows (the actual test host) we use `cmd /c echo hello && cmd /c exit 0`.
    /// On Unix we use `sh -c "echo hello"`.
    #[tokio::test]
    async fn spawn_monitor_smoke_process_exits_playtime_recorded_registry_cleared() {
        use crate::core::instances::read_manifest_pub;
        use std::sync::Arc;

        let tmp = tempfile::TempDir::new().unwrap();
        let inst_dir = make_instance_dir(&tmp, 0);

        // game_dir must exist (spawn_instance creates it, but create here to be safe).
        let game_dir = tmp.path().join("mc");
        fs::create_dir_all(&game_dir).unwrap();

        let sink = Arc::new(CapturingLaunchSink::new());
        let registry = Arc::new(new_running_registry());

        // Choose a trivial cross-platform process.
        #[cfg(windows)]
        let (java_path, argv) = {
            let java = PathBuf::from("cmd.exe");
            let args = vec!["/c".to_string(), "echo hello".to_string()];
            (java, args)
        };
        #[cfg(not(windows))]
        let (java_path, argv) = {
            let java = PathBuf::from("sh");
            let args = vec!["-c".to_string(), "echo hello".to_string()];
            (java, args)
        };

        let slug = "smoke-test-instance".to_string();

        spawn_instance(
            slug.clone(),
            inst_dir.clone(),
            game_dir,
            java_path,
            argv,
            Arc::clone(&registry),
            Arc::clone(&sink),
        )
        .await
        .expect("spawn must succeed");

        // Registry must contain the instance immediately after spawn.
        assert!(
            registry.lock().unwrap().contains_key(&slug),
            "registry must have entry after spawn"
        );

        // Wait for the process to exit — poll with a timeout.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            if !registry.lock().unwrap().contains_key(&slug) {
                break;
            }
            if std::time::Instant::now() > deadline {
                panic!("process did not exit within 10s");
            }
        }

        // Registry cleared.
        assert!(
            !registry.lock().unwrap().contains_key(&slug),
            "registry must be cleared after process exits"
        );

        // Sink received at least one line containing "hello".
        let lines = sink.lines.lock().unwrap();
        assert!(
            lines
                .iter()
                .any(|(_, _, line)| line.to_lowercase().contains("hello")),
            "sink must have received a line containing 'hello': {lines:?}"
        );

        // Exit code received.
        let exits = sink.exit_codes.lock().unwrap();
        assert_eq!(exits.len(), 1, "exactly one exit event");

        // Playtime persisted.
        let inst = read_manifest_pub(&inst_dir.join("instance.json"))
            .expect("manifest must be readable after smoke");
        assert!(
            inst.last_played.is_some(),
            "last_played must be set after process exits"
        );
    }

    // -----------------------------------------------------------------------
    // CP3 — already-running rejection
    // -----------------------------------------------------------------------

    #[test]
    fn kill_instance_not_running_returns_err() {
        let registry = new_running_registry();
        let result = kill_instance(&registry, "not-running-id");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not running"));
    }

    /// Assert that after `kill_instance` fires, the registry entry is still
    /// present (the monitor owns removal), and that playtime is recorded and
    /// the entry is gone only after the child actually exits.
    ///
    /// Uses a long-running `sleep` process so we can fire the kill before exit.
    #[tokio::test]
    async fn kill_leaves_entry_until_monitor_removes_it() {
        use crate::core::instances::read_manifest_pub;
        use std::sync::Arc;

        let tmp = tempfile::TempDir::new().unwrap();
        let inst_dir = make_instance_dir(&tmp, 0);
        let game_dir = tmp.path().join("mc");
        fs::create_dir_all(&game_dir).unwrap();

        let sink = Arc::new(CapturingLaunchSink::new());
        let registry = Arc::new(new_running_registry());

        // A process that sleeps long enough we can kill it mid-run.
        #[cfg(windows)]
        let (java_path, argv) = {
            let java = PathBuf::from("cmd.exe");
            let args = vec!["/c".to_string(), "ping -n 30 127.0.0.1 >nul".to_string()];
            (java, args)
        };
        #[cfg(not(windows))]
        let (java_path, argv) = {
            let java = PathBuf::from("sh");
            let args = vec!["-c".to_string(), "sleep 30".to_string()];
            (java, args)
        };

        let slug = "kill-test-instance".to_string();

        spawn_instance(
            slug.clone(),
            inst_dir.clone(),
            game_dir,
            java_path,
            argv,
            Arc::clone(&registry),
            Arc::clone(&sink),
        )
        .await
        .expect("spawn must succeed");

        // Entry must be in registry right after spawn.
        assert!(
            registry.lock().unwrap().contains_key(&slug),
            "registry must have entry after spawn"
        );

        // Fire the kill signal — must NOT remove the entry immediately.
        kill_instance(&registry, &slug).expect("kill must succeed while running");

        // Entry must STILL be present right after kill (monitor hasn't exited yet).
        assert!(
            registry.lock().unwrap().contains_key(&slug),
            "registry entry must persist immediately after kill (monitor owns removal)"
        );

        // Wait for the monitor to remove the entry (child terminates after kill).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            if !registry.lock().unwrap().contains_key(&slug) {
                break;
            }
            if std::time::Instant::now() > deadline {
                panic!("registry entry was not removed within 10s after kill");
            }
        }

        // Entry gone — monitor removed it after child exited.
        assert!(
            !registry.lock().unwrap().contains_key(&slug),
            "registry must be cleared after monitor confirms exit"
        );

        // Playtime must have been recorded on the kill path.
        let inst = read_manifest_pub(&inst_dir.join("instance.json"))
            .expect("manifest must be readable after kill");
        assert!(
            inst.last_played.is_some(),
            "last_played must be set after kill path"
        );

        // Exit event emitted.
        let exits = sink.exit_codes.lock().unwrap();
        assert_eq!(exits.len(), 1, "exactly one exit event after kill");
    }

    // -----------------------------------------------------------------------
    // CP4 — identity routing in build_argv
    // -----------------------------------------------------------------------

    fn make_identity_meta() -> LaunchMeta {
        // Minimal meta that exercises the identity placeholders.
        make_meta(
            "release",
            vec!["-cp", "${classpath}"],
            vec![
                "--username", "${auth_player_name}",
                "--uuid", "${auth_uuid}",
                "--accessToken", "${auth_access_token}",
                "--userType", "${user_type}",
                "--clientId", "${auth_xuid}",
            ],
            vec!["/data/versions/1.21.1/1.21.1.jar"],
            false,
            None,
        )
    }

    /// Online identity: argv must contain the account's username, uuid, access_token.
    #[test]
    fn cp4_online_identity_in_argv() {
        let meta = make_identity_meta();
        let paths = make_paths();

        let identity = LaunchIdentity {
            player_name: "TruePlayer".to_string(),
            uuid: "00112233-4455-6677-8899-aabbccddeeff".to_string(),
            access_token: "real_mc_token_xyz".to_string(),
            xuid: "xuid_online_999".to_string(),
            user_type: "msa".to_string(),
        };

        let argv = build_argv(&meta, &paths, &identity).expect("no unresolved placeholders");
        let joined = argv.join(" ");

        assert!(
            joined.contains("TruePlayer"),
            "argv must contain account username: {argv:?}"
        );
        assert!(
            joined.contains("00112233-4455-6677-8899-aabbccddeeff"),
            "argv must contain account uuid: {argv:?}"
        );
        assert!(
            joined.contains("real_mc_token_xyz"),
            "argv must contain access_token: {argv:?}"
        );
        assert!(
            joined.contains("xuid_online_999"),
            "argv must contain xuid: {argv:?}"
        );
        // Must NOT contain the offline constants as standalone tokens.
        // (Use exact token match rather than substring to avoid false positives
        //  when the online player name happens to contain "Player" as a substring.)
        assert!(
            !argv.iter().any(|a| a.as_str() == OFFLINE_PLAYER_NAME),
            "argv must not contain offline player name as an exact token when online: {argv:?}"
        );
        assert!(
            !joined.contains("2e5dcd13-3805-3256-b49c-819167bf4871"),
            "argv must not contain offline UUID when online: {argv:?}"
        );
    }

    /// Offline identity: argv must contain OFFLINE_PLAYER_NAME and offline_uuid().
    #[test]
    fn cp4_offline_identity_in_argv() {
        let meta = make_identity_meta();
        let paths = make_paths();
        let identity = LaunchIdentity::offline();

        let argv = build_argv(&meta, &paths, &identity).expect("no error");
        let joined = argv.join(" ");

        assert!(
            joined.contains(OFFLINE_PLAYER_NAME),
            "argv must contain offline player name: {argv:?}"
        );
        assert!(
            joined.contains("2e5dcd13-3805-3256-b49c-819167bf4871"),
            "argv must contain offline UUID: {argv:?}"
        );
        assert!(
            // access_token "0" appears somewhere in the argv
            argv.iter().any(|a| a == "0"),
            "argv must contain token '0' for offline: {argv:?}"
        );
    }

    /// ${auth_xuid} must be in the substitution table — not left as a raw placeholder.
    #[test]
    fn cp4_auth_xuid_placeholder_is_substituted() {
        let meta = make_identity_meta();
        let paths = make_paths();

        let identity = LaunchIdentity {
            player_name: "AnyPlayer".to_string(),
            uuid: "aaaabbbb-0000-0000-0000-ccccddddeeee".to_string(),
            access_token: "tok".to_string(),
            xuid: "xuid_check_123".to_string(),
            user_type: "msa".to_string(),
        };

        let argv = build_argv(&meta, &paths, &identity).expect("no error");
        // No raw placeholder must survive.
        for arg in &argv {
            assert!(
                !arg.contains("${auth_xuid}"),
                "raw ${{auth_xuid}} must not appear in argv: {arg}"
            );
        }
        // The xuid value must appear.
        assert!(
            argv.iter().any(|a| a.contains("xuid_check_123")),
            "xuid must be substituted into argv: {argv:?}"
        );
    }

    // -----------------------------------------------------------------------
    // CP4 — resolve_launch_identity routing
    //
    // Tests use a mock AuthHttpClient and an AccountStore backed by a FakeKeyring
    // (in-memory, no real keyring) + TempDir (no persistent file I/O side effects
    // that would cross test isolation). No live HTTP in any test.
    // -----------------------------------------------------------------------

    use crate::core::auth::{AccountMeta, AccountStore, AuthError, AuthHttpClient, KeyringBackend};
    use std::collections::{HashMap as StdHashMap, VecDeque};
    use std::sync::Mutex as StdMutex;
    use tokio::sync::Mutex as TokioMutex;
    use tempfile::TempDir;

    /// In-memory keyring for tests — no OS keychain calls.
    struct FakeKeyring {
        store: StdMutex<StdHashMap<String, String>>,
    }

    impl FakeKeyring {
        fn new() -> Self {
            FakeKeyring { store: StdMutex::new(StdHashMap::new()) }
        }
    }

    impl KeyringBackend for FakeKeyring {
        fn store_secret(&self, id: &str, secret: &str) -> Result<(), AuthError> {
            self.store.lock().unwrap().insert(id.to_owned(), secret.to_owned());
            Ok(())
        }
        fn load_secret(&self, id: &str) -> Result<String, AuthError> {
            self.store
                .lock()
                .unwrap()
                .get(id)
                .cloned()
                .ok_or_else(|| AuthError::Keyring(format!("no secret for {id}")))
        }
        fn delete_secret(&self, id: &str) -> Result<(), AuthError> {
            self.store.lock().unwrap().remove(id);
            Ok(())
        }
    }

    /// Canned HTTP response.
    struct MockResp(u16, String);

    impl MockResp {
        fn ok(body: impl Into<String>) -> Self { MockResp(200, body.into()) }
    }

    /// Mock HTTP client — pops responses in FIFO order regardless of method.
    struct MockAuthClient {
        responses: std::sync::Arc<TokioMutex<VecDeque<MockResp>>>,
    }

    impl MockAuthClient {
        fn new(responses: Vec<MockResp>) -> Self {
            Self { responses: std::sync::Arc::new(TokioMutex::new(responses.into_iter().collect())) }
        }

        async fn pop(&self) -> (u16, String) {
            let mut q = self.responses.lock().await;
            let MockResp(s, b) = q.pop_front().expect("MockAuthClient: no more canned responses");
            (s, b)
        }
    }

    #[async_trait::async_trait]
    impl AuthHttpClient for MockAuthClient {
        async fn post_form(&self, _url: &str, _params: &[(&str, &str)]) -> Result<(u16, String), reqwest::Error> {
            Ok(self.pop().await)
        }
        async fn post_json(&self, _url: &str, _body: serde_json::Value) -> Result<(u16, String), reqwest::Error> {
            Ok(self.pop().await)
        }
        async fn get_bearer(&self, _url: &str, _token: &str) -> Result<(u16, String), reqwest::Error> {
            Ok(self.pop().await)
        }
    }

    /// Full Xbox-chain success: MS refresh → MS tokens → XBL → XSTS → MC token → profile.
    fn xbox_chain_responses() -> Vec<MockResp> {
        vec![
            // refresh_ms_token: returns MS tokens
            MockResp::ok(r#"{"access_token":"ms_access","refresh_token":"ms_refresh_new","expires_in":3600}"#),
            // XBL authenticate
            MockResp::ok(r#"{"Token":"xbl_tok","DisplayClaims":{"xui":[{"uhs":"uhs_val"}]}}"#),
            // XSTS authorize
            MockResp::ok(r#"{"Token":"xsts_tok","DisplayClaims":{"xui":[{"xid":"xuid_abc"}]}}"#),
            // MC login_with_xbox
            MockResp::ok(r#"{"username":"ignored","access_token":"mc_tok_fresh","token_type":"Bearer","expires_in":86400}"#),
            // MC profile
            MockResp::ok(r#"{"id":"uuid1234","name":"OnlinePlayer","skins":[],"capes":[]}"#),
        ]
    }

    fn make_store_with_account(dir: &TempDir, account_id: &str, refresh_token: &str) -> AccountStore {
        let path = dir.path().join("accounts.json");
        let mut store = AccountStore::load(path, Box::new(FakeKeyring::new()))
            .expect("AccountStore::load should succeed");
        let meta = AccountMeta {
            id: account_id.to_owned(),
            username: "SomePlayer".to_owned(),
            xuid: "xuid_old".to_owned(),
            mc_token_expires: None, // never cached → always refresh
        };
        store.add_account(meta, refresh_token).expect("add_account");
        store.set_active_account(account_id).expect("set_active");
        store
    }

    /// The MC profile fixture returns `"id": "uuid1234"` — this is the account id
    /// that `xbox_chain` returns in `Account.id`. The store account id must match
    /// so that `add_account` (which replaces by id) updates the same entry and
    /// `get_active_account` (which looks up by `active_account_id`) still finds it.
    const FIXTURE_ACCOUNT_ID: &str = "uuid1234";

    /// offline = true → returns offline identity regardless of store contents.
    #[tokio::test]
    async fn cp4_resolve_offline_flag_returns_offline_identity() {
        let dir = TempDir::new().unwrap();
        // Store has an active account, but offline flag overrides.
        let mut store = make_store_with_account(&dir, "acc-1", "rt_unused");
        let http = MockAuthClient::new(vec![]); // no HTTP calls expected

        let identity = resolve_launch_identity(&mut store, &http, true)
            .await
            .expect("offline resolve must not error");

        assert_eq!(identity.player_name, OFFLINE_PLAYER_NAME);
        assert_eq!(
            identity.uuid,
            offline_uuid().as_hyphenated().to_string(),
            "offline uuid must match"
        );
        assert_eq!(identity.access_token, "0");
    }

    /// No active account → offline identity (no HTTP calls).
    #[tokio::test]
    async fn cp4_resolve_no_active_account_returns_offline() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("accounts.json");
        let mut store = AccountStore::load(path, Box::new(FakeKeyring::new()))
            .expect("load");
        let http = MockAuthClient::new(vec![]); // no HTTP calls expected

        let identity = resolve_launch_identity(&mut store, &http, false)
            .await
            .expect("no-account resolve must not error");

        assert_eq!(identity.player_name, OFFLINE_PLAYER_NAME);
    }

    /// Active account present → performs full refresh, returns online identity.
    /// Asserts: username/uuid/xuid from the chain, fresh MC token in identity.
    #[tokio::test]
    async fn cp4_resolve_active_account_refresh_at_launch() {
        let dir = TempDir::new().unwrap();
        // Account id must match the MC profile fixture's `"id"` field ("uuid1234")
        // so that add_account replaces the existing entry (same id) and
        // get_active_account still resolves after the refresh.
        let mut store = make_store_with_account(&dir, FIXTURE_ACCOUNT_ID, "stored_refresh_tok");
        let http = MockAuthClient::new(xbox_chain_responses());

        let identity = resolve_launch_identity(&mut store, &http, false)
            .await
            .expect("online resolve must succeed");

        assert_eq!(identity.player_name, "OnlinePlayer", "username from xbox chain");
        assert_eq!(identity.uuid, "uuid1234", "uuid from MC profile");
        assert_eq!(identity.xuid, "xuid_abc", "xuid from XSTS claims");
        assert_eq!(identity.access_token, "mc_tok_fresh", "fresh MC token from chain");
        assert_eq!(identity.user_type, "msa");

        // Offline constants must not appear.
        assert_ne!(identity.player_name, OFFLINE_PLAYER_NAME);
        assert_ne!(
            identity.uuid,
            offline_uuid().as_hyphenated().to_string()
        );

        // Store must have been updated with refreshed metadata.
        let updated = store.get_active_account().expect("active account still set");
        assert_eq!(updated.username, "OnlinePlayer", "store updated with new username");
        assert_eq!(updated.xuid, "xuid_abc", "store updated with new xuid");

        // New MS refresh token must be in keyring under the account id.
        let new_rt = store.get_refresh_token(FIXTURE_ACCOUNT_ID).expect("refresh token in keyring");
        assert_eq!(new_rt, "ms_refresh_new", "keyring updated with new MS refresh token");
    }
}
