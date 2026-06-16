//! Headless Forge / NeoForge installer runner — Phase 4, slice B, CP1.
//!
//! Runs the official installer jar once per loader version to produce the
//! patched client artifacts and loader `version.json`. Install is idempotent:
//! if `versions/<id>/<id>.json` already exists in `data_dir`, the function
//! returns immediately without spawning the JVM.
//!
//! ## URL patterns
//!
//! | Loader | Maven repo | Coordinate |
//! |--------|-----------|------------|
//! | NeoForge | `https://maven.neoforged.net/releases` | `net.neoforged:neoforge:<v>` |
//! | Forge    | `https://maven.minecraftforge.net`    | `net.minecraftforge:forge:<mc_ver>-<v>` |
//!
//! The installer jar filename follows the Maven convention for each coordinate;
//! [`installer_url`] and [`installer_jar_name`] are the pure, unit-testable
//! helpers that produce those strings.
//!
//! ## Spawn seam
//!
//! The JVM spawn is injectable via a `SpawnFn` closure, enabling unit tests to
//! assert the exact argv and working dir without a live JVM. The closure
//! receives `(java_bin, args, working_dir)` and returns an async result.
//!
//! ## Install-log sink
//!
//! [`InstallSink`] is a trait parallel to `launch::LaunchSink`. Implementors
//! receive one line at a time from the installer's stdout/stderr. The
//! `TauriInstallSink` in `lib.rs` emits `install://log` events; tests use
//! `CapturingInstallSink`.

use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::Mutex;

use crate::core::loader_profile::maven_coord_to_path;

// ---------------------------------------------------------------------------
// Loader kind
// ---------------------------------------------------------------------------

/// Supported headless-installer loader kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallerLoaderKind {
    NeoForge,
    Forge,
}

// ---------------------------------------------------------------------------
// Pure helpers — Maven URL / filename construction
// ---------------------------------------------------------------------------

/// Build the installer jar filename for the given loader kind and version.
///
/// NeoForge: `neoforge-<v>-installer.jar`
/// Forge:    `forge-<mc_ver>-<v>-installer.jar`
pub fn installer_jar_name(kind: InstallerLoaderKind, loader_version: &str, mc_version: &str) -> String {
    match kind {
        InstallerLoaderKind::NeoForge => {
            format!("neoforge-{loader_version}-installer.jar")
        }
        InstallerLoaderKind::Forge => {
            // Forge Maven coord is `net.minecraftforge:forge:<mc_ver>-<v>`
            // so the artifact version component in the filename is `<mc_ver>-<v>`.
            format!("forge-{mc_version}-{loader_version}-installer.jar")
        }
    }
}

/// Build the full Maven download URL for the installer jar.
///
/// NeoForge: `https://maven.neoforged.net/releases/net/neoforged/neoforge/<v>/neoforge-<v>-installer.jar`
/// Forge:    `https://maven.minecraftforge.net/net/minecraftforge/forge/<mc>-<v>/forge-<mc>-<v>-installer.jar`
pub fn installer_url(kind: InstallerLoaderKind, loader_version: &str, mc_version: &str) -> String {
    let jar = installer_jar_name(kind, loader_version, mc_version);
    match kind {
        InstallerLoaderKind::NeoForge => {
            // Maven coord: net.neoforged:neoforge:<v>
            // maven_coord_to_path produces `…/neoforge-<v>.jar`; the actual
            // installer artifact is `neoforge-<v>-installer.jar` — override the filename.
            let coord = format!("net.neoforged:neoforge:{loader_version}");
            let path = maven_coord_to_path(&coord);
            let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or(&path);
            format!("https://maven.neoforged.net/releases/{dir}/{jar}")
        }
        InstallerLoaderKind::Forge => {
            // Maven coord: net.minecraftforge:forge:<mc_ver>-<v>
            let coord = format!("net.minecraftforge:forge:{mc_version}-{loader_version}");
            let path = maven_coord_to_path(&coord);
            // Override the jar name to include the -installer suffix (maven_coord_to_path
            // would produce `forge-<mc>-<v>.jar`; the actual artifact is `-installer.jar`)
            let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or(&path);
            format!("https://maven.minecraftforge.net/{dir}/{jar}")
        }
    }
}

/// The canonical loader version ID used as the `versions/<id>/` directory name
/// and the `versions/<id>/<id>.json` file name.
///
/// NeoForge: `neoforge-<v>`
/// Forge:    `<mc_ver>-forge-<v>`
///
/// Forge's installer writes the version profile to
/// `versions/<mc_ver>-forge-<v>/<mc_ver>-forge-<v>.json`. The MC version comes
/// first — `1.21.1-forge-54.0.21`, not `forge-1.21.1-54.0.21`.
pub fn loader_version_id(kind: InstallerLoaderKind, loader_version: &str, mc_version: &str) -> String {
    match kind {
        InstallerLoaderKind::NeoForge => format!("neoforge-{loader_version}"),
        InstallerLoaderKind::Forge => format!("{mc_version}-forge-{loader_version}"),
    }
}

// ---------------------------------------------------------------------------
// Install-log sink trait
// ---------------------------------------------------------------------------

