//! Unit tests for `launcher_import`. Wired via
//! `#[cfg(test)] #[path = "launcher_import_tests.rs"] mod tests;`.
//! All tests are pure — no I/O, no network.

use super::*;

// ── CP-1 fixtures ─────────────────────────────────────────────────────────────

const PRISM_INSTANCE: &str = include_str!("fixtures/prism_instance.cfg");
const PRISM_INSTANCE_HEADER: &str = include_str!("fixtures/prism_instance_general_header.cfg");
const PRISM_INSTANCE_LEGACY: &str = include_str!("fixtures/prism_instance_legacy.cfg");

// ── CP-1: parse_instance_cfg ──────────────────────────────────────────────────

#[test]
fn cp1_name_and_icon_key_parsed() {
    let cfg = parse_instance_cfg(PRISM_INSTANCE).expect("should parse");
    assert_eq!(cfg.name.as_deref(), Some("My Fabric Instance"));
    assert_eq!(cfg.icon_key.as_deref(), Some("myfabric"));
}

#[test]
fn cp1_instance_type_captured() {
    let cfg = parse_instance_cfg(PRISM_INSTANCE).expect("should parse");
    assert_eq!(cfg.instance_type.as_deref(), Some("OneSix"));
}

#[test]
fn cp1_gated_memory_parsed() {
    let cfg = parse_instance_cfg(PRISM_INSTANCE).expect("should parse");
    assert!(cfg.override_memory, "OverrideMemory=true must set override_memory");
    assert_eq!(cfg.min_mem_mb, Some(512));
    assert_eq!(cfg.max_mem_mb, Some(8192));
}

#[test]
fn cp1_gated_java_location_parsed() {
    let cfg = parse_instance_cfg(PRISM_INSTANCE).expect("should parse");
    assert!(cfg.override_java_location, "OverrideJavaLocation=true must set flag");
    assert_eq!(cfg.java_path.as_deref(), Some("/usr/lib/jvm/java-21/bin/java"));
}

#[test]
fn cp1_gated_java_args_parsed() {
    let cfg = parse_instance_cfg(PRISM_INSTANCE).expect("should parse");
    assert!(cfg.override_java_args, "OverrideJavaArgs=true must set flag");
    assert_eq!(cfg.jvm_args.as_deref(), Some("-XX:+UseG1GC -Xss1M"));
}

#[test]
fn cp1_general_header_tolerated() {
    let cfg = parse_instance_cfg(PRISM_INSTANCE_HEADER)
        .expect("[General] section header must not cause a parse failure");
    assert_eq!(cfg.name.as_deref(), Some("Pack With Header"));
    assert_eq!(cfg.icon_key.as_deref(), Some("flames"));
    assert_eq!(cfg.instance_type.as_deref(), Some("OneSix"));
}

#[test]
fn cp1_general_header_override_false_parsed() {
    // OverrideMemory=false in the header fixture — gates must be false.
    let cfg = parse_instance_cfg(PRISM_INSTANCE_HEADER).expect("should parse");
    assert!(!cfg.override_memory);
    assert!(!cfg.override_java_location);
    assert!(!cfg.override_java_args);
}

#[test]
fn cp1_legacy_instance_type_captured_not_rejected() {
    // parse_instance_cfg captures Legacy; the JOB layer (CP-5) is responsible for rejection.
    let cfg = parse_instance_cfg(PRISM_INSTANCE_LEGACY)
        .expect("parse must succeed even for Legacy instances");
    assert_eq!(cfg.instance_type.as_deref(), Some("Legacy"));
    assert_eq!(cfg.name.as_deref(), Some("Old Legacy Pack"));
}

#[test]
fn cp1_unknown_keys_silently_ignored() {
    let text = "name=Test Instance\nSomeUnknownKey=abc\nAnotherKey=123\n";
    let cfg =
        parse_instance_cfg(text).expect("unknown keys must not cause an error");
    assert_eq!(cfg.name.as_deref(), Some("Test Instance"));
}

