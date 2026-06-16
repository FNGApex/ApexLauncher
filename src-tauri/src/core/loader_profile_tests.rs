//! Unit tests for `loader_profile`. Extracted from the source module; wired back
//! via `#[cfg(test)] #[path = "loader_profile_tests.rs"] mod tests;` so private items
//! remain accessible through `super::*`.

use super::*;
use crate::core::resolver::ArgumentEntry;

// --- Fixture parse -------------------------------------------------------

#[test]
fn fixture_parses_main_class() {
    let json = include_str!("fixtures/fabric_profile.json");
    let profile: LoaderProfile = serde_json::from_str(json).expect("fixture should parse");
    assert_eq!(
        profile.main_class,
        "net.fabricmc.loader.impl.launch.knot.KnotClient"
    );
}

#[test]
fn fixture_parses_libraries() {
    let json = include_str!("fixtures/fabric_profile.json");
    let profile: LoaderProfile = serde_json::from_str(json).expect("fixture should parse");
    assert!(
        !profile.libraries.is_empty(),
        "libraries should not be empty"
    );
    // First library: fabric-loader itself
    assert_eq!(
        profile.libraries[0].name,
        "net.fabricmc:fabric-loader:0.16.10"
    );
    assert_eq!(
        profile.libraries[0].url,
        Some("https://maven.fabricmc.net/".to_string())
    );
}

#[test]
fn fixture_parses_arguments() {
    let json = include_str!("fixtures/fabric_profile.json");
    let profile: LoaderProfile = serde_json::from_str(json).expect("fixture should parse");
    // JVM args: one entry "-DFabricMcEmu=net.minecraft.client.main.Main"
    assert_eq!(profile.arguments.jvm.len(), 1);
    match &profile.arguments.jvm[0] {
        ArgumentEntry::Plain(s) => assert_eq!(s, "-DFabricMcEmu=net.minecraft.client.main.Main"),
        other => panic!("expected Plain arg, got: {other:?}"),
    }
    // Game args: empty array
    assert_eq!(profile.arguments.game.len(), 0);
}

#[test]
fn fixture_has_five_libraries() {
    let json = include_str!("fixtures/fabric_profile.json");
    let profile: LoaderProfile = serde_json::from_str(json).expect("fixture should parse");
    assert_eq!(profile.libraries.len(), 5);
}

// --- profile_url ---------------------------------------------------------

#[test]
fn profile_url_fabric() {
    let url = profile_url("fabric", "1.21.1", "0.16.10").expect("fabric url should be ok");
    assert_eq!(
        url,
        "https://meta.fabricmc.net/v2/versions/loader/1.21.1/0.16.10/profile/json"
    );
}

#[test]
fn profile_url_quilt() {
    let url = profile_url("quilt", "1.21.1", "0.26.4").expect("quilt url should be ok");
    assert_eq!(
        url,
        "https://meta.quiltmc.org/v3/versions/loader/1.21.1/0.26.4/profile/json"
    );
}

#[test]
fn profile_url_unknown_kind_errors() {
    let result = profile_url("forge", "1.21.1", "54.0.0");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("unknown loader kind"));
}

#[test]
fn profile_url_fabric_differs_from_quilt() {
    let fabric = profile_url("fabric", "1.20.4", "0.15.11").unwrap();
    let quilt = profile_url("quilt", "1.20.4", "0.26.4").unwrap();
    assert_ne!(fabric, quilt, "fabric and quilt URLs must differ");
    assert!(fabric.contains("fabricmc.net"));
    assert!(quilt.contains("quiltmc.org"));
}

// --- maven_coord_to_path -------------------------------------------------

#[test]
fn maven_coord_three_segment() {
    assert_eq!(
        maven_coord_to_path("net.fabricmc:fabric-loader:0.16.10"),
        "net/fabricmc/fabric-loader/0.16.10/fabric-loader-0.16.10.jar"
    );
}

#[test]
fn maven_coord_four_segment_classifier() {
    assert_eq!(
        maven_coord_to_path("a.b:c:1.0:natives"),
        "a/b/c/1.0/c-1.0-natives.jar"
    );
}

#[test]
fn maven_coord_asm() {
    assert_eq!(
        maven_coord_to_path("org.ow2.asm:asm:9.7"),
        "org/ow2/asm/asm/9.7/asm-9.7.jar"
    );
}

#[test]
fn maven_coord_complex_version() {
    assert_eq!(
        maven_coord_to_path("net.fabricmc:sponge-mixin:0.15.4+mixin.0.8.7"),
        "net/fabricmc/sponge-mixin/0.15.4+mixin.0.8.7/sponge-mixin-0.15.4+mixin.0.8.7.jar"
    );
}

