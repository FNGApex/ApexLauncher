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
/// Forge:    `forge-<mc_ver>-<v>`
pub fn loader_version_id(kind: InstallerLoaderKind, loader_version: &str, mc_version: &str) -> String {
    match kind {
        InstallerLoaderKind::NeoForge => format!("neoforge-{loader_version}"),
        InstallerLoaderKind::Forge => format!("forge-{mc_version}-{loader_version}"),
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
pub struct CapturingInstallSink {
    pub lines: Mutex<Vec<(String, String)>>, // (stream, line)
}

impl CapturingInstallSink {
    pub fn new() -> Self {
        Self {
            lines: Mutex::new(Vec::new()),
        }
    }
}

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

    // Download the installer jar into a temp dir under data_dir.
    let installer_dir = data_dir.join("installer-cache");
    std::fs::create_dir_all(&installer_dir)
        .map_err(|e| format!("failed to create installer cache dir: {e}"))?;

    let jar_name = installer_jar_name(kind, loader_version, mc_version);
    let jar_dest = installer_dir.join(&jar_name);

    // Only download if not already cached.
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
    use tokio::io::AsyncBufReadExt as _;
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

            // Stream bytes to file.
            use std::io::Write as _;
            let mut file = std::fs::File::create(&dest)
                .map_err(|e| format!("failed to create installer jar at {}: {e}", dest.display()))?;
            let bytes = resp
                .bytes()
                .await
                .map_err(|e| format!("failed to read installer response bytes: {e}"))?;
            file.write_all(&bytes)
                .map_err(|e| format!("failed to write installer jar: {e}"))?;

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

            let mut stdout_reader =
                tokio::io::BufReader::new(stdout_pipe).lines();
            let mut stderr_reader =
                tokio::io::BufReader::new(stderr_pipe).lines();

            let mut stdout_lines = Vec::new();
            let mut stderr_lines = Vec::new();

            // Drain stdout and stderr concurrently until both close.
            loop {
                tokio::select! {
                    line = stdout_reader.next_line() => {
                        match line {
                            Ok(Some(l)) => stdout_lines.push(l),
                            _ => break,
                        }
                    }
                    line = stderr_reader.next_line() => {
                        match line {
                            Ok(Some(l)) => stderr_lines.push(l),
                            _ => break,
                        }
                    }
                }
            }

            // Drain any remainder after one stream closes.
            while let Ok(Some(l)) = stdout_reader.next_line().await {
                stdout_lines.push(l);
            }
            while let Ok(Some(l)) = stderr_reader.next_line().await {
                stderr_lines.push(l);
            }

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
mod tests {
    use super::*;
    use std::sync::Arc;

    // --- Pure URL / filename construction ------------------------------------

    #[test]
    fn neoforge_installer_url() {
        let url = installer_url(InstallerLoaderKind::NeoForge, "21.1.72", "1.21.1");
        assert_eq!(
            url,
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/21.1.72/neoforge-21.1.72-installer.jar"
        );
    }

    #[test]
    fn forge_installer_url() {
        let url = installer_url(InstallerLoaderKind::Forge, "54.0.21", "1.21.1");
        assert_eq!(
            url,
            "https://maven.minecraftforge.net/net/minecraftforge/forge/1.21.1-54.0.21/forge-1.21.1-54.0.21-installer.jar"
        );
    }

    #[test]
    fn neoforge_jar_name() {
        assert_eq!(
            installer_jar_name(InstallerLoaderKind::NeoForge, "21.1.72", "1.21.1"),
            "neoforge-21.1.72-installer.jar"
        );
    }

    #[test]
    fn forge_jar_name() {
        assert_eq!(
            installer_jar_name(InstallerLoaderKind::Forge, "54.0.21", "1.21.1"),
            "forge-1.21.1-54.0.21-installer.jar"
        );
    }

    #[test]
    fn neoforge_version_id() {
        assert_eq!(
            loader_version_id(InstallerLoaderKind::NeoForge, "21.1.72", "1.21.1"),
            "neoforge-21.1.72"
        );
    }

    #[test]
    fn forge_version_id() {
        assert_eq!(
            loader_version_id(InstallerLoaderKind::Forge, "54.0.21", "1.21.1"),
            "forge-1.21.1-54.0.21"
        );
    }

    // --- Idempotency guard ---------------------------------------------------

    #[tokio::test]
    async fn idempotency_guard_returns_early_when_version_json_exists() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data_dir = dir.path();

        // Pre-create versions/<id>/<id>.json
        let version_id = "neoforge-21.1.72";
        let version_dir = data_dir.join("versions").join(version_id);
        std::fs::create_dir_all(&version_dir).unwrap();
        let version_json = version_dir.join(format!("{version_id}.json"));
        std::fs::write(&version_json, "{}").unwrap();

        let sink = CapturingInstallSink::new();

        let result = run_installer_core(
            InstallerLoaderKind::NeoForge,
            "21.1.72",
            "1.21.1",
            Path::new("/fake/java"),
            data_dir,
            // download — must NOT be called
            |_url, _dest| async { panic!("download should not be called on idempotency hit") },
            // spawn — must NOT be called
            |_java, _args, _cwd| async { panic!("spawn should not be called on idempotency hit") },
            &sink,
        )
        .await;

        assert!(result.is_ok(), "expected Ok, got: {:?}", result.err());
        assert_eq!(result.unwrap(), version_json);
        // No lines should have been emitted.
        assert!(sink.lines.lock().unwrap().is_empty());
    }

    // --- Spawn: argv + cwd assertion (exit 0) --------------------------------

    #[tokio::test]
    async fn spawn_receives_correct_argv_and_cwd_on_success() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data_dir = dir.path().to_path_buf();

        let java_bin = PathBuf::from("/usr/bin/java");

        // Capture what spawn receives.
        let captured_java: Arc<Mutex<Option<PathBuf>>> = Arc::new(Mutex::new(None));
        let captured_args: Arc<Mutex<Option<Vec<String>>>> = Arc::new(Mutex::new(None));
        let captured_cwd: Arc<Mutex<Option<PathBuf>>> = Arc::new(Mutex::new(None));

        let cap_java = Arc::clone(&captured_java);
        let cap_args = Arc::clone(&captured_args);
        let cap_cwd = Arc::clone(&captured_cwd);

        let expected_jar = data_dir.join("installer-cache").join("neoforge-21.1.72-installer.jar");
        let expected_target = data_dir.clone();

        // We must create the version.json to satisfy the post-spawn existence check.
        let version_id = "neoforge-21.1.72";
        let version_dir = data_dir.join("versions").join(version_id);
        std::fs::create_dir_all(&version_dir).unwrap();
        let version_json = version_dir.join(format!("{version_id}.json"));

        let version_json_for_spawn = version_json.clone();
        let sink = CapturingInstallSink::new();

        let result = run_installer_core(
            InstallerLoaderKind::NeoForge,
            "21.1.72",
            "1.21.1",
            &java_bin,
            &data_dir,
            // download — write a dummy jar so the path exists
            {
                let data_dir = data_dir.clone();
                move |_url, dest| async move {
                    std::fs::create_dir_all(data_dir.join("installer-cache")).unwrap();
                    std::fs::write(&dest, b"fake-jar").unwrap();
                    Ok(())
                }
            },
            // spawn — assert argv + cwd, produce success outcome
            move |java, args, cwd| {
                *cap_java.lock().unwrap() = Some(java);
                *cap_args.lock().unwrap() = Some(args);
                *cap_cwd.lock().unwrap() = Some(cwd);
                // Create version.json so the post-spawn check passes.
                std::fs::write(&version_json_for_spawn, "{}").unwrap();
                async move {
                    Ok(SpawnResult {
                        stdout_lines: vec!["Installing...".to_string()],
                        stderr_lines: vec!["[INFO] Done".to_string()],
                        exit_code: 0,
                    })
                }
            },
            &sink,
        )
        .await;

        assert!(result.is_ok(), "expected Ok, got: {:?}", result.err());

        // Assert java binary.
        assert_eq!(captured_java.lock().unwrap().as_deref(), Some(Path::new("/usr/bin/java")));

        // Assert exact argv: ["-jar", "<jar>", "--installClient", "<data_dir>"]
        let args = captured_args.lock().unwrap().clone().unwrap();
        assert_eq!(args.len(), 4);
        assert_eq!(args[0], "-jar");
        assert_eq!(PathBuf::from(&args[1]), expected_jar, "installer jar path mismatch");
        assert_eq!(args[2], "--installClient");
        assert_eq!(PathBuf::from(&args[3]), expected_target, "installClient target mismatch");

        // Assert working dir = data_dir.
        assert_eq!(captured_cwd.lock().unwrap().as_deref(), Some(data_dir.as_path()));

        // Sink should have received the lines.
        let lines = sink.lines.lock().unwrap().clone();
        assert!(lines.contains(&("stdout".to_string(), "Installing...".to_string())));
        assert!(lines.contains(&("stderr".to_string(), "[INFO] Done".to_string())));
    }

    // --- Spawn: non-zero exit → Err ------------------------------------------

    #[tokio::test]
    async fn non_zero_exit_returns_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data_dir = dir.path().to_path_buf();
        let java_bin = PathBuf::from("/usr/bin/java");

        let sink = CapturingInstallSink::new();

        let result = run_installer_core(
            InstallerLoaderKind::Forge,
            "54.0.21",
            "1.21.1",
            &java_bin,
            &data_dir,
            // download — write a dummy jar
            {
                let data_dir = data_dir.clone();
                move |_url, dest| async move {
                    std::fs::create_dir_all(data_dir.join("installer-cache")).unwrap();
                    std::fs::write(&dest, b"fake-jar").unwrap();
                    Ok(())
                }
            },
            // spawn — exits with code 1
            |_java, _args, _cwd| async {
                Ok(SpawnResult {
                    stdout_lines: vec![],
                    stderr_lines: vec!["Error: installation failed".to_string()],
                    exit_code: 1,
                })
            },
            &sink,
        )
        .await;

        assert!(result.is_err(), "expected Err on non-zero exit");
        let msg = result.unwrap_err();
        assert!(msg.contains("1"), "error should mention exit code 1: {msg}");
    }

    // --- launcher_profiles.json seeding -------------------------------------

    #[tokio::test]
    async fn seeds_launcher_profiles_before_spawn() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data_dir = dir.path().to_path_buf();
        let java_bin = PathBuf::from("/usr/bin/java");

        // Verify file does NOT exist before run.
        let profiles = data_dir.join("launcher_profiles.json");
        assert!(!profiles.exists());

        let profiles_at_spawn: Arc<Mutex<Option<bool>>> = Arc::new(Mutex::new(None));
        let cap = Arc::clone(&profiles_at_spawn);

        let version_id = "neoforge-21.1.72";
        let version_dir = data_dir.join("versions").join(version_id);
        let data_dir_clone = data_dir.clone();
        let sink = CapturingInstallSink::new();

        let _ = run_installer_core(
            InstallerLoaderKind::NeoForge,
            "21.1.72",
            "1.21.1",
            &java_bin,
            &data_dir,
            {
                let data_dir = data_dir.clone();
                move |_url, dest| async move {
                    std::fs::create_dir_all(data_dir.join("installer-cache")).unwrap();
                    std::fs::write(&dest, b"fake-jar").unwrap();
                    Ok(())
                }
            },
            move |_java, _args, _cwd| {
                // Check that launcher_profiles.json exists at spawn time.
                let exists = data_dir_clone.join("launcher_profiles.json").exists();
                *cap.lock().unwrap() = Some(exists);
                // Create version.json to satisfy post-spawn check.
                std::fs::create_dir_all(&version_dir).unwrap();
                let vj = version_dir.join(format!("{version_id}.json"));
                std::fs::write(&vj, "{}").unwrap();
                async { Ok(SpawnResult { stdout_lines: vec![], stderr_lines: vec![], exit_code: 0 }) }
            },
            &sink,
        )
        .await;

        assert_eq!(
            *profiles_at_spawn.lock().unwrap(),
            Some(true),
            "launcher_profiles.json must exist at spawn time"
        );

        // Verify content.
        let content = std::fs::read_to_string(&profiles).unwrap();
        assert_eq!(content, r#"{"profiles":{}}"#);
    }

    // --- Event emission via mock sink ----------------------------------------

    #[tokio::test]
    async fn sink_receives_stdout_and_stderr_lines() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data_dir = dir.path().to_path_buf();
        let java_bin = PathBuf::from("/usr/bin/java");

        let version_id = "forge-1.21.1-54.0.21";
        let version_dir = data_dir.join("versions").join(version_id);
        let sink = Arc::new(CapturingInstallSink::new());

        let sink_ref = Arc::clone(&sink);
        // We pass `sink_ref` directly — need a reference that lives long enough.
        // Re-borrow via Arc deref.

        let _ = run_installer_core(
            InstallerLoaderKind::Forge,
            "54.0.21",
            "1.21.1",
            &java_bin,
            &data_dir,
            {
                let data_dir = data_dir.clone();
                move |_url, dest| async move {
                    std::fs::create_dir_all(data_dir.join("installer-cache")).unwrap();
                    std::fs::write(&dest, b"fake").unwrap();
                    Ok(())
                }
            },
            move |_java, _args, _cwd| {
                std::fs::create_dir_all(&version_dir).unwrap();
                let vj = version_dir.join(format!("{version_id}.json"));
                std::fs::write(&vj, "{}").unwrap();
                async {
                    Ok(SpawnResult {
                        stdout_lines: vec!["line A".to_string(), "line B".to_string()],
                        stderr_lines: vec!["err C".to_string()],
                        exit_code: 0,
                    })
                }
            },
            sink_ref.as_ref(),
        )
        .await;

        let lines = sink.lines.lock().unwrap().clone();
        assert!(lines.contains(&("stdout".to_string(), "line A".to_string())));
        assert!(lines.contains(&("stdout".to_string(), "line B".to_string())));
        assert!(lines.contains(&("stderr".to_string(), "err C".to_string())));
    }
}