#[test]
fn cp1_missing_optional_fields_default_correctly() {
    let text = "name=Minimal\n";
    let cfg = parse_instance_cfg(text).expect("should parse minimal file");
    assert_eq!(cfg.name.as_deref(), Some("Minimal"));
    assert!(cfg.icon_key.is_none());
    assert!(cfg.instance_type.is_none());
    assert!(!cfg.override_memory);
    assert!(cfg.min_mem_mb.is_none());
    assert!(cfg.max_mem_mb.is_none());
    assert!(!cfg.override_java_location);
    assert!(cfg.java_path.is_none());
    assert!(!cfg.override_java_args);
    assert!(cfg.jvm_args.is_none());
}

#[test]
fn cp1_jvm_args_with_equals_sign_parsed_whole() {
    // JvmArgs may contain `=` (e.g. -Dproperty=value); split_once('=') must not truncate.
    let text = "JvmArgs=-Dfoo=bar -Xmx4G\n";
    let cfg = parse_instance_cfg(text).expect("should parse");
    assert_eq!(cfg.jvm_args.as_deref(), Some("-Dfoo=bar -Xmx4G"));
}

#[test]
fn cp1_empty_input_returns_default_cfg() {
    let cfg = parse_instance_cfg("").expect("empty input must not error");
    assert!(cfg.name.is_none());
    assert!(!cfg.override_memory);
}

// ── CP-2 fixtures ─────────────────────────────────────────────────────────────

const MMC_PACK_FABRIC: &str = include_str!("fixtures/mmc_pack_fabric.json");
const MMC_PACK_QUILT: &str = include_str!("fixtures/mmc_pack_quilt.json");
const MMC_PACK_NEOFORGE: &str = include_str!("fixtures/mmc_pack_neoforge.json");
const MMC_PACK_FORGE: &str = include_str!("fixtures/mmc_pack_forge.json");
const MMC_PACK_VANILLA: &str = include_str!("fixtures/mmc_pack_vanilla.json");
const MMC_PACK_LITELOADER: &str = include_str!("fixtures/mmc_pack_liteloader.json");

// ── CP-2: parse_mmc_pack ─────────────────────────────────────────────────────

#[test]
fn cp2_fabric_loader_uid_maps_to_fabric() {
    let pack = parse_mmc_pack(MMC_PACK_FABRIC).expect("should parse");
    assert_eq!(pack.minecraft, "1.21.1");
    match pack.loader {
        ImportedLoader::Loader { kind, version } => {
            assert_eq!(kind, "fabric");
            assert_eq!(version, "0.16.9");
        }
        other => panic!("expected Loader{{fabric}}, got {:?}", other),
    }
}

#[test]
fn cp2_quilt_loader_uid_maps_to_quilt() {
    let pack = parse_mmc_pack(MMC_PACK_QUILT).expect("should parse");
    assert_eq!(pack.minecraft, "1.20.6");
    match pack.loader {
        ImportedLoader::Loader { kind, version } => {
            assert_eq!(kind, "quilt");
            assert_eq!(version, "0.27.1");
        }
        other => panic!("expected Loader{{quilt}}, got {:?}", other),
    }
}

#[test]
fn cp2_neoforge_loader_uid_maps_to_neoforge() {
    let pack = parse_mmc_pack(MMC_PACK_NEOFORGE).expect("should parse");
    assert_eq!(pack.minecraft, "1.21.1");
    match pack.loader {
        ImportedLoader::Loader { kind, version } => {
            assert_eq!(kind, "neoforge");
            assert_eq!(version, "21.1.209");
        }
        other => panic!("expected Loader{{neoforge}}, got {:?}", other),
    }
}

#[test]
fn cp2_forge_loader_uid_maps_to_forge() {
    let pack = parse_mmc_pack(MMC_PACK_FORGE).expect("should parse");
    assert_eq!(pack.minecraft, "1.20.1");
    match pack.loader {
        ImportedLoader::Loader { kind, version } => {
            assert_eq!(kind, "forge");
            assert_eq!(version, "47.2.0");
        }
        other => panic!("expected Loader{{forge}}, got {:?}", other),
    }
}

#[test]
fn cp2_no_loader_component_yields_vanilla() {
    let pack = parse_mmc_pack(MMC_PACK_VANILLA).expect("should parse");
    assert_eq!(pack.minecraft, "1.21.4");
    assert!(
        matches!(pack.loader, ImportedLoader::Vanilla),
        "no loader component must yield Vanilla, got {:?}",
        pack.loader
    );
}

