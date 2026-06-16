//! Unit tests for `forge_installer`. Extracted from the source module; wired back
//! via `#[cfg(test)] #[path = "forge_installer_tests.rs"] mod tests;` so private items
//! remain accessible through `super::*`.

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
    // Forge's installer writes versions/<mc>-forge-<loader>/, e.g.
    // `1.21.1-forge-54.0.21` — NOT `forge-1.21.1-54.0.21`.
    assert_eq!(
        loader_version_id(InstallerLoaderKind::Forge, "54.0.21", "1.21.1"),
        "1.21.1-forge-54.0.21"
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

    let expected_jar = data_dir
        .join("installers")
        .join("neoforge-21.1.72-installer.jar");
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
                std::fs::create_dir_all(data_dir.join("installers")).unwrap();
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
    assert_eq!(
        captured_java.lock().unwrap().as_deref(),
        Some(Path::new("/usr/bin/java"))
    );

    // Assert exact argv: ["-jar", "<jar>", "--installClient", "<data_dir>"]
    let args = captured_args.lock().unwrap().clone().unwrap();
    assert_eq!(args.len(), 4);
    assert_eq!(args[0], "-jar");
    assert_eq!(
        PathBuf::from(&args[1]),
        expected_jar,
        "installer jar path mismatch"
    );
    assert_eq!(args[2], "--installClient");
    assert_eq!(
        PathBuf::from(&args[3]),
        expected_target,
        "installClient target mismatch"
    );

    // Assert working dir = data_dir.
    assert_eq!(
        captured_cwd.lock().unwrap().as_deref(),
        Some(data_dir.as_path())
    );

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
                std::fs::create_dir_all(data_dir.join("installers")).unwrap();
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
                std::fs::create_dir_all(data_dir.join("installers")).unwrap();
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
            async {
                Ok(SpawnResult {
                    stdout_lines: vec![],
                    stderr_lines: vec![],
                    exit_code: 0,
                })
            }
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

    let version_id = "1.21.1-forge-54.0.21";
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
                std::fs::create_dir_all(data_dir.join("installers")).unwrap();
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

// --- F-2: .part leftover does not satisfy the cache check ----------------
//
// A pre-existing `<jar>.part` file (interrupted download) must NOT be
// treated as a completed download. The download closure must still be called.

#[tokio::test]
async fn leftover_part_file_does_not_skip_download() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data_dir = dir.path().to_path_buf();
    let java_bin = PathBuf::from("/usr/bin/java");

    // Pre-create the installer cache dir and a leftover `.part` file.
    let installer_dir = data_dir.join("installers");
    std::fs::create_dir_all(&installer_dir).unwrap();
    let jar_name = installer_jar_name(InstallerLoaderKind::NeoForge, "21.1.72", "1.21.1");
    let jar_dest = installer_dir.join(&jar_name);
    // Simulate interrupted download: `.part` exists but the final `.jar` does not.
    let part_path = jar_dest.with_extension("part");
    std::fs::write(&part_path, b"partial-garbage").unwrap();
    assert!(!jar_dest.exists(), "jar must not exist before test");

    let download_called = Arc::new(Mutex::new(false));
    let dc = Arc::clone(&download_called);

    let version_id = "neoforge-21.1.72";
    let version_dir = data_dir.join("versions").join(version_id);
    let sink = CapturingInstallSink::new();

    let _ = run_installer_core(
        InstallerLoaderKind::NeoForge,
        "21.1.72",
        "1.21.1",
        &java_bin,
        &data_dir,
        // download — record that it was called, write the jar
        move |_url, dest| {
            *dc.lock().unwrap() = true;
            async move {
                std::fs::write(&dest, b"real-jar").unwrap();
                Ok(())
            }
        },
        // spawn — succeed and create version.json
        move |_java, _args, _cwd| {
            std::fs::create_dir_all(&version_dir).unwrap();
            let vj = version_dir.join(format!("{version_id}.json"));
            std::fs::write(&vj, "{}").unwrap();
            async {
                Ok(SpawnResult {
                    stdout_lines: vec![],
                    stderr_lines: vec![],
                    exit_code: 0,
                })
            }
        },
        &sink,
    )
    .await;

    assert!(
        *download_called.lock().unwrap(),
        "download must be called when only a .part leftover exists"
    );
}

// --- F-2: completed jar satisfies cache check (no re-download) -----------

