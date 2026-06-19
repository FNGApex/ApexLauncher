//! Unit tests for `launch`. Extracted from the source module; wired back
//! via `#[cfg(test)] #[path = "launch_tests.rs"] mod tests;` so private items
//! remain accessible through `super::*`.

use super::*;
use std::path::PathBuf;

/// Offline identity for tests that don't exercise identity routing.
fn offline_identity() -> LaunchIdentity {
    LaunchIdentity::offline()
}

/// Minimal EffectiveJava for tests that do not exercise heap/extra-args behaviour.
/// Uses 512 MiB xmx (arbitrary non-zero value), no xms, no extra args, no path override.
fn noop_eff() -> crate::core::java_resolve::EffectiveJava {
    crate::core::java_resolve::EffectiveJava {
        xmx_mb: 512,
        xms_mb: None,
        extra_args: vec![],
        java_path: None,
    }
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
        assets_root: PathBuf::from("/data/cache/assets"),
        natives_directory: PathBuf::from("/instances/my-world/natives"),
        legacy_assets_root: PathBuf::from("/data/cache/assets/virtual/legacy"),
        library_directory: PathBuf::from("/instances/my-world/libraries"),
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
    let cp = vec![
        "/data/libraries/authlib.jar",
        "/data/versions/1.21.1/1.21.1.jar",
    ];

    let meta = make_meta("release", jvm_args, game_args, cp, false, None);
    let paths = make_paths();

    let argv = build_argv(&meta, &paths, &offline_identity(), &noop_eff()).expect("no unresolved placeholders");

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
        jvm_section
            .iter()
            .any(|a| a.contains("/instances/my-world/natives")),
        "natives_directory must be substituted: {:?}",
        jvm_section
    );
    let cp_idx = jvm_section
        .iter()
        .position(|a| a == "-cp")
        .expect("-cp must be present");
    let cp_val = &jvm_section[cp_idx + 1];
    assert!(
        cp_val.contains("authlib.jar"),
        "classpath must contain authlib.jar: {cp_val}"
    );
    assert!(
        cp_val.contains("1.21.1.jar"),
        "classpath must contain client jar: {cp_val}"
    );

    // game_args section (after main_class).
    let game_section = &argv[mc_idx + 1..];
    let game_str = game_section.join(" ");
    assert!(
        game_str.contains("Player"),
        "${{auth_player_name}} not substituted"
    );
    assert!(
        game_str.contains("1.21.1"),
        "${{version_name}} not substituted"
    );
    assert!(
        game_str.contains("/instances/my-world/mc"),
        "${{game_directory}} not substituted"
    );
    assert!(
        game_str.contains("/data/cache/assets"),
        "${{assets_root}} not substituted"
    );
    assert!(
        game_str.contains("17"),
        "${{assets_index_name}} not substituted"
    );
    assert!(
        game_str.contains("2e5dcd13-3805-3256-b49c-819167bf4871"),
        "${{auth_uuid}} not substituted"
    );
    assert!(
        game_str.contains('0'),
        "${{auth_access_token}} not substituted"
    );
    assert!(game_str.contains("msa"), "${{user_type}} not substituted");
    assert!(
        game_str.contains("release"),
        "${{version_type}} not substituted"
    );

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

    let err = build_argv(&meta, &paths, &offline_identity(), &noop_eff())
        .expect_err("must error on unknown placeholder");
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
    let jvm_args = vec!["-Dlog4j.configurationFile=${path}", "-cp", "${classpath}"];
    let meta = make_meta(
        "release",
        jvm_args,
        vec![],
        vec!["/data/versions/1.21.1/1.21.1.jar"],
        false,
        None, // no logging config
    );
    let paths = make_paths();

    let argv = build_argv(&meta, &paths, &offline_identity(), &noop_eff()).expect("no error expected");
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
    let jvm_args = vec!["-Dlog4j.configurationFile=${path}", "-cp", "${classpath}"];
    let meta = make_meta(
        "release",
        jvm_args,
        vec![],
        vec!["/data/versions/1.21.1/1.21.1.jar"],
        false,
        Some("/data/assets/log_configs/log4j2.xml"),
    );
    let paths = make_paths();

    let argv = build_argv(&meta, &paths, &offline_identity(), &noop_eff()).expect("no error expected");
    assert!(
        argv.iter()
            .any(|a| a.contains("/data/assets/log_configs/log4j2.xml")),
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

    let argv = build_argv(&meta, &paths, &offline_identity(), &noop_eff()).expect("no error expected");

    // Defaults must include -cp and classpath.
    let cp_idx = argv
        .iter()
        .position(|a| a == "-cp")
        .expect("-cp must be injected");
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
        assert!(!arg.contains("${"), "raw placeholder in legacy argv: {arg}");
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

    let argv = build_argv(&meta, &paths, &offline_identity(), &noop_eff()).expect("no error");
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

    let argv = build_argv(&meta, &paths, &offline_identity(), &noop_eff()).expect("no error");
    // Modern: uses /data/cache/assets, NOT /data/cache/assets/virtual/legacy.
    assert!(
        argv.iter().any(|a| a == "/data/cache/assets"),
        "modern assets must use /data/cache/assets: {argv:?}"
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

    let argv = build_argv(&meta, &paths, &offline_identity(), &noop_eff()).expect("no error");
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
    // After C2, ${library_directory} resolves to the per-instance libraries dir
    // (paths.library_directory = /instances/my-world/libraries), NOT the shared
    // cache. This is the key C2 invariant: Forge reads from the instance tree.
    let jvm_args = vec![
        "-DlibraryDirectory=${library_directory}",
        "-cp",
        "${classpath}",
    ];
    let meta = make_meta(
        "release",
        jvm_args,
        vec![],
        vec!["/data/cache/versions/1.21.1/1.21.1.jar"],
        false,
        None,
    );
    let paths = make_paths();
    // library_directory = /instances/my-world/libraries (per-instance, post-C2)

    let argv = build_argv(&meta, &paths, &offline_identity(), &noop_eff()).expect("no unresolved");

    let lib_dir_arg = argv
        .iter()
        .find(|a| a.starts_with("-DlibraryDirectory="))
        .expect("-DlibraryDirectory arg must be in argv");

    // Must resolve to the per-instance libraries dir, not the cache.
    assert!(
        lib_dir_arg.ends_with("libraries"),
        "${{library_directory}} must resolve to .../libraries: {lib_dir_arg}"
    );
    assert!(
        lib_dir_arg.contains("instances"),
        "${{library_directory}} must point at the instance tree after C2: {lib_dir_arg}"
    );
    assert!(
        !lib_dir_arg.contains("${library_directory}"),
        "${{library_directory}} must be fully substituted: {lib_dir_arg}"
    );
    // Must NOT point at the shared cache.
    assert!(
        !lib_dir_arg.contains("/data/cache/libraries"),
        "${{library_directory}} must NOT point at shared cache/libraries: {lib_dir_arg}"
    );
}

/// `--assetsDir` must still point at the shared `cache/assets`, not the instance.
///
/// This verifies design Recommendation A: assets stay shared. The materialization
/// step (C2) must not change where `${assets_root}` resolves.
#[test]
fn build_argv_assets_dir_stays_in_shared_cache() {
    let game_args = vec!["--assetsDir", "${assets_root}"];
    let meta = make_meta(
        "release",
        vec!["-cp", "${classpath}"],
        game_args,
        vec!["/data/cache/versions/1.21.1/1.21.1.jar"],
        false,
        None,
    );
    let paths = make_paths();
    // assets_root = /data/cache/assets — must remain unchanged after C2.

    let argv = build_argv(&meta, &paths, &offline_identity(), &noop_eff()).expect("no error");

    let assets_arg = argv
        .iter()
        .position(|a| a == "--assetsDir")
        .and_then(|i| argv.get(i + 1))
        .expect("--assetsDir must be in argv");

    assert!(
        assets_arg.contains("cache"),
        "--assetsDir must point into the shared cache: {assets_arg}"
    );
    assert!(
        assets_arg.ends_with("assets"),
        "--assetsDir must end with 'assets': {assets_arg}"
    );
    assert!(
        !assets_arg.contains("instances"),
        "--assetsDir must NOT point at the instance tree: {assets_arg}"
    );
}

#[test]
fn build_argv_forge_classpath_separator_substituted() {
    // Forge JVM args can include ${classpath_separator} for manual classpath assembly.
    let jvm_args = vec!["-DcpSep=${classpath_separator}", "-cp", "${classpath}"];
    let meta = make_meta(
        "release",
        jvm_args,
        vec![],
        vec!["/data/versions/1.21.1/1.21.1.jar"],
        false,
        None,
    );
    let paths = make_paths();

    let argv = build_argv(&meta, &paths, &offline_identity(), &noop_eff()).expect("no unresolved");

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

    let argv = build_argv(&meta, &paths, &offline_identity(), &noop_eff()).expect("no unresolved");

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
    assert!(
        extracted.exists(),
        "libfoo.so must be extracted: {:?}",
        extracted
    );

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
            min_memory_mb: None,
            path_override: None,
            use_pack_settings: false,
        },
        source: None,
        pack_locked: false,
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
    use crate::core::instances::{read_manifest_pub, record_playtime};

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
    use crate::core::instances::{read_manifest_pub, record_playtime};

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

    // Wait for the monitor to mark the entry terminal — poll with a timeout.
    // CP6: the entry is RETAINED post-exit (terminal status), not removed, so
    // get_run_state / get_run_logs can recover state after the process is gone.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        let terminal = registry
            .lock()
            .unwrap()
            .get(&slug)
            .map(|s| s.status.is_terminal())
            .unwrap_or(false);
        if terminal {
            break;
        }
        if std::time::Instant::now() > deadline {
            panic!("process did not reach terminal status within 10s");
        }
    }

    // Entry retained with a terminal status (not removed).
    assert!(
        registry
            .lock()
            .unwrap()
            .get(&slug)
            .map(|s| s.status == RunStatus::Exited)
            .unwrap_or(false),
        "registry entry must be retained with Exited status after process exits"
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

    // Fire the kill signal — must NOT mark terminal immediately.
    kill_instance(&registry, &slug).expect("kill must succeed while running");

    // Entry must STILL be non-terminal right after kill (monitor hasn't exited yet).
    assert!(
        registry
            .lock()
            .unwrap()
            .get(&slug)
            .map(|s| !s.status.is_terminal())
            .unwrap_or(false),
        "registry entry must persist non-terminal immediately after kill"
    );

    // Wait for the monitor to mark the entry terminal (child terminates after kill).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        let terminal = registry
            .lock()
            .unwrap()
            .get(&slug)
            .map(|s| s.status.is_terminal())
            .unwrap_or(false);
        if terminal {
            break;
        }
        if std::time::Instant::now() > deadline {
            panic!("registry entry was not marked terminal within 10s after kill");
        }
    }

    // Entry retained with terminal Killed status — monitor recorded the exit.
    assert!(
        registry
            .lock()
            .unwrap()
            .get(&slug)
            .map(|s| s.status == RunStatus::Killed)
            .unwrap_or(false),
        "registry entry must be retained with Killed status after monitor confirms exit"
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
            "--username",
            "${auth_player_name}",
            "--uuid",
            "${auth_uuid}",
            "--accessToken",
            "${auth_access_token}",
            "--userType",
            "${user_type}",
            "--clientId",
            "${clientid}",
            "--xuid",
            "${auth_xuid}",
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
        client_id: "azure_client_xyz".to_string(),
    };

    let argv = build_argv(&meta, &paths, &identity, &noop_eff()).expect("no unresolved placeholders");
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

    let argv = build_argv(&meta, &paths, &identity, &noop_eff()).expect("no error");
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
        client_id: "client_check_456".to_string(),
    };

    let argv = build_argv(&meta, &paths, &identity, &noop_eff()).expect("no error");
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