#[test]
fn cp2_liteloader_uid_yields_unsupported() {
    let pack = parse_mmc_pack(MMC_PACK_LITELOADER).expect("should parse");
    assert_eq!(pack.minecraft, "1.12.2");
    match pack.loader {
        ImportedLoader::Unsupported(name) => {
            assert_eq!(name, "liteloader", "unsupported name must be 'liteloader'");
        }
        other => panic!("expected Unsupported(liteloader), got {:?}", other),
    }
}

#[test]
fn cp2_intermediary_with_dependency_only_is_ignored() {
    // The fabric fixture has intermediary as dependencyOnly; the loader must be fabric.
    let pack = parse_mmc_pack(MMC_PACK_FABRIC).expect("should parse");
    match &pack.loader {
        ImportedLoader::Loader { kind, .. } => {
            assert_eq!(kind, "fabric", "intermediary must be ignored; fabric must win");
        }
        other => panic!("expected Loader{{fabric}}, got {:?}", other),
    }
}

#[test]
fn cp2_lwjgl3_uid_is_ignored_by_uid() {
    // vanilla fixture has org.lwjgl3 WITHOUT dependencyOnly — must still be ignored by uid.
    let pack = parse_mmc_pack(MMC_PACK_VANILLA).expect("should parse");
    assert!(
        matches!(pack.loader, ImportedLoader::Vanilla),
        "org.lwjgl3 must be ignored by uid; got {:?}",
        pack.loader
    );
}

#[test]
fn cp2_dependency_only_flag_skips_any_uid() {
    // Even a loader uid is skipped when dependencyOnly=true.
    let json = r#"{
        "formatVersion": 1,
        "components": [
            { "uid": "net.minecraft", "version": "1.21.1" },
            { "uid": "net.fabricmc.fabric-loader", "version": "0.16.9", "dependencyOnly": true }
        ]
    }"#;
    let pack = parse_mmc_pack(json).expect("should parse");
    assert!(
        matches!(pack.loader, ImportedLoader::Vanilla),
        "dependencyOnly fabric-loader must be skipped; got {:?}",
        pack.loader
    );
}

#[test]
fn cp2_java_component_uid_is_ignored() {
    let json = r#"{
        "formatVersion": 1,
        "components": [
            { "uid": "net.minecraft", "version": "1.21.1" },
            { "uid": "net.adoptium.java", "version": "21.0.3" },
            { "uid": "com.azul.java", "version": "17.0.9" }
        ]
    }"#;
    let pack = parse_mmc_pack(json).expect("should parse");
    assert!(
        matches!(pack.loader, ImportedLoader::Vanilla),
        "*.java components must be ignored; got {:?}",
        pack.loader
    );
}

#[test]
fn cp2_missing_minecraft_component_returns_error() {
    let json = r#"{
        "formatVersion": 1,
        "components": [
            { "uid": "net.fabricmc.fabric-loader", "version": "0.16.9" }
        ]
    }"#;
    let result = parse_mmc_pack(json);
    assert!(result.is_err(), "missing net.minecraft must produce an error");
    match result.unwrap_err() {
        LauncherImportError::MissingField(field) => {
            assert_eq!(field, "net.minecraft");
        }
        other => panic!("expected MissingField(net.minecraft), got {:?}", other),
    }
}

#[test]
fn cp2_empty_components_array_returns_error() {
    let json = r#"{ "formatVersion": 1, "components": [] }"#;
    let result = parse_mmc_pack(json);
    assert!(result.is_err(), "no components at all must error (no net.minecraft)");
}

#[test]
fn cp2_forge_legacy_doubled_version_yields_unsupported() {
    // Ancient 1.7.10 Forge uses "mc-build-mc" form — contains `-` → Unsupported.
    let json = r#"{
        "formatVersion": 1,
        "components": [
            { "uid": "net.minecraft", "version": "1.7.10" },
            { "uid": "net.minecraftforge", "version": "1.7.10-10.13.4.1614-1.7.10" }
        ]
    }"#;
    let pack = parse_mmc_pack(json).expect("should parse (not crash)");
    assert_eq!(pack.minecraft, "1.7.10");
    match pack.loader {
        ImportedLoader::Unsupported(name) => {
            assert_eq!(name, "forge-legacy", "legacy forge must map to the exact label");
        }
        other => panic!("expected Unsupported for legacy forge, got {:?}", other),
    }
}