// --- NeoForge / Forge profile parsing -----------------------------------

const NEOFORGE_FIXTURE: &str = include_str!("fixtures/neoforge_profile.json");

#[test]
fn neoforge_fixture_parses_via_load_forge_profile() {
    // Confirms the fixture JSON parses correctly via `load_forge_profile`
    // (the forge-format path that reads downloads.artifact.url), not the
    // Fabric-style flat-url path.
    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    std::fs::write(tmp.path(), NEOFORGE_FIXTURE).expect("write fixture");
    let profile = load_forge_profile(tmp.path()).expect("neoforge fixture must parse");
    assert_eq!(profile.main_class, "net.neoforged.fml.startup.Client");
}

#[test]
fn neoforge_fixture_inherits_from() {
    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    std::fs::write(tmp.path(), NEOFORGE_FIXTURE).expect("write fixture");
    let profile = load_forge_profile(tmp.path()).expect("neoforge fixture must parse");
    assert_eq!(profile.inherits_from, Some("26.1.2".to_string()));
}

#[test]
fn neoforge_fixture_library_count() {
    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    std::fs::write(tmp.path(), NEOFORGE_FIXTURE).expect("write fixture");
    let profile = load_forge_profile(tmp.path()).expect("neoforge fixture must parse");
    // Fixture has 5 libraries: 4 with artifact url, 1 with empty downloads (no artifact).
    assert_eq!(profile.libraries.len(), 5);
}

#[test]
fn neoforge_fixture_libraries_with_url() {
    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    std::fs::write(tmp.path(), NEOFORGE_FIXTURE).expect("write fixture");
    let profile = load_forge_profile(tmp.path()).expect("neoforge fixture must parse");
    // First library has a url.
    assert_eq!(
        profile.libraries[0].name,
        "net.neoforged.fancymodloader:earlydisplay:11.0.13"
    );
    assert_eq!(
            profile.libraries[0].url,
            Some("https://maven.neoforged.net/releases/net/neoforged/fancymodloader/earlydisplay/11.0.13/earlydisplay-11.0.13.jar".to_string())
        );
}

#[test]
fn neoforge_fixture_processor_produced_lib_has_none_url() {
    // The last library in the fixture has `"downloads": {}` (no artifact) —
    // simulating a Forge processor-produced library with no download URL.
    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    std::fs::write(tmp.path(), NEOFORGE_FIXTURE).expect("write fixture");
    let profile = load_forge_profile(tmp.path()).expect("neoforge fixture must parse");
    let last = profile
        .libraries
        .last()
        .expect("fixture must have libraries");
    assert_eq!(last.name, "net.neoforged:client-extra:26.1.2");
    assert_eq!(last.url, None, "processor-produced lib must have url=None");
}

#[test]
fn neoforge_fixture_arguments_parsed() {
    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    std::fs::write(tmp.path(), NEOFORGE_FIXTURE).expect("write fixture");
    let profile = load_forge_profile(tmp.path()).expect("neoforge fixture must parse");
    // JVM args include ${library_directory} placeholder.
    let jvm_strings: Vec<&str> = profile
        .arguments
        .jvm
        .iter()
        .filter_map(|e| {
            if let ArgumentEntry::Plain(s) = e {
                Some(s.as_str())
            } else {
                None
            }
        })
        .collect();
    assert!(
        jvm_strings
            .iter()
            .any(|s| s.contains("${library_directory}")),
        "jvm args must contain library_directory placeholder: {:?}",
        jvm_strings
    );
    // Game args include --fml.neoForgeVersion.
    let game_strings: Vec<&str> = profile
        .arguments
        .game
        .iter()
        .filter_map(|e| {
            if let ArgumentEntry::Plain(s) = e {
                Some(s.as_str())
            } else {
                None
            }
        })
        .collect();
    assert!(
        game_strings.contains(&"--fml.neoForgeVersion"),
        "game args must contain --fml.neoForgeVersion: {:?}",
        game_strings
    );
}

#[test]
fn load_forge_profile_missing_file_errors() {
    let result = load_forge_profile(std::path::Path::new("/nonexistent/path/version.json"));
    assert!(result.is_err());
    let msg = result.unwrap_err();
    assert!(msg.contains("cannot read forge profile"), "error: {msg}");
}

#[test]
fn fabric_profile_inherits_from_parses_as_some() {
    // The fabric_profile.json fixture contains `"inheritsFrom": "1.21.1"`, so
    // LoaderProfile.inherits_from deserialises as Some("1.21.1").
    let json = include_str!("fixtures/fabric_profile.json");
    let profile: LoaderProfile = serde_json::from_str(json).expect("fabric profile must parse");
    assert_eq!(profile.inherits_from, Some("1.21.1".to_string()));
}