#[tokio::test]
async fn completed_jar_satisfies_cache_check() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data_dir = dir.path().to_path_buf();
    let java_bin = PathBuf::from("/usr/bin/java");

    // Pre-create the installer cache dir and the completed jar.
    let installer_dir = data_dir.join("installers");
    std::fs::create_dir_all(&installer_dir).unwrap();
    let jar_name = installer_jar_name(InstallerLoaderKind::NeoForge, "21.1.72", "1.21.1");
    let jar_dest = installer_dir.join(&jar_name);
    std::fs::write(&jar_dest, b"cached-jar").unwrap();

    let download_called = Arc::new(Mutex::new(false));
    let dc = Arc::clone(&download_called);

    let version_id = "neoforge-21.1.72";
    let version_dir = data_dir.join("versions").join(version_id);
    let sink = CapturingInstallSink::new();

    let _ = run_installer_core(
        InstallerLoaderKind::NeoForge,
        "21.1.72",
        "1.21.1",
        &java_bin,
        &data_dir,
        // download — must NOT be called
        move |_url, _dest| {
            *dc.lock().unwrap() = true;
            async { Ok(()) }
        },
        move |_java, _args, _cwd| {
            std::fs::create_dir_all(&version_dir).unwrap();
            let vj = version_dir.join(format!("{version_id}.json"));
            std::fs::write(&vj, "{}").unwrap();
            async {
                Ok(SpawnResult {
                    stdout_lines: vec![],
                    stderr_lines: vec![],
                    exit_code: 0,
                })
            }
        },
        &sink,
    )
    .await;

    assert!(
        !*download_called.lock().unwrap(),
        "download must NOT be called when the completed jar is cached"
    );
}

// --- F-1: both streams fully collected even when one stream is long -------
//
// The injectable SpawnResult already carries pre-collected lines, so we
// verify at the run_installer_core level that all lines from a long stdout
// and a long stderr both arrive at the sink. This is the seam-level proof
// that the concurrent-drain contract is honoured; the live tokio::spawn fix
// is what makes it hold against a real process.

#[tokio::test]
async fn both_streams_fully_collected_when_one_stream_is_long() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data_dir = dir.path().to_path_buf();
    let java_bin = PathBuf::from("/usr/bin/java");

    let version_id = "neoforge-21.1.72";
    let version_dir = data_dir.join("versions").join(version_id);
    let sink = Arc::new(CapturingInstallSink::new());

    // Build a long stdout (1000 lines) and a short stderr (3 lines).
    let long_stdout: Vec<String> = (0..1000).map(|i| format!("stdout-line-{i}")).collect();
    let short_stderr: Vec<String> = vec![
        "err-0".to_string(),
        "err-1".to_string(),
        "err-2".to_string(),
    ];

    let expected_last_stdout = long_stdout.last().unwrap().clone();
    let long_stdout_clone = long_stdout.clone();
    let short_stderr_clone = short_stderr.clone();

    let sink_ref = Arc::clone(&sink);
    let _ = run_installer_core(
        InstallerLoaderKind::NeoForge,
        "21.1.72",
        "1.21.1",
        &java_bin,
        &data_dir,
        {
            let data_dir = data_dir.clone();
            move |_url, dest| async move {
                std::fs::create_dir_all(data_dir.join("installers")).unwrap();
                std::fs::write(&dest, b"fake").unwrap();
                Ok(())
            }
        },
        move |_java, _args, _cwd| {
            std::fs::create_dir_all(&version_dir).unwrap();
            let vj = version_dir.join(format!("{version_id}.json"));
            std::fs::write(&vj, "{}").unwrap();
            async move {
                Ok(SpawnResult {
                    stdout_lines: long_stdout_clone,
                    stderr_lines: short_stderr_clone,
                    exit_code: 0,
                })
            }
        },
        sink_ref.as_ref(),
    )
    .await;

    let lines = sink.lines.lock().unwrap().clone();
    let stdout_count = lines.iter().filter(|(s, _)| s == "stdout").count();
    let stderr_count = lines.iter().filter(|(s, _)| s == "stderr").count();

    assert_eq!(stdout_count, 1000, "all 1000 stdout lines must arrive");
    assert_eq!(stderr_count, 3, "all 3 stderr lines must arrive");
    // Confirm the last stdout line is present (guards against truncation).
    assert!(
        lines.contains(&("stdout".to_string(), expected_last_stdout.clone())),
        "last stdout line missing: {expected_last_stdout}"
    );
}