#[test]
fn cp2_neoforge_beta_version_passes_through_verbatim() {
    // NeoForge legitimately ships `-beta`/`-alpha` versions; the `-` is NOT a
    // legacy marker (unlike Forge). The version must pass through unchanged so
    // the launch-time installer resolves the real `neoforge-<v>` artifact.
    let json = r#"{
        "formatVersion": 1,
        "components": [
            { "uid": "net.minecraft", "version": "1.21.1" },
            { "uid": "net.neoforged", "version": "21.1.209-beta" }
        ]
    }"#;
    let pack = parse_mmc_pack(json).expect("neoforge beta must parse");
    match pack.loader {
        ImportedLoader::Loader { kind, version } => {
            assert_eq!(kind, "neoforge");
            assert_eq!(version, "21.1.209-beta", "beta suffix must be preserved verbatim");
        }
        other => panic!("expected a neoforge Loader, got {:?}", other),
    }
}

#[test]
fn cp2_empty_minecraft_version_returns_error() {
    // An explicit empty `net.minecraft` version is caught like an absent one.
    let json = r#"{
        "formatVersion": 1,
        "components": [ { "uid": "net.minecraft", "version": "" } ]
    }"#;
    assert!(
        matches!(parse_mmc_pack(json), Err(LauncherImportError::MissingField(_))),
        "empty net.minecraft version must error, not yield an empty MC string"
    );
}

#[test]
fn cp1_non_numeric_memory_value_returns_malformed_field() {
    // A corrupt integer key surfaces a MalformedField error (not a silent default).
    let cfg = "MaxMemAlloc=not-a-number\n";
    assert!(
        matches!(
            parse_instance_cfg(cfg),
            Err(LauncherImportError::MalformedField { .. })
        ),
        "non-numeric MaxMemAlloc must return MalformedField"
    );
}

#[test]
fn cp2_first_loader_uid_wins_when_multiple_present() {
    // Unusual: two loader uids — first in array order wins.
    let json = r#"{
        "formatVersion": 1,
        "components": [
            { "uid": "net.minecraft", "version": "1.21.1" },
            { "uid": "net.fabricmc.fabric-loader", "version": "0.16.9" },
            { "uid": "org.quiltmc.quilt-loader", "version": "0.27.1" }
        ]
    }"#;
    let pack = parse_mmc_pack(json).expect("should parse");
    match pack.loader {
        ImportedLoader::Loader { kind, .. } => {
            assert_eq!(kind, "fabric", "first loader uid in array must win");
        }
        other => panic!("expected Loader{{fabric}}, got {:?}", other),
    }
}

#[test]
fn cp2_malformed_json_returns_error() {
    let result = parse_mmc_pack("not json at all");
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), LauncherImportError::MalformedMmcPack(_)));
}

// ── CP-3 + CP-4 tests ─────────────────────────────────────────────────────────

use std::path::Path;
use tempfile::TempDir;

// ── CP-3: resolve_game_dir ────────────────────────────────────────────────────

#[test]
fn cp3_resolve_game_dir_prefers_dot_minecraft() {
    let tmp = TempDir::new().unwrap();
    let inst = tmp.path().join("inst");
    std::fs::create_dir_all(inst.join(".minecraft")).unwrap();
    std::fs::create_dir_all(inst.join("minecraft")).unwrap();
    let result = resolve_game_dir(&inst).unwrap();
    assert_eq!(result, inst.join(".minecraft"));
}

#[test]
fn cp3_resolve_game_dir_falls_back_to_minecraft() {
    let tmp = TempDir::new().unwrap();
    let inst = tmp.path().join("inst");
    std::fs::create_dir_all(inst.join("minecraft")).unwrap();
    let result = resolve_game_dir(&inst).unwrap();
    assert_eq!(result, inst.join("minecraft"));
}