/// Sink for installer stdout/stderr lines.
///
/// Parallel to `launch::LaunchSink`. The `TauriInstallSink` in `lib.rs`
/// emits `install://log` events; tests use [`CapturingInstallSink`].
pub trait InstallSink: Send + Sync + 'static {
    /// Called for each line from the installer's stdout or stderr.
    /// `stream` is `"stdout"` or `"stderr"`.
    fn log(&self, stream: &str, line: &str);
}

/// An [`InstallSink`] that captures lines for test assertion.
#[cfg(test)]
pub struct CapturingInstallSink {
    pub lines: Mutex<Vec<(String, String)>>, // (stream, line)
}

#[cfg(test)]
impl CapturingInstallSink {
    pub fn new() -> Self {
        Self {
            lines: Mutex::new(Vec::new()),
        }
    }
}

#[cfg(test)]
impl InstallSink for CapturingInstallSink {
    fn log(&self, stream: &str, line: &str) {
        self.lines
            .lock()
            .unwrap()
            .push((stream.to_owned(), line.to_owned()));
    }
}

// ---------------------------------------------------------------------------
// Spawn result type (injectable seam)
// ---------------------------------------------------------------------------

/// Outcome of one installer subprocess invocation.
pub struct SpawnResult {
    /// All stdout lines (in order).
    pub stdout_lines: Vec<String>,
    /// All stderr lines (in order).
    pub stderr_lines: Vec<String>,
    /// Process exit code.
    pub exit_code: i32,
}

// ---------------------------------------------------------------------------
// Core installer runner (injectable spawn)
// ---------------------------------------------------------------------------