/// ${clientid} (the `--clientId` telemetry arg in MS-auth version JSONs) must be
/// in the substitution table. Regression: a real modern manifest emits
/// `--clientId ${clientid}` and argv assembly failed with an unsubstituted
/// placeholder because the table only had ${auth_xuid}.
#[test]
fn cp4_clientid_placeholder_is_substituted() {
    let meta = make_identity_meta();
    let paths = make_paths();

    let identity = LaunchIdentity {
        player_name: "AnyPlayer".to_string(),
        uuid: "aaaabbbb-0000-0000-0000-ccccddddeeee".to_string(),
        access_token: "tok".to_string(),
        xuid: "xuid_1".to_string(),
        user_type: "msa".to_string(),
        client_id: "client_id_value_789".to_string(),
    };

    let argv = build_argv(&meta, &paths, &identity, &noop_eff()).expect("no error");
    // No raw placeholder must survive.
    for arg in &argv {
        assert!(
            !arg.contains("${clientid}"),
            "raw ${{clientid}} must not appear in argv: {arg}"
        );
    }
    // The client_id value must appear.
    assert!(
        argv.iter().any(|a| a.contains("client_id_value_789")),
        "client_id must be substituted into argv: {argv:?}"
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
use tempfile::TempDir;
use tokio::sync::Mutex as TokioMutex;

/// In-memory keyring for tests — no OS keychain calls.
struct FakeKeyring {
    store: StdMutex<StdHashMap<String, String>>,
}

impl FakeKeyring {
    fn new() -> Self {
        FakeKeyring {
            store: StdMutex::new(StdHashMap::new()),
        }
    }
}

impl KeyringBackend for FakeKeyring {
    fn store_secret(&self, id: &str, secret: &str) -> Result<(), AuthError> {
        self.store
            .lock()
            .unwrap()
            .insert(id.to_owned(), secret.to_owned());
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
    fn ok(body: impl Into<String>) -> Self {
        MockResp(200, body.into())
    }
}

/// Mock HTTP client — pops responses in FIFO order regardless of method.
struct MockAuthClient {
    responses: std::sync::Arc<TokioMutex<VecDeque<MockResp>>>,
}

impl MockAuthClient {
    fn new(responses: Vec<MockResp>) -> Self {
        Self {
            responses: std::sync::Arc::new(TokioMutex::new(responses.into_iter().collect())),
        }
    }

    async fn pop(&self) -> (u16, String) {
        let mut q = self.responses.lock().await;
        let MockResp(s, b) = q
            .pop_front()
            .expect("MockAuthClient: no more canned responses");
        (s, b)
    }
}

#[async_trait::async_trait]
impl AuthHttpClient for MockAuthClient {
    async fn post_form(
        &self,
        _url: &str,
        _params: &[(&str, &str)],
    ) -> Result<(u16, String), reqwest::Error> {
        Ok(self.pop().await)
    }
    async fn post_json(
        &self,
        _url: &str,
        _body: serde_json::Value,
    ) -> Result<(u16, String), reqwest::Error> {
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
        MockResp::ok(
            r#"{"access_token":"ms_access","refresh_token":"ms_refresh_new","expires_in":3600}"#,
        ),
        // XBL authenticate
        MockResp::ok(r#"{"Token":"xbl_tok","DisplayClaims":{"xui":[{"uhs":"uhs_val"}]}}"#),
        // XSTS authorize
        MockResp::ok(r#"{"Token":"xsts_tok","DisplayClaims":{"xui":[{"xid":"xuid_abc"}]}}"#),
        // MC login_with_xbox
        MockResp::ok(
            r#"{"username":"ignored","access_token":"mc_tok_fresh","token_type":"Bearer","expires_in":86400}"#,
        ),
        // MC profile
        MockResp::ok(r#"{"id":"uuid1234","name":"OnlinePlayer","skins":[],"capes":[]}"#),
    ]
}

fn make_store_with_account(dir: &TempDir, _account_id: &str, refresh_token: &str) -> AccountStore {
    let path = dir.path().join("account.json");
    let mut store = AccountStore::load(path, Box::new(FakeKeyring::new()))
        .expect("AccountStore::load should succeed");
    let meta = AccountMeta {
        id: FIXTURE_ACCOUNT_ID.to_owned(),
        username: "SomePlayer".to_owned(),
        xuid: "xuid_old".to_owned(),
        mc_token_expires: None, // never cached → always refresh
    };
    store.set_account(meta, refresh_token).expect("set_account");
    store
}

/// The MC profile fixture returns `"id": "uuid1234"` — this is the account id
/// that `xbox_chain` returns in `Account.id`.
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

/// No account → offline identity (no HTTP calls).
#[tokio::test]
async fn cp4_resolve_no_active_account_returns_offline() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("account.json");
    let mut store = AccountStore::load(path, Box::new(FakeKeyring::new())).expect("load");
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
    // so that set_account overwrites the existing entry and
    // get_account still resolves after the refresh.
    let mut store = make_store_with_account(&dir, FIXTURE_ACCOUNT_ID, "stored_refresh_tok");
    let http = MockAuthClient::new(xbox_chain_responses());

    let identity = resolve_launch_identity(&mut store, &http, false)
        .await
        .expect("online resolve must succeed");

    assert_eq!(
        identity.player_name, "OnlinePlayer",
        "username from xbox chain"
    );
    assert_eq!(identity.uuid, "uuid1234", "uuid from MC profile");
    assert_eq!(identity.xuid, "xuid_abc", "xuid from XSTS claims");
    assert_eq!(
        identity.access_token, "mc_tok_fresh",
        "fresh MC token from chain"
    );
    assert_eq!(identity.user_type, "msa");

    // Offline constants must not appear.
    assert_ne!(identity.player_name, OFFLINE_PLAYER_NAME);
    assert_ne!(identity.uuid, offline_uuid().as_hyphenated().to_string());

    // Store must have been updated with refreshed metadata.
    let updated = store
        .get_account()
        .expect("account still set after refresh");
    assert_eq!(
        updated.username, "OnlinePlayer",
        "store updated with new username"
    );
    assert_eq!(updated.xuid, "xuid_abc", "store updated with new xuid");

    // New MS refresh token must be in keyring.
    let new_rt = store.get_refresh_token().expect("refresh token in keyring");
    assert_eq!(
        new_rt, "ms_refresh_new",
        "keyring updated with new MS refresh token"
    );
}

// -----------------------------------------------------------------------
// C2 — rewrite_classpath_for_instance
//
// Pure helper: given cache_dir, instance_dir, and a LaunchMeta with
// absolute cache-rooted classpath+natives, rewrites them to instance-rooted
// paths and returns the relative paths to materialize.
// -----------------------------------------------------------------------

fn make_rewrite_meta(
    classpath: Vec<&str>,
    natives: Vec<&str>,
) -> crate::core::resolver::LaunchMeta {
    crate::core::resolver::LaunchMeta {
        version_id: "1.21.1".to_string(),
        version_type: "release".to_string(),
        main_class: "net.minecraft.client.main.Main".to_string(),
        jvm_args: vec![],
        game_args: vec![],
        asset_index_id: "17".to_string(),
        assets_legacy: false,
        java_major: 21,
        classpath: classpath.into_iter().map(str::to_owned).collect(),
        natives: natives.into_iter().map(str::to_owned).collect(),
        logging_config: None,
    }
}

/// Classpath entries under cache_dir are rewritten to instance_dir.
/// Returned rel_paths are the stripped relative paths.
#[test]
fn rewrite_classpath_entries_resolve_under_instance_dir() {
    let cache_dir = PathBuf::from("/data/cache");
    let instance_dir = PathBuf::from("/data/instances/my-world");

    let mut meta = make_rewrite_meta(
        vec![
            "/data/cache/libraries/com/example/foo/1.0/foo-1.0.jar",
            "/data/cache/versions/1.21.1/1.21.1.jar",
        ],
        vec![],
    );

    let rel_paths = rewrite_classpath_for_instance(&cache_dir, &instance_dir, &mut meta);

    // Both classpath entries must now point at the instance dir.
    assert_eq!(meta.classpath.len(), 2);
    assert!(
        meta.classpath[0].starts_with("/data/instances/my-world"),
        "classpath[0] must be under instance_dir: {}",
        meta.classpath[0]
    );
    assert!(
        meta.classpath[1].starts_with("/data/instances/my-world"),
        "classpath[1] must be under instance_dir: {}",
        meta.classpath[1]
    );

    // Rel paths must be the cache-relative paths.
    assert_eq!(rel_paths.len(), 2);
    assert_eq!(
        rel_paths[0],
        PathBuf::from("libraries/com/example/foo/1.0/foo-1.0.jar")
    );
    assert_eq!(rel_paths[1], PathBuf::from("versions/1.21.1/1.21.1.jar"));
}

/// Natives entries under cache_dir are rewritten and included in rel_paths.
#[test]
fn rewrite_natives_entries_resolve_under_instance_dir() {
    let cache_dir = PathBuf::from("/data/cache");
    let instance_dir = PathBuf::from("/data/instances/my-world");

    let mut meta = make_rewrite_meta(
        vec!["/data/cache/libraries/net/java/jogl/1.0/jogl-1.0.jar"],
        vec!["/data/cache/libraries/net/java/jogl/1.0/jogl-1.0-natives-linux.jar"],
    );

    let rel_paths = rewrite_classpath_for_instance(&cache_dir, &instance_dir, &mut meta);

    // Natives entry rewritten to instance dir.
    assert!(
        meta.natives[0].starts_with("/data/instances/my-world"),
        "natives[0] must be under instance_dir: {}",
        meta.natives[0]
    );

    // Both the classpath jar and the natives jar appear in rel_paths.
    assert_eq!(
        rel_paths.len(),
        2,
        "both classpath and natives rel paths must be returned"
    );
}

/// Entries outside cache_dir pass through unchanged.
#[test]
fn entries_outside_cache_dir_pass_through_unchanged() {
    let cache_dir = PathBuf::from("/data/cache");
    let instance_dir = PathBuf::from("/data/instances/my-world");

    let outside_entry = "/opt/jdk/lib/rt.jar";
    let mut meta = make_rewrite_meta(
        vec![
            "/data/cache/libraries/asm/asm/9.0/asm-9.0.jar",
            outside_entry,
        ],
        vec![],
    );

    let rel_paths = rewrite_classpath_for_instance(&cache_dir, &instance_dir, &mut meta);

    // Cache entry is rewritten; outside entry is left unchanged.
    assert!(
        meta.classpath[0].starts_with("/data/instances/my-world"),
        "cache entry must be rewritten: {}",
        meta.classpath[0]
    );
    assert_eq!(
        meta.classpath[1], outside_entry,
        "outside-cache entry must pass through unchanged"
    );

    // Only the cache-rooted entry appears in rel_paths.
    assert_eq!(
        rel_paths.len(),
        1,
        "only cache-rooted paths appear in rel_paths"
    );
}

/// A path present in BOTH classpath and natives appears exactly once in rel_paths.
#[test]
fn a_path_in_both_classpath_and_natives_dedups_to_one_rel_entry() {
    let cache_dir = PathBuf::from("/data/cache");
    let instance_dir = PathBuf::from("/data/instances/my-world");

    // The same jar appears in both classpath and natives (e.g. a fat jar used as
    // both a library and a native-extraction source).
    let shared = "/data/cache/libraries/lwjgl/3.3.1/lwjgl-3.3.1.jar";
    let mut meta = make_rewrite_meta(vec![shared], vec![shared]);

    let rel_paths = rewrite_classpath_for_instance(&cache_dir, &instance_dir, &mut meta);

    let expected_rel = PathBuf::from("libraries/lwjgl/3.3.1/lwjgl-3.3.1.jar");
    let occurrences = rel_paths.iter().filter(|p| **p == expected_rel).count();
    assert_eq!(
        occurrences, 1,
        "shared classpath+natives path must appear exactly once in rel_paths; got {occurrences}"
    );
}

/// Rel paths returned are correct relative paths (no leading separator).
#[test]
fn returned_rel_paths_are_correct_relative_paths() {
    let cache_dir = PathBuf::from("/some/cache");
    let instance_dir = PathBuf::from("/some/instances/slug");

    let mut meta = make_rewrite_meta(
        vec![
            "/some/cache/libraries/a/b/c.jar",
            "/some/cache/versions/1.20/1.20.jar",
        ],
        vec![],
    );

    let rel_paths = rewrite_classpath_for_instance(&cache_dir, &instance_dir, &mut meta);

    // Rel paths must not start with a separator.
    for rel in &rel_paths {
        assert!(
            rel.is_relative(),
            "rel_path must be relative: {}",
            rel.display()
        );
    }

    // Verify exact values.
    assert!(rel_paths.contains(&PathBuf::from("libraries/a/b/c.jar")));
    assert!(rel_paths.contains(&PathBuf::from("versions/1.20/1.20.jar")));
}

// -----------------------------------------------------------------------
// CP6 — runner extension: RunState, prep serialization, log ring, recovery
// -----------------------------------------------------------------------

/// Cross-platform trivial process: prints `hello` and exits 0.
#[cfg(windows)]
fn echo_proc() -> (PathBuf, Vec<String>) {
    (
        PathBuf::from("cmd.exe"),
        vec!["/c".to_string(), "echo hello".to_string()],
    )
}
#[cfg(not(windows))]
fn echo_proc() -> (PathBuf, Vec<String>) {
    (
        PathBuf::from("sh"),
        vec!["-c".to_string(), "echo hello".to_string()],
    )
}

/// Cross-platform long-running process (~30s) so a kill can fire mid-run.
#[cfg(windows)]
fn sleep_proc() -> (PathBuf, Vec<String>) {
    (
        PathBuf::from("cmd.exe"),
        vec!["/c".to_string(), "ping -n 30 127.0.0.1 >nul".to_string()],
    )
}
#[cfg(not(windows))]
fn sleep_proc() -> (PathBuf, Vec<String>) {
    (
        PathBuf::from("sh"),
        vec!["-c".to_string(), "sleep 30".to_string()],
    )
}

/// Poll until `pred(&map)` is true or `secs` elapse (then panic with `what`).
async fn wait_until<F>(registry: &Arc<RunningRegistry>, secs: u64, what: &str, pred: F)
where
    F: Fn(&std::collections::HashMap<String, RunState>) -> bool,
{
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    loop {
        if pred(&registry.lock().unwrap()) {
            return;
        }
        if std::time::Instant::now() > deadline {
            panic!("timed out after {secs}s waiting for: {what}");
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(25)).await;
    }
}

/// Prep serialization: two launches share one `Semaphore(1)`. While each holds
/// the permit it marks its instance `Preparing`; the snapshot must NEVER show
/// two `Preparing` at once, yet BOTH must reach `Running`.
#[tokio::test]
async fn cp6_prep_is_serialized_never_two_preparing_both_run() {
    use std::sync::Arc;

    let tmp = tempfile::TempDir::new().unwrap();
    let game_dir = tmp.path().join("mc");
    fs::create_dir_all(&game_dir).unwrap();

    let registry = Arc::new(new_running_registry());
    let sink = Arc::new(CapturingLaunchSink::new());
    let prep = new_prep_semaphore();

    // Each "launch" flow: acquire the prep permit, mark Preparing, do a short
    // prep delay, then spawn the JVM (releasing the permit only after spawn).
    let run_one = |slug: String| {
        let registry = Arc::clone(&registry);
        let sink = Arc::clone(&sink);
        let prep = Arc::clone(&prep);
        let inst_dir = make_instance_dir_named(&tmp, &slug);
        let game_dir = game_dir.clone();
        async move {
            let permit = prep.acquire_owned().await.unwrap();
            mark_preparing(&registry, &slug, &*sink);
            tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
            let (java, argv) = sleep_proc();
            spawn_instance(
                slug.clone(),
                inst_dir,
                game_dir,
                java,
                argv,
                Arc::clone(&registry),
                Arc::clone(&sink),
            )
            .await
            .expect("spawn must succeed");
            drop(permit); // release prep permit only after the JVM spawned
        }
    };

    let a = tokio::spawn(run_one("pack-a".to_string()));
    let b = tokio::spawn(run_one("pack-b".to_string()));

    // While both flows are in flight, repeatedly snapshot: at most one Preparing.
    let watcher = {
        let registry = Arc::clone(&registry);
        tokio::spawn(async move {
            for _ in 0..200 {
                let preparing = {
                    let g = registry.lock().unwrap();
                    g.values()
                        .filter(|s| s.status == RunStatus::Preparing)
                        .count()
                };
                assert!(
                    preparing <= 1,
                    "snapshot showed {preparing} Preparing instances at once"
                );
                tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
            }
        })
    };

    a.await.unwrap();
    b.await.unwrap();
    watcher.await.unwrap();

    // Both reached Running.
    wait_until(&registry, 10, "both packs Running", |g| {
        g.values()
            .filter(|s| s.status == RunStatus::Running)
            .count()
            == 2
    })
    .await;

    // Clean up the long-running children.
    let _ = kill_instance(&registry, "pack-a");
    let _ = kill_instance(&registry, "pack-b");
}

/// `list_running` enumerates only the non-terminal (Preparing/Running) entries.
#[tokio::test]
async fn cp6_list_running_enumerates_active_instances() {
    use std::sync::Arc;

    let tmp = tempfile::TempDir::new().unwrap();
    let game_dir = tmp.path().join("mc");
    fs::create_dir_all(&game_dir).unwrap();

    let registry = Arc::new(new_running_registry());
    let sink = Arc::new(CapturingLaunchSink::new());

    assert!(list_running(&registry).is_empty(), "empty registry");

    let inst_dir = make_instance_dir_named(&tmp, "enum-1");
    let (java, argv) = sleep_proc();
    spawn_instance(
        "enum-1".to_string(),
        inst_dir,
        game_dir,
        java,
        argv,
        Arc::clone(&registry),
        Arc::clone(&sink),
    )
    .await
    .expect("spawn must succeed");

    let running = list_running(&registry);
    assert_eq!(running.len(), 1, "one running instance");
    assert_eq!(running[0].slug, "enum-1");
    assert_eq!(running[0].status, RunStatus::Running);

    let _ = kill_instance(&registry, "enum-1");
}

/// Exit recovery: after the child exits the entry is RETAINED with a terminal
/// status + buffered exit code, and `get_run_logs` replays buffered lines even
/// though the exit happened earlier.
#[tokio::test]
async fn cp6_exit_recovery_state_and_logs_replayable() {
    use std::sync::Arc;

    let tmp = tempfile::TempDir::new().unwrap();
    let inst_dir = make_instance_dir_named(&tmp, "recover-1");
    let game_dir = tmp.path().join("mc");
    fs::create_dir_all(&game_dir).unwrap();

    let registry = Arc::new(new_running_registry());
    let sink = Arc::new(CapturingLaunchSink::new());

    let (java, argv) = echo_proc();
    spawn_instance(
        "recover-1".to_string(),
        inst_dir,
        game_dir,
        java,
        argv,
        Arc::clone(&registry),
        Arc::clone(&sink),
    )
    .await
    .expect("spawn must succeed");

    // Wait for the monitor to record a terminal status (entry retained).
    wait_until(&registry, 10, "instance reaches terminal status", |g| {
        g.get("recover-1")
            .map(|s| s.status.is_terminal())
            .unwrap_or(false)
    })
    .await;

    // get_run_state reflects terminal status + exit code 0 — well after exit.
    let state = get_run_state(&registry, "recover-1").expect("state retained after exit");
    assert_eq!(state.status, RunStatus::Exited);
    assert_eq!(state.exit_code, Some(0));

    // Logs replay the buffered "hello" line even though exit already happened.
    let logs = get_run_logs(&registry, "recover-1").expect("logs retained after exit");
    assert!(
        logs.iter().any(|l| l.line.to_lowercase().contains("hello")),
        "replayed logs must contain 'hello': {logs:?}"
    );

    // Terminal instances are not "running".
    assert!(
        list_running(&registry).is_empty(),
        "terminal instance must not be listed as running"
    );
}

/// kill_instance records the exit: terminal status `Killed`.
#[tokio::test]
async fn cp6_kill_records_terminal_killed_status() {
    use std::sync::Arc;

    let tmp = tempfile::TempDir::new().unwrap();
    let inst_dir = make_instance_dir_named(&tmp, "kill-rec");
    let game_dir = tmp.path().join("mc");
    fs::create_dir_all(&game_dir).unwrap();

    let registry = Arc::new(new_running_registry());
    let sink = Arc::new(CapturingLaunchSink::new());

    let (java, argv) = sleep_proc();
    spawn_instance(
        "kill-rec".to_string(),
        inst_dir,
        game_dir,
        java,
        argv,
        Arc::clone(&registry),
        Arc::clone(&sink),
    )
    .await
    .expect("spawn must succeed");

    kill_instance(&registry, "kill-rec").expect("kill must succeed");

    wait_until(&registry, 10, "instance reaches terminal after kill", |g| {
        g.get("kill-rec")
            .map(|s| s.status.is_terminal())
            .unwrap_or(false)
    })
    .await;

    let state = get_run_state(&registry, "kill-rec").expect("state retained after kill");
    assert_eq!(state.status, RunStatus::Killed, "kill → terminal Killed");
}

/// Log ring caps at the configured maximum, dropping the oldest lines.
#[test]
fn cp6_log_ring_caps_and_drops_oldest() {
    let mut state = RunState::new_preparing();
    let total = LOG_RING_CAP + 50;
    for i in 0..total {
        state.push_log("stdout", &format!("line-{i}"));
    }
    assert_eq!(
        state.log_ring.len(),
        LOG_RING_CAP,
        "ring must cap at LOG_RING_CAP"
    );
    // Oldest 50 dropped; the first retained line is `line-50`.
    assert_eq!(state.log_ring.front().unwrap().line, "line-50");
    assert_eq!(
        state.log_ring.back().unwrap().line,
        format!("line-{}", total - 1)
    );
}

/// Helper: write an instance manifest under `<tmp>/<slug>/instance.json` and
/// return that per-instance dir. Distinct slugs let several instances coexist
/// under one temp root in the concurrency tests.
fn make_instance_dir_named(tmp: &tempfile::TempDir, slug: &str) -> std::path::PathBuf {
    use crate::core::instances::{Instance, JavaCfg, Loader, SCHEMA_VERSION};
    use std::io::Write as _;

    let inst = Instance {
        schema: SCHEMA_VERSION,
        id: format!("id-{slug}"),
        name: slug.to_string(),
        slug: slug.to_string(),
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
            min_memory_mb: None,
            path_override: None,
            use_pack_settings: false,
        },
        source: None,
        pack_locked: false,
        mods: vec![],
        created: "2024-01-01T00:00:00+00:00".to_string(),
        last_played: None,
        total_playtime_sec: 0,
    };

    let dir = tmp.path().join(slug);
    fs::create_dir_all(&dir).unwrap();
    let json = serde_json::to_string_pretty(&inst).unwrap();
    let mut f = fs::File::create(dir.join("instance.json")).unwrap();
    f.write_all(json.as_bytes()).unwrap();
    dir
}

// -----------------------------------------------------------------------
// A-3 — EffectiveJava heap + extra args in build_argv
// -----------------------------------------------------------------------

use crate::core::java_resolve::EffectiveJava;

/// Construct a minimal EffectiveJava with default values for fields not under test.
fn eff_java(xmx_mb: u32, xms_mb: Option<u32>, extra_args: Vec<&str>) -> EffectiveJava {
    EffectiveJava {
        xmx_mb,
        xms_mb,
        extra_args: extra_args.into_iter().map(str::to_owned).collect(),
        java_path: None,
    }
}

/// Minimal meta that satisfies build_argv without placeholder errors.
fn minimal_meta() -> LaunchMeta {
    make_meta(
        "release",
        vec!["-cp", "${classpath}"],
        vec![],
        vec!["/data/versions/1.21.1/1.21.1.jar"],
        false,
        None,
    )
}

/// -Xmx4096M must appear in argv when xmx_mb = 4096.
/// -Xms* must NOT appear when xms_mb = None.
#[test]
fn a3_xmx_present_xms_absent_when_none() {
    let meta = minimal_meta();
    let paths = make_paths();
    let eff = eff_java(4096, None, vec![]);

    let argv = build_argv(&meta, &paths, &offline_identity(), &eff).expect("no error");

    assert!(
        argv.iter().any(|a| a == "-Xmx4096M"),
        "-Xmx4096M must appear in argv: {argv:?}"
    );
    assert!(
        !argv.iter().any(|a| a.starts_with("-Xms")),
        "-Xms must NOT appear when xms_mb is None: {argv:?}"
    );
}

/// -Xms2048M must appear when xms_mb = Some(2048).
#[test]
fn a3_xms_present_when_some() {
    let meta = minimal_meta();
    let paths = make_paths();
    let eff = eff_java(4096, Some(2048), vec![]);

    let argv = build_argv(&meta, &paths, &offline_identity(), &eff).expect("no error");

    assert!(
        argv.iter().any(|a| a == "-Xms2048M"),
        "-Xms2048M must appear in argv when xms_mb is Some(2048): {argv:?}"
    );
}

/// extra_args tokens appear in argv.
#[test]
fn a3_extra_args_appear_in_argv() {
    let meta = minimal_meta();
    let paths = make_paths();
    let eff = eff_java(2048, None, vec!["-XX:+UseG1GC", "-XX:MaxGCPauseMillis=50"]);

    let argv = build_argv(&meta, &paths, &offline_identity(), &eff).expect("no error");

    assert!(
        argv.iter().any(|a| a == "-XX:+UseG1GC"),
        "-XX:+UseG1GC must appear in argv: {argv:?}"
    );
    assert!(
        argv.iter().any(|a| a == "-XX:MaxGCPauseMillis=50"),
        "-XX:MaxGCPauseMillis=50 must appear in argv: {argv:?}"
    );
}

/// Heap args (-Xmx, -Xms, extra_args) must all precede main_class in argv ordering.
#[test]
fn a3_heap_args_precede_main_class() {
    let meta = minimal_meta();
    let paths = make_paths();
    let eff = eff_java(4096, Some(2048), vec!["-XX:+UseG1GC"]);

    let argv = build_argv(&meta, &paths, &offline_identity(), &eff).expect("no error");

    let mc_idx = argv
        .iter()
        .position(|a| a == "net.minecraft.client.main.Main")
        .expect("main_class must be in argv");

    let xmx_idx = argv.iter().position(|a| a == "-Xmx4096M").expect("-Xmx4096M must be in argv");
    let xms_idx = argv.iter().position(|a| a == "-Xms2048M").expect("-Xms2048M must be in argv");
    let g1_idx = argv.iter().position(|a| a == "-XX:+UseG1GC").expect("-XX:+UseG1GC must be in argv");

    assert!(xmx_idx < mc_idx, "-Xmx must precede main_class (xmx={xmx_idx}, mc={mc_idx})");
    assert!(xms_idx < mc_idx, "-Xms must precede main_class (xms={xms_idx}, mc={mc_idx})");
    assert!(g1_idx < mc_idx, "extra_args must precede main_class (g1={g1_idx}, mc={mc_idx})");
}

/// Legacy manifest (empty jvm_args) + EffectiveJava still assembles a valid argv.
/// The default jvm_args are injected and no raw placeholder survives.
#[test]
fn a3_legacy_empty_jvm_args_plus_effective_java_valid() {
    // Legacy: jvm_args empty; game_args use the old-style minecraftArguments placeholders.
    let game_args = vec![
        "--username", "${auth_player_name}",
        "--version", "${version_name}",
        "--gameDir", "${game_directory}",
        "--assetsDir", "${assets_root}",
        "--assetIndex", "${assets_index_name}",
        "--uuid", "${auth_uuid}",
        "--accessToken", "${auth_access_token}",
        "--userType", "${user_type}",
    ];
    let meta = make_meta(
        "release",
        vec![], // empty → legacy path
        game_args,
        vec!["/data/libraries/a.jar", "/data/versions/1.8.9/1.8.9.jar"],
        false,
        None,
    );
    let paths = make_paths();
    let eff = eff_java(1024, None, vec![]);

    let argv = build_argv(&meta, &paths, &offline_identity(), &eff).expect("no error on legacy+effective");

    // -Xmx must appear.
    assert!(argv.iter().any(|a| a == "-Xmx1024M"), "-Xmx1024M must appear: {argv:?}");
    // Default classpath must also be present (legacy branch injects it).
    assert!(argv.iter().any(|a| a == "-cp"), "-cp must be injected for legacy manifest: {argv:?}");
    // No raw placeholders.
    for arg in &argv {
        assert!(!arg.contains("${"), "raw placeholder in legacy+effective argv: {arg}");
    }
}