#[test]
fn cp3_resolve_game_dir_missing_returns_error() {
    let tmp = TempDir::new().unwrap();
    let inst = tmp.path().join("inst");
    std::fs::create_dir_all(&inst).unwrap();
    let result = resolve_game_dir(&inst);
    assert!(result.is_err(), "no game dir must return an error");
    assert!(
        matches!(result.unwrap_err(), LauncherImportError::NoGameDir { .. }),
        "must be NoGameDir error"
    );
}

// ── CP-3: copy_game_dir ───────────────────────────────────────────────────────

/// Build a standard game-dir source tree for copy tests.
///
/// Creates: mods/x.jar, mods/x.jar.disabled, config/sub/y.toml, .options,
/// logs/latest.log, crash-reports/crash.txt (6 files total).
fn make_src_tree(root: &Path) {
    let mods = root.join("mods");
    std::fs::create_dir_all(&mods).unwrap();
    std::fs::write(mods.join("x.jar"), b"jar").unwrap();
    std::fs::write(mods.join("x.jar.disabled"), b"disabled").unwrap();

    let config_sub = root.join("config").join("sub");
    std::fs::create_dir_all(&config_sub).unwrap();
    std::fs::write(config_sub.join("y.toml"), b"toml").unwrap();

    std::fs::write(root.join(".options"), b"opts").unwrap();

    let logs = root.join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    std::fs::write(logs.join("latest.log"), b"log").unwrap();

    let crash = root.join("crash-reports");
    std::fs::create_dir_all(&crash).unwrap();
    std::fs::write(crash.join("crash.txt"), b"crash").unwrap();
}

#[test]
fn cp3_copy_game_dir_preserves_tree_structure() {
    let src_tmp = TempDir::new().unwrap();
    let src = src_tmp.path();
    make_src_tree(src);

    let dest_tmp = TempDir::new().unwrap();
    let dest = dest_tmp.path().join("out");
    let count = copy_game_dir(src, &dest, false).unwrap();

    assert!(dest.join("mods").join("x.jar").exists(), "mods/x.jar must be copied");
    assert!(
        dest.join("mods").join("x.jar.disabled").exists(),
        ".disabled file must be copied"
    );
    assert!(
        dest.join("config").join("sub").join("y.toml").exists(),
        "config/sub/y.toml must be copied"
    );
    assert!(dest.join(".options").exists(), "dotfile must be copied");
    assert!(
        dest.join("logs").join("latest.log").exists(),
        "logs must be present when skip_logs=false"
    );
    assert!(
        dest.join("crash-reports").join("crash.txt").exists(),
        "crash-reports must be present when skip_logs=false"
    );
    // 6 files: x.jar, x.jar.disabled, y.toml, .options, latest.log, crash.txt
    assert_eq!(count, 6, "must count all 6 files");
}

#[test]
fn cp3_copy_game_dir_skip_logs_omits_logs_and_crash_reports() {
    let src_tmp = TempDir::new().unwrap();
    let src = src_tmp.path();
    make_src_tree(src);

    let dest_tmp = TempDir::new().unwrap();
    let dest = dest_tmp.path().join("out");
    let count = copy_game_dir(src, &dest, true).unwrap();

    assert!(!dest.join("logs").exists(), "logs/ must be skipped when skip_logs=true");
    assert!(
        !dest.join("crash-reports").exists(),
        "crash-reports/ must be skipped when skip_logs=true"
    );
    // 4 files remain: x.jar, x.jar.disabled, y.toml, .options
    assert_eq!(count, 4, "4 files after skipping logs and crash-reports");
}

#[test]
fn cp3_copy_game_dir_skip_logs_false_keeps_all() {
    let src_tmp = TempDir::new().unwrap();
    let src = src_tmp.path();
    make_src_tree(src);

    let dest_tmp = TempDir::new().unwrap();
    let dest = dest_tmp.path().join("out");
    let count = copy_game_dir(src, &dest, false).unwrap();

    assert!(dest.join("logs").join("latest.log").exists());
    assert!(dest.join("crash-reports").join("crash.txt").exists());
    assert_eq!(count, 6);
}

