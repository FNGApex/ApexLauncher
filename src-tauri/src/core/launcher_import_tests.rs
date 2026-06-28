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