/// Run the headless installer for the given loader version, using an injectable
/// download function and spawn closure so the unit tests need no live JVM or
/// live HTTP.
///
/// # Arguments
///
/// - `kind`           — `NeoForge` or `Forge`.
/// - `loader_version` — loader version string (e.g. `"21.1.72"` for NeoForge or
///                      `"54.0.21"` for Forge).
/// - `mc_version`     — Minecraft version string (e.g. `"1.21.1"`).
/// - `java_bin`       — path to the `java` / `java.exe` executable.
/// - `data_dir`       — the launcher app data dir. The installer is run with this
///                      as its working dir and `--installClient` target so that
///                      `versions/` and `libraries/` land in the shared layout.
/// - `download`       — async closure: given `(url, dest_path)`, downloads the
///                      installer jar to `dest_path`. Must be `async`.
/// - `spawn`          — async closure: given `(java_bin, args, cwd)`, spawns the
///                      JVM and returns a [`SpawnResult`]. Must be `async`.
/// - `sink`           — receives log lines from the installer's stdout/stderr.
///
/// # Returns
///
/// On success, the absolute path to the produced `versions/<id>/<id>.json`.
/// On failure (non-zero exit code, IO error, download error), an `Err(String)`.
pub async fn run_installer_core<D, DF, S, SF, Sink>(
    kind: InstallerLoaderKind,
    loader_version: &str,
    mc_version: &str,
    java_bin: &Path,
    data_dir: &Path,
    download: D,
    spawn: S,
    sink: &Sink,
) -> Result<PathBuf, String>
where
    D: FnOnce(String, PathBuf) -> DF,
    DF: std::future::Future<Output = Result<(), String>>,
    S: FnOnce(PathBuf, Vec<String>, PathBuf) -> SF,
    SF: std::future::Future<Output = Result<SpawnResult, String>>,
    Sink: InstallSink,
{
    let version_id = loader_version_id(kind, loader_version, mc_version);

    // Idempotency guard: if versions/<id>/<id>.json already exists, skip.
    let version_json = data_dir
        .join("versions")
        .join(&version_id)
        .join(format!("{version_id}.json"));

    if version_json.exists() {
        return Ok(version_json);
    }

    // Download the installer jar into the shared installers cache under data_dir.
    let installer_dir = data_dir.join("installers");
    std::fs::create_dir_all(&installer_dir)
        .map_err(|e| format!("failed to create installer cache dir: {e}"))?;

    let jar_name = installer_jar_name(kind, loader_version, mc_version);
    let jar_dest = installer_dir.join(&jar_name);

    // Only download if the completed jar is not already cached.
    // A leftover `.part` file (from an interrupted download) does NOT satisfy
    // this check — the download closure is responsible for writing to a `.part`
    // path and atomically renaming to `jar_dest` on success.
    if !jar_dest.exists() {
        let url = installer_url(kind, loader_version, mc_version);
        download(url, jar_dest.clone()).await?;
    }

    // Seed launcher_profiles.json in data_dir before spawning the installer.
    // The Forge installer guards on the existence of this file.
    let profiles_path = data_dir.join("launcher_profiles.json");
    if !profiles_path.exists() {
        std::fs::write(&profiles_path, r#"{"profiles":{}}"#)
            .map_err(|e| format!("failed to seed launcher_profiles.json: {e}"))?;
    }

    // Build the JVM argv: `java -jar <installer_jar> --installClient <data_dir>`
    let args = vec![
        "-jar".to_string(),
        jar_dest.to_string_lossy().into_owned(),
        "--installClient".to_string(),
        data_dir.to_string_lossy().into_owned(),
    ];

    // Spawn the installer JVM.
    let result = spawn(java_bin.to_path_buf(), args, data_dir.to_path_buf()).await?;

    // Forward output to the sink.
    for line in &result.stdout_lines {
        sink.log("stdout", line);
    }
    for line in &result.stderr_lines {
        sink.log("stderr", line);
    }

    // Non-zero exit → fail.
    if result.exit_code != 0 {
        return Err(format!(
            "installer exited with code {}: {}",
            result.exit_code,
            result.stderr_lines.last().cloned().unwrap_or_default()
        ));
    }

    // Verify the expected output exists.
    if !version_json.exists() {
        return Err(format!(
            "installer succeeded but expected output missing: {}",
            version_json.display()
        ));
    }

    Ok(version_json)
}

// ---------------------------------------------------------------------------
// Live runner (uses reqwest + tokio::process)
// ---------------------------------------------------------------------------

/// Run the headless installer using real HTTP and a real JVM spawn.
///
/// This function is not called in unit tests (which use `run_installer_core`
/// with injected closures). Called from `lib.rs` after `ensure_java` has
/// produced `java_bin`.
pub async fn run_installer<Sink: InstallSink>(
    kind: InstallerLoaderKind,
    loader_version: &str,
    mc_version: &str,
    java_bin: &Path,
    data_dir: &Path,
    sink: &Sink,
) -> Result<PathBuf, String> {
    use tokio::process::Command;

    run_installer_core(
        kind,
        loader_version,
        mc_version,
        java_bin,
        data_dir,
        // download closure
        |url, dest| async move {
            let client = reqwest::Client::builder()
                .user_agent(concat!(
                    "modloader/",
                    env!("CARGO_PKG_VERSION"),
                    " (https://github.com/; minecraft launcher)"
                ))
                .build()
                .map_err(|e| format!("failed to build HTTP client: {e}"))?;

            let resp = client
                .get(&url)
                .send()
                .await
                .map_err(|e| format!("installer download request failed for {url}: {e}"))?;

            if !resp.status().is_success() {
                return Err(format!("installer download returned {}: {url}", resp.status()));
            }

            // Stream chunks to a `.part` file; rename to `dest` on success
            // so that an interrupted download does not leave a valid-looking jar
            // that would satisfy the cache check on the next run.
            use futures_util::StreamExt as _;
            use std::io::Write as _;
            let part = dest.with_extension("part");
            let mut file = std::fs::File::create(&part)
                .map_err(|e| format!("failed to create installer part at {}: {e}", part.display()))?;
            let mut stream = resp.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|e| format!("failed to read installer response chunk: {e}"))?;
                file.write_all(&chunk)
                    .map_err(|e| format!("failed to write installer jar chunk: {e}"))?;
            }
            drop(file);
            std::fs::rename(&part, &dest)
                .map_err(|e| format!("failed to rename installer part to jar: {e}"))?;

            Ok(())
        },
        // spawn closure
        |java_bin, args, cwd| async move {
            let mut child = Command::new(&java_bin)
                .args(&args)
                .current_dir(&cwd)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| format!("failed to spawn installer JVM: {e}"))?;

            let stdout_pipe = child.stdout.take().ok_or("failed to capture stdout")?;
            let stderr_pipe = child.stderr.take().ok_or("failed to capture stderr")?;

            // Drain stdout and stderr on independent tasks so that neither pipe
            // can block the child when the other pipe's OS buffer fills (> ~64 KB).
            // A select!-then-sequential-drain approach breaks as soon as one stream
            // closes while the other still has buffered data.
            let stdout_task = tokio::spawn(async move {
                use tokio::io::AsyncBufReadExt as _;
                let mut reader = tokio::io::BufReader::new(stdout_pipe).lines();
                let mut lines = Vec::new();
                while let Ok(Some(l)) = reader.next_line().await {
                    lines.push(l);
                }
                lines
            });
            let stderr_task = tokio::spawn(async move {
                use tokio::io::AsyncBufReadExt as _;
                let mut reader = tokio::io::BufReader::new(stderr_pipe).lines();
                let mut lines = Vec::new();
                while let Ok(Some(l)) = reader.next_line().await {
                    lines.push(l);
                }
                lines
            });

            // Wait for both reader tasks before waiting on the child so the pipes
            // are fully drained (avoids a deadlock where wait() holds the child
            // while the child blocks on a full pipe).
            let stdout_lines = stdout_task
                .await
                .map_err(|e| format!("stdout reader task panicked: {e}"))?;
            let stderr_lines = stderr_task
                .await
                .map_err(|e| format!("stderr reader task panicked: {e}"))?;

            let status = child
                .wait()
                .await
                .map_err(|e| format!("failed to wait for installer: {e}"))?;

            Ok(SpawnResult {
                stdout_lines,
                stderr_lines,
                exit_code: status.code().unwrap_or(-1),
            })
        },
        sink,
    )
    .await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "forge_installer_tests.rs"]
mod tests;