#[test]
fn cp3_copy_game_dir_returns_correct_file_count() {
    let src_tmp = TempDir::new().unwrap();
    let src = src_tmp.path();
    std::fs::write(src.join("a.txt"), b"a").unwrap();
    std::fs::write(src.join("b.txt"), b"b").unwrap();
    let sub = src.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("c.txt"), b"c").unwrap();

    let dest_tmp = TempDir::new().unwrap();
    let dest = dest_tmp.path().join("out");
    let count = copy_game_dir(src, &dest, false).unwrap();
    assert_eq!(count, 3, "must count all files recursively");
}

#[test]
fn cp3_copy_game_dir_disabled_jars_are_copied() {
    let src_tmp = TempDir::new().unwrap();
    let src = src_tmp.path();
    let mods = src.join("mods");
    std::fs::create_dir_all(&mods).unwrap();
    std::fs::write(mods.join("mod.jar"), b"active").unwrap();
    std::fs::write(mods.join("mod.jar.disabled"), b"disabled").unwrap();

    let dest_tmp = TempDir::new().unwrap();
    let dest = dest_tmp.path().join("out");
    let count = copy_game_dir(src, &dest, false).unwrap();
    assert!(dest.join("mods").join("mod.jar").exists());
    assert!(dest.join("mods").join("mod.jar.disabled").exists());
    assert_eq!(count, 2);
}

/// On Unix, symlinks pointing outside the source tree must be silently skipped —
/// not followed, and not materialized in the dest.
#[cfg(unix)]
#[test]
fn cp3_symlinks_outside_tree_are_skipped() {
    use std::os::unix::fs::symlink;

    let outer_tmp = TempDir::new().unwrap();
    let src_tmp = TempDir::new().unwrap();
    let src = src_tmp.path();

    std::fs::write(src.join("real.txt"), b"real").unwrap();
    // Symlink to a directory outside the source tree.
    symlink(outer_tmp.path(), src.join("escape")).unwrap();

    let dest_tmp = TempDir::new().unwrap();
    let dest = dest_tmp.path().join("out");
    let count = copy_game_dir(src, &dest, false).unwrap();

    assert!(dest.join("real.txt").exists(), "real file must be copied");
    assert!(!dest.join("escape").exists(), "symlink must not appear in dest");
    assert_eq!(count, 1, "only the real file counts");
}

// ── CP-4: resolve_icon_path ───────────────────────────────────────────────────

/// Build `<root>/instances/Inst/` and return `(root_path, inst_dir)`.
fn make_prism_layout(tmp: &TempDir) -> (std::path::PathBuf, std::path::PathBuf) {
    let root = tmp.path().to_path_buf();
    let inst_dir = root.join("instances").join("Inst");
    std::fs::create_dir_all(&inst_dir).unwrap();
    (root, inst_dir)
}

#[test]
fn cp4_resolve_icon_path_finds_png() {
    let tmp = TempDir::new().unwrap();
    let (root, inst_dir) = make_prism_layout(&tmp);
    let icons_dir = root.join("icons");
    std::fs::create_dir_all(&icons_dir).unwrap();
    let png = icons_dir.join("myicon.png");
    std::fs::write(&png, b"png").unwrap();

    let result = resolve_icon_path(&inst_dir, "myicon");
    assert_eq!(result, Some(png));
}

#[test]
fn cp4_resolve_icon_path_prefers_first_ext_in_allowlist() {
    // Both .png and .jpg exist; .png must win (comes first in ICON_EXTS order).
    let tmp = TempDir::new().unwrap();
    let (root, inst_dir) = make_prism_layout(&tmp);
    let icons_dir = root.join("icons");
    std::fs::create_dir_all(&icons_dir).unwrap();
    std::fs::write(icons_dir.join("icon.png"), b"png").unwrap();
    std::fs::write(icons_dir.join("icon.jpg"), b"jpg").unwrap();

    let result = resolve_icon_path(&inst_dir, "icon");
    assert_eq!(result, Some(icons_dir.join("icon.png")));
}

#[test]
fn cp4_resolve_icon_path_returns_none_for_missing_file() {
    let tmp = TempDir::new().unwrap();
    let (_root, inst_dir) = make_prism_layout(&tmp);
    // No icons/ dir, no file.
    let result = resolve_icon_path(&inst_dir, "builtin_grass");
    assert_eq!(result, None);
}

