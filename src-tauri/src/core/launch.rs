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
//! CP3 (next): tokio::process spawn + log streaming + running registry + kill.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

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
    /// Absolute path to `<data>/assets/`.
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
    /// Construct standard paths from an app data dir and instance slug.
    ///
    /// `data_dir` — the Tauri app data directory.
    /// `instances_dir` — the directory that holds all instance subdirs.
    /// `slug` — the instance slug (subdirectory name under `instances_dir`).
    pub fn new(data_dir: &Path, instances_dir: &Path, slug: &str) -> Self {
        Self {
            game_directory: instances_dir.join(slug).join("mc"),
            assets_root: data_dir.join("assets"),
            natives_directory: instances_dir.join(slug).join("natives"),
            legacy_assets_root: data_dir.join("assets").join("virtual").join("legacy"),
        }
    }
}

// ---------------------------------------------------------------------------
// Argv assembler
// ---------------------------------------------------------------------------

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

/// Build the full JVM argv for the given `LaunchMeta` + resolved paths.
///
/// Returns `[<substituted jvm_args>, main_class, <substituted game_args>]`.
///
/// Any `${...}` placeholder not in the substitution table causes an
/// [`AssembleError::UnsubstitutedPlaceholders`] rather than passing raw text
/// to the JVM.
///
/// When `launch.jvm_args` is empty (legacy manifests), a minimal set of
/// default JVM args is prepended so the JVM can start.
pub fn build_argv(launch: &LaunchMeta, paths: &LaunchPaths) -> Result<Vec<String>, AssembleError> {
    let classpath = build_classpath(&launch.classpath);
    let uuid = offline_uuid();

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
        ("${auth_player_name}", OFFLINE_PLAYER_NAME.to_string()),
        ("${auth_uuid}", uuid.as_hyphenated().to_string()),
        ("${auth_access_token}", "0".to_string()),
        ("${user_type}", "msa".to_string()),
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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

        let argv = build_argv(&meta, &paths).expect("no unresolved placeholders");

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

        let err = build_argv(&meta, &paths).expect_err("must error on unknown placeholder");
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

        let argv = build_argv(&meta, &paths).expect("no error expected");
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

        let argv = build_argv(&meta, &paths).expect("no error expected");
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

        let argv = build_argv(&meta, &paths).expect("no error expected");

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

        let argv = build_argv(&meta, &paths).expect("no error");
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

        let argv = build_argv(&meta, &paths).expect("no error");
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

        let argv = build_argv(&meta, &paths).expect("no error");
        assert!(
            argv.iter().any(|a| a == "snapshot"),
            "snapshot version_type must appear in argv: {argv:?}"
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
}