#[test]
fn cp4_resolve_icon_path_returns_none_for_unsupported_ext() {
    let tmp = TempDir::new().unwrap();
    let (root, inst_dir) = make_prism_layout(&tmp);
    let icons_dir = root.join("icons");
    std::fs::create_dir_all(&icons_dir).unwrap();
    // Only a .bmp — not in the allowlist.
    std::fs::write(icons_dir.join("myicon.bmp"), b"bmp").unwrap();

    let result = resolve_icon_path(&inst_dir, "myicon");
    assert_eq!(result, None);
}

#[test]
fn cp4_resolve_icon_path_returns_none_for_missing_icons_dir() {
    let tmp = TempDir::new().unwrap();
    let (_root, inst_dir) = make_prism_layout(&tmp);
    // icons/ dir is NOT created.
    let result = resolve_icon_path(&inst_dir, "anyicon");
    assert_eq!(result, None);
}

#[test]
fn cp4_resolve_icon_path_all_allowed_exts_are_found() {
    // Verify each remaining allowed extension (.jpg, .jpeg, .webp, .gif) is found.
    for ext in &["jpg", "jpeg", "webp", "gif"] {
        let tmp = TempDir::new().unwrap();
        let (root, inst_dir) = make_prism_layout(&tmp);
        let icons_dir = root.join("icons");
        std::fs::create_dir_all(&icons_dir).unwrap();
        let icon_file = icons_dir.join(format!("myicon.{}", ext));
        std::fs::write(&icon_file, b"img").unwrap();

        let result = resolve_icon_path(&inst_dir, "myicon");
        assert_eq!(result, Some(icon_file), "ext .{} should be found", ext);
    }
}

// ── Security: FIX 1 — icon_key_is_safe ───────────────────────────────────────

/// Verify that a bare single-component stem is accepted.
#[test]
fn sec_icon_key_safe_accepts_bare_stem() {
    assert!(icon_key_is_safe("myicon"));
    assert!(icon_key_is_safe("flames"));
    assert!(icon_key_is_safe("pack-icon_2"));
}

/// `..` alone or as first component must be rejected.
#[test]
fn sec_icon_key_safe_rejects_parent_dir() {
    assert!(!icon_key_is_safe(".."));
    assert!(!icon_key_is_safe("../evil"));
    assert!(!icon_key_is_safe("../../../etc/passwd"));
}

/// A leading `/` (absolute Unix path) must be rejected.
#[test]
fn sec_icon_key_safe_rejects_absolute_unix() {
    assert!(!icon_key_is_safe("/etc/passwd"));
}

/// A forward-slash inside the stem (path segment) must be rejected.
#[test]
fn sec_icon_key_safe_rejects_slash_path() {
    assert!(!icon_key_is_safe("a/b"));
}

/// A backslash or Windows-style path must be rejected.
#[test]
fn sec_icon_key_safe_rejects_backslash_path() {
    assert!(!icon_key_is_safe("a\\b"));
    assert!(!icon_key_is_safe("C:\\Windows\\x"));
}

/// A Windows drive-letter prefix must be rejected on all platforms.
#[test]
fn sec_icon_key_safe_rejects_drive_letter() {
    assert!(!icon_key_is_safe("C:foo"));
    assert!(!icon_key_is_safe("Z:"));
}

/// An empty key must be rejected (no filename to look up).
#[test]
fn sec_icon_key_safe_rejects_empty() {
    assert!(!icon_key_is_safe(""));
}

/// A legitimate icon key finds its file; a traversal key does NOT, even when a
/// file happens to exist at the escaped path.
///
/// Without the sanitization guard, `icons_dir.join("../evil.png")` resolves one
/// directory above `icons_dir` (i.e., to `data_root/evil.png`). If that file
/// exists, the un-patched code returns `Some(...)`. This test catches the BLOCKER.
#[test]
fn sec_icon_key_traversal_rejected_even_if_file_exists_at_escaped_path() {
    let tmp = TempDir::new().unwrap();
    let (root, inst_dir) = make_prism_layout(&tmp);

    // Create icons_dir so the OS can traverse into it (needed for `..` resolution).
    let icons_dir = root.join("icons");
    std::fs::create_dir_all(&icons_dir).unwrap();

    // Place a file ONE level above icons_dir — exactly where "../evil" would land.
    // data_root/evil.png  ==  icons_dir/../evil.png
    let escaped_file = root.join("evil.png");
    std::fs::write(&escaped_file, b"evil content").unwrap();

    // Without the fix: icons_dir.join("../evil.png").is_file() == true → returns Some.
    let result = resolve_icon_path(&inst_dir, "../evil");
    assert_eq!(
        result, None,
        "icon_key containing '..' must be rejected even when the escaped file exists"
    );

    // Sanity: a legitimate key that points to a real icon IS found.
    let legit_icon = icons_dir.join("myicon.png");
    std::fs::write(&legit_icon, b"png data").unwrap();
    assert_eq!(
        resolve_icon_path(&inst_dir, "myicon"),
        Some(legit_icon),
        "a clean icon_key with a real file must still be found"
    );
}

// ── Security: FIX 2 — is_reparse_or_symlink helper ───────────────────────────

/// On Windows, a regular file must NOT be reported as a reparse point.
///
/// Junction creation requires `mklink /J` (elevated or developer mode) so we
/// cannot create one in a hermetic unit test. The Windows-specific
/// `FILE_ATTRIBUTE_REPARSE_POINT` branch is therefore covered only by this
/// negative case and documented here.
#[cfg(windows)]
#[test]
fn sec_is_reparse_or_symlink_regular_file_returns_false() {
    let tmp = TempDir::new().unwrap();
    let f = tmp.path().join("regular.txt");
    std::fs::write(&f, b"data").unwrap();
    let meta = f.symlink_metadata().unwrap();
    assert!(
        !is_reparse_or_symlink(&meta),
        "a regular file must not be flagged as a reparse point"
    );
}

/// On Windows, a regular directory must NOT be reported as a reparse point.
#[cfg(windows)]
#[test]
fn sec_is_reparse_or_symlink_regular_dir_returns_false() {
    let tmp = TempDir::new().unwrap();
    let d = tmp.path().join("subdir");
    std::fs::create_dir_all(&d).unwrap();
    let meta = d.symlink_metadata().unwrap();
    assert!(
        !is_reparse_or_symlink(&meta),
        "a regular directory must not be flagged as a reparse point"
    );
}

// ── Security: FIX 4 — relative_path_is_safe hardening ───────────────────────

/// Drive-letter prefix `C:foo` must be rejected on all platforms.
///
/// On Linux, `Path::new("C:foo")` parses as a single `Normal("C:foo")` component,
/// so the old component-only check let it through. The string-level check catches it.
#[test]
fn sec_path_safety_rejects_drive_letter() {
    assert!(
        !relative_path_is_safe(Path::new("C:foo")),
        "C:foo must be rejected on all platforms"
    );
    assert!(
        !relative_path_is_safe(Path::new("Z:bar")),
        "Z:bar must be rejected on all platforms"
    );
}

/// UNC-style prefix `\\server\share` must be rejected on all platforms.
///
/// On Linux, the backslashes are ordinary filename characters and the path
/// parses as `Normal(r"\\server\share")`, slipping past the component check.
/// The string-level `starts_with('\\')` guard catches it.
#[test]
fn sec_path_safety_rejects_unc_prefix() {
    // Rust literal "\\\\server\\share" == the string \\server\share
    assert!(
        !relative_path_is_safe(Path::new("\\\\server\\share")),
        "UNC-style path must be rejected"
    );
}

/// Absolute Unix-style path must be rejected.
#[test]
fn sec_path_safety_rejects_absolute_unix() {
    assert!(!relative_path_is_safe(Path::new("/abs")));
}

/// `..` component must be rejected.
#[test]
fn sec_path_safety_rejects_parent_dir() {
    assert!(!relative_path_is_safe(Path::new("a/../b")));
    assert!(!relative_path_is_safe(Path::new("..")));
}

/// A normal relative path must be accepted.
#[test]
fn sec_path_safety_accepts_valid_relative() {
    assert!(relative_path_is_safe(Path::new("mods/x.jar")));
    assert!(relative_path_is_safe(Path::new("config/foo.toml")));
    assert!(relative_path_is_safe(Path::new("a")));
}
