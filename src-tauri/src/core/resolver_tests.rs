//! Unit tests for `resolver`. Extracted from the source module; wired back
//! via `#[cfg(test)] #[path = "resolver_tests.rs"] mod tests;` so private items
//! remain accessible through `super::*`.

use super::*;

const MODERN_FIXTURE: &str = include_str!("fixtures/version_manifest_modern.json");

#[test]
fn parse_modern_manifest_client_download() {
    let spec: VersionSpec =
        serde_json::from_str(MODERN_FIXTURE).expect("fixture must deserialize without error");

    assert_eq!(
            spec.downloads.client.url,
            "https://piston-data.mojang.com/v1/objects/aabbccddeeff00112233445566778899aabbccdd/client.jar"
        );
    assert_eq!(
        spec.downloads.client.sha1,
        "aabbccddeeff00112233445566778899aabbccdd"
    );
    assert_eq!(spec.downloads.client.size, 26234786);
}

#[test]
fn parse_modern_manifest_main_class() {
    let spec: VersionSpec =
        serde_json::from_str(MODERN_FIXTURE).expect("fixture must deserialize without error");

    assert_eq!(spec.main_class, "net.minecraft.client.main.Main");
}

#[test]
fn parse_modern_manifest_java_major() {
    let spec: VersionSpec =
        serde_json::from_str(MODERN_FIXTURE).expect("fixture must deserialize without error");

    assert_eq!(spec.java_major(), 21);
}

#[test]
fn parse_modern_manifest_asset_index() {
    let spec: VersionSpec =
        serde_json::from_str(MODERN_FIXTURE).expect("fixture must deserialize without error");

    assert_eq!(spec.asset_index.id, "17");
    assert_eq!(
        spec.asset_index.sha1,
        "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
    );
    assert_eq!(spec.asset_index.size, 447030);
    assert_eq!(spec.asset_index.total_size, 799786602);
}

#[test]
fn parse_modern_manifest_libraries() {
    let spec: VersionSpec =
        serde_json::from_str(MODERN_FIXTURE).expect("fixture must deserialize without error");

    assert_eq!(spec.libraries.len(), 3);
    let authlib = &spec.libraries[0];
    assert_eq!(authlib.name, "com.mojang:authlib:6.0.54");
    let artifact = authlib.downloads.artifact.as_ref().unwrap();
    assert_eq!(artifact.size, 112233);
}

#[test]
fn parse_modern_manifest_structured_arguments() {
    let spec: VersionSpec =
        serde_json::from_str(MODERN_FIXTURE).expect("fixture must deserialize without error");

    // Modern manifest: `arguments` present, `minecraftArguments` absent.
    assert!(spec.arguments.is_some());
    assert!(spec.minecraft_arguments.is_none());

    let args = spec.arguments.unwrap();
    // Game args include plain string entries.
    assert!(!args.game.is_empty());
    // JVM args include both plain strings and conditional (rules-based) entries.
    assert!(!args.jvm.is_empty());
}

#[test]
fn java_major_defaults_to_8_when_absent() {
    // Minimal manifest with no javaVersion field.
    let json = r#"{
            "id": "1.8.9",
            "mainClass": "net.minecraft.client.main.Main",
            "downloads": {
                "client": {
                    "url": "https://example.com/client.jar",
                    "sha1": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "size": 1000000
                }
            },
            "assetIndex": {
                "id": "1.8",
                "sha1": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "size": 100000,
                "totalSize": 200000000,
                "url": "https://example.com/1.8.json"
            },
            "libraries": []
        }"#;

    let spec: VersionSpec = serde_json::from_str(json).expect("minimal manifest must deserialize");
    assert_eq!(spec.java_major(), 8);
}

// -----------------------------------------------------------------------
// CP2: artifact path field
// -----------------------------------------------------------------------

#[test]
fn artifact_path_field_present_in_modern_fixture() {
    let spec: VersionSpec = serde_json::from_str(MODERN_FIXTURE).expect("fixture must deserialize");
    let artifact = spec.libraries[0].downloads.artifact.as_ref().unwrap();
    assert_eq!(
        artifact.path.as_deref(),
        Some("com/mojang/authlib/6.0.54/authlib-6.0.54.jar")
    );
}

// -----------------------------------------------------------------------
// CP2: rule evaluation
// -----------------------------------------------------------------------

fn make_rule(action: &str, os_name: Option<&str>) -> serde_json::Value {
    match os_name {
        None => serde_json::json!({ "action": action }),
        Some(name) => serde_json::json!({ "action": action, "os": { "name": name } }),
    }
}

#[test]
fn eval_rules_no_rules_allows_all_os() {
    // Empty rules → allowed on every OS.
    assert!(eval_rules(&[], "linux"));
    assert!(eval_rules(&[], "windows"));
    assert!(eval_rules(&[], "osx"));
}

#[test]
fn eval_rules_allow_all_no_os() {
    // Single allow rule with no OS constraint → matches everything.
    let rules = vec![make_rule("allow", None)];
    assert!(eval_rules(&rules, "linux"));
    assert!(eval_rules(&rules, "windows"));
    assert!(eval_rules(&rules, "osx"));
}

#[test]
fn eval_rules_allow_specific_os() {
    // allow osx only.
    let rules = vec![make_rule("allow", Some("osx"))];
    assert!(!eval_rules(&rules, "linux"));
    assert!(!eval_rules(&rules, "windows"));
    assert!(eval_rules(&rules, "osx"));
}

#[test]
fn eval_rules_disallow_specific_os() {
    // allow all, then disallow windows.
    let rules = vec![
        make_rule("allow", None),
        make_rule("disallow", Some("windows")),
    ];
    assert!(eval_rules(&rules, "linux"));
    assert!(!eval_rules(&rules, "windows"));
    assert!(eval_rules(&rules, "osx"));
}

#[test]
fn eval_rules_last_matching_rule_wins() {
    // disallow windows, then allow windows → allowed (later rule wins).
    let rules = vec![
        make_rule("disallow", Some("windows")),
        make_rule("allow", Some("windows")),
    ];
    assert!(eval_rules(&rules, "windows"));
    // linux: only the first rule has os=windows (no match), second has os=windows
    // (no match) → default disallow (rules present, none matched linux).
    assert!(!eval_rules(&rules, "linux"));
}

// -----------------------------------------------------------------------
// CP2: classpath selection (legacy fixture)
// -----------------------------------------------------------------------

const LEGACY_FIXTURE: &str = include_str!("fixtures/version_manifest_legacy.json");

#[test]
fn parse_legacy_manifest_uses_minecraft_arguments() {
    let spec: VersionSpec =
        serde_json::from_str(LEGACY_FIXTURE).expect("legacy fixture must deserialize");
    assert!(spec.minecraft_arguments.is_some());
    assert!(spec.arguments.is_none());
    assert!(spec
        .minecraft_arguments
        .unwrap()
        .contains("${auth_player_name}"));
}

#[test]
fn classpath_linux_excludes_osx_and_windows_only_libs() {
    let spec: VersionSpec =
        serde_json::from_str(LEGACY_FIXTURE).expect("legacy fixture must deserialize");
    let cp = select_classpath(&spec.libraries, "linux");

    // authlib (no rules), lwjgl (allow all), lwjgl_util (allow all) → 3 entries.
    // java-objc-bridge (allow osx only) → excluded.
    // icu4j-core-mojang (allow windows only) → excluded.
    assert_eq!(cp.len(), 3);
    let paths: Vec<&str> = cp.iter().map(|e| e.maven_path.as_str()).collect();
    assert!(paths.contains(&"com/mojang/authlib/1.5.25/authlib-1.5.25.jar"));
    assert!(paths.contains(
        &"org/lwjgl/lwjgl/lwjgl/2.9.4-nightly-20150209/lwjgl-2.9.4-nightly-20150209.jar"
    ));
    assert!(paths.contains(
        &"org/lwjgl/lwjgl/lwjgl_util/2.9.4-nightly-20150209/lwjgl_util-2.9.4-nightly-20150209.jar"
    ));
}

#[test]
fn classpath_windows_excludes_osx_only_lib_includes_windows_only() {
    let spec: VersionSpec =
        serde_json::from_str(LEGACY_FIXTURE).expect("legacy fixture must deserialize");
    let cp = select_classpath(&spec.libraries, "windows");

    // authlib + lwjgl + lwjgl_util + icu4j (windows-only) = 4.
    // java-objc-bridge (osx-only) excluded.
    assert_eq!(cp.len(), 4);
    let paths: Vec<&str> = cp.iter().map(|e| e.maven_path.as_str()).collect();
    assert!(paths.contains(&"com/ibm/icu/icu4j-core-mojang/51.2/icu4j-core-mojang-51.2.jar"));
    assert!(!paths.contains(&"ca/weblite/java-objc-bridge/1.1/java-objc-bridge-1.1.jar"));
}

#[test]
fn classpath_osx_excludes_windows_only_lib_includes_osx_only() {
    let spec: VersionSpec =
        serde_json::from_str(LEGACY_FIXTURE).expect("legacy fixture must deserialize");
    let cp = select_classpath(&spec.libraries, "osx");

    // authlib + lwjgl + lwjgl_util + java-objc-bridge (osx-only) = 4.
    // icu4j (windows-only) excluded.
    assert_eq!(cp.len(), 4);
    let paths: Vec<&str> = cp.iter().map(|e| e.maven_path.as_str()).collect();
    assert!(paths.contains(&"ca/weblite/java-objc-bridge/1.1/java-objc-bridge-1.1.jar"));
    assert!(!paths.contains(&"com/ibm/icu/icu4j-core-mojang/51.2/icu4j-core-mojang-51.2.jar"));
}

// -----------------------------------------------------------------------
// CP2: natives selection
// -----------------------------------------------------------------------

#[test]
fn natives_linux_resolves_correct_classifier() {
    let spec: VersionSpec =
        serde_json::from_str(LEGACY_FIXTURE).expect("legacy fixture must deserialize");
    let natives = select_natives(&spec.libraries, "linux");

    assert_eq!(natives.len(), 1);
    assert_eq!(natives[0].classifier, "natives-linux");
    assert_eq!(
            natives[0].maven_path,
            "org/lwjgl/lwjgl/lwjgl/2.9.4-nightly-20150209/lwjgl-2.9.4-nightly-20150209-natives-linux.jar"
        );
    assert_eq!(natives[0].sha1, "cccc0003cccc0003cccc0003cccc0003cccc0003");
}

#[test]
fn natives_windows_resolves_correct_classifier() {
    let spec: VersionSpec =
        serde_json::from_str(LEGACY_FIXTURE).expect("legacy fixture must deserialize");
    let natives = select_natives(&spec.libraries, "windows");

    assert_eq!(natives.len(), 1);
    assert_eq!(natives[0].classifier, "natives-windows");
    assert_eq!(
            natives[0].maven_path,
            "org/lwjgl/lwjgl/lwjgl/2.9.4-nightly-20150209/lwjgl-2.9.4-nightly-20150209-natives-windows.jar"
        );
}

#[test]
fn natives_osx_resolves_correct_classifier() {
    let spec: VersionSpec =
        serde_json::from_str(LEGACY_FIXTURE).expect("legacy fixture must deserialize");
    let natives = select_natives(&spec.libraries, "osx");

    assert_eq!(natives.len(), 1);
    assert_eq!(natives[0].classifier, "natives-osx");
    assert_eq!(
        natives[0].maven_path,
        "org/lwjgl/lwjgl/lwjgl/2.9.4-nightly-20150209/lwjgl-2.9.4-nightly-20150209-natives-osx.jar"
    );
}

#[test]
fn natives_arch_token_substituted() {
    // A library whose classifier template contains ${arch}.
    let json = serde_json::json!({
        "name": "org.lwjgl:lwjgl:3.x",
        "downloads": {
            "classifiers": {
                "natives-linux-64": {
                    "path": "org/lwjgl/lwjgl/3.x/lwjgl-3.x-natives-linux-64.jar",
                    "sha1": "aaaa1234aaaa1234aaaa1234aaaa1234aaaa1234",
                    "size": 100000,
                    "url": "https://example.com/lwjgl-3.x-natives-linux-64.jar"
                }
            }
        },
        "natives": {
            "linux": "natives-linux-${arch}"
        }
    });
    let lib: Library = serde_json::from_value(json).unwrap();
    let natives = select_natives(&[lib], "linux");
    assert_eq!(natives.len(), 1);
    assert_eq!(natives[0].classifier, "natives-linux-64");
}

#[test]
fn maven_dest_path_prefix() {
    // Verify classpath entries report the correct maven_path for dest computation.
    let spec: VersionSpec =
        serde_json::from_str(LEGACY_FIXTURE).expect("legacy fixture must deserialize");
    let cp = select_classpath(&spec.libraries, "linux");
    // Every entry's dest would be: libraries/<maven_path>
    for entry in &cp {
        assert!(
            !entry.maven_path.starts_with('/'),
            "maven_path must be relative: {}",
            entry.maven_path
        );
        assert!(
            entry.maven_path.ends_with(".jar"),
            "maven_path must end with .jar: {}",
            entry.maven_path
        );
    }
}

// -----------------------------------------------------------------------
// CP3: asset index resolution
// -----------------------------------------------------------------------

const ASSET_INDEX_FIXTURE: &str = include_str!("fixtures/asset_index_sample.json");

#[test]
fn parse_asset_index_object_count() {
    let data: AssetIndexData =
        serde_json::from_str(ASSET_INDEX_FIXTURE).expect("asset index fixture must deserialize");
    assert_eq!(data.objects.len(), 4);
}

#[test]
fn asset_objects_to_items_url_and_dest() {
    let data: AssetIndexData =
        serde_json::from_str(ASSET_INDEX_FIXTURE).expect("asset index fixture must deserialize");
    let base = std::path::Path::new("/data");
    let items = asset_objects_to_items(&data.objects, base);

    // Should produce one item per object.
    assert_eq!(items.len(), 4);

    // Find the specific object we know the hash for.
    let known_hash = "bdf48ef6b5d0d23bbb02e17d04865216179f510a";
    let item = items
        .iter()
        .find(|i| i.url.ends_with(known_hash))
        .expect("item for known hash must be present");

    assert_eq!(
        item.url,
        format!("https://resources.download.minecraft.net/bd/{}", known_hash)
    );
    assert_eq!(
        item.dest,
        std::path::PathBuf::from(format!("/data/assets/objects/bd/{}", known_hash))
    );
    assert_eq!(
        item.expected_hash,
        Some(crate::core::download::ExpectedHash::Sha1(
            known_hash.to_string()
        ))
    );
    assert_eq!(item.size, Some(3665));
}

#[test]
fn asset_objects_to_items_two_hex_prefix() {
    // Verify prefix is always the first two chars of the hash.
    let data: AssetIndexData =
        serde_json::from_str(ASSET_INDEX_FIXTURE).expect("asset index fixture must deserialize");
    let base = std::path::Path::new("/tmp/mc");
    let items = asset_objects_to_items(&data.objects, base);

    for item in &items {
        // URL format: …/<2hex>/<sha1>
        let url_parts: Vec<&str> = item.url.rsplitn(3, '/').collect();
        let sha1 = url_parts[0];
        let prefix = url_parts[1];
        assert_eq!(
            prefix,
            &sha1[..2],
            "URL prefix must be first 2 chars of hash"
        );

        // dest last two path components must be <2hex> then <sha1>.
        // Use Path components to avoid OS path-separator differences.
        let components: Vec<_> = item.dest.components().collect();
        let n = components.len();
        assert!(
            n >= 2,
            "dest must have at least 2 components: {:?}",
            item.dest
        );
        let last = components[n - 1].as_os_str().to_str().unwrap();
        let second_last = components[n - 2].as_os_str().to_str().unwrap();
        assert_eq!(last, sha1, "last dest component must be the sha1 hash");
        assert_eq!(
            second_last,
            &sha1[..2],
            "second-last dest component must be the 2-char prefix"
        );
    }
}

#[test]
fn asset_index_file_item_correct() {
    // Build a synthetic AssetIndex descriptor (like the one inside VersionSpec).
    let ai = AssetIndex {
        id: "17".to_string(),
        sha1: "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef".to_string(),
        size: 447030,
        total_size: 799786602,
        url: "https://piston-meta.mojang.com/v1/packages/deadbeef.../17.json".to_string(),
    };
    let base = std::path::Path::new("/data");
    let item = asset_index_file_item(&ai, base);

    assert_eq!(item.url, ai.url);
    assert_eq!(
        item.dest,
        std::path::PathBuf::from("/data/assets/indexes/17.json")
    );
    assert_eq!(
        item.expected_hash,
        Some(crate::core::download::ExpectedHash::Sha1(
            "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef".to_string()
        ))
    );
    assert_eq!(item.size, Some(447030));
}

#[test]
fn assets_legacy_false_for_modern_index() {
    // Modern index: no `virtual`, no `map_to_resources` → legacy = false.
    let data: AssetIndexData =
        serde_json::from_str(ASSET_INDEX_FIXTURE).expect("asset index fixture must deserialize");
    assert!(!data.assets_legacy());
}

#[test]
fn assets_legacy_true_when_virtual_set() {
    let json = r#"{"objects": {}, "virtual": true}"#;
    let data: AssetIndexData = serde_json::from_str(json).expect("must deserialize");
    assert!(data.assets_legacy());
}

#[test]
fn assets_legacy_true_when_map_to_resources_set() {
    let json = r#"{"objects": {}, "mapToResources": true}"#;
    let data: AssetIndexData = serde_json::from_str(json).expect("must deserialize");
    assert!(data.assets_legacy());
}

#[test]
fn asset_index_file_item_from_modern_fixture() {
    // Confirm the AssetIndex from the already-parsed modern version fixture
    // produces the correct index-file DownloadItem.
    let spec: VersionSpec =
        serde_json::from_str(MODERN_FIXTURE).expect("modern fixture must deserialize");
    let base = std::path::Path::new("/data");
    let item = asset_index_file_item(&spec.asset_index, base);

    assert_eq!(
        item.dest,
        std::path::PathBuf::from("/data/assets/indexes/17.json")
    );
    assert_eq!(
        item.expected_hash,
        Some(crate::core::download::ExpectedHash::Sha1(
            "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef".to_string()
        ))
    );
    assert_eq!(item.size, Some(447030));
}

// -----------------------------------------------------------------------
// CP4: end-to-end assemble test
// -----------------------------------------------------------------------

#[test]
fn assemble_modern_fixture_plan_item_count_and_launch_meta() {
    // Inputs: modern version manifest + sample asset index.
    let spec: VersionSpec =
        serde_json::from_str(MODERN_FIXTURE).expect("modern fixture must deserialize");
    let asset_data: AssetIndexData =
        serde_json::from_str(ASSET_INDEX_FIXTURE).expect("asset index fixture must deserialize");
    let base = std::path::Path::new("/data");

    let (plan, launch) = assemble(&spec, &asset_data, "linux", base);

    // Modern fixture libraries (linux):
    //   select_classpath("linux") → 3 (authlib, lwjgl, slf4j — all have artifacts, no rules)
    //   select_natives("linux") → 1 (lwjgl natives-linux)
    // Asset items: 4 objects + 1 index file
    // Client jar: 1
    // Total: 3 + 1 + 4 + 1 + 1 = 10
    assert_eq!(plan.items.len(), 10, "expected 10 plan items");

    // LaunchMeta fields.
    assert_eq!(launch.main_class, "net.minecraft.client.main.Main");
    assert_eq!(launch.java_major, 21);
    assert_eq!(launch.asset_index_id, "17");
    assert!(!launch.assets_legacy);
    assert_eq!(launch.version_id, "1.21.1");
    assert_eq!(launch.version_type, "release");

    // Classpath contains client jar path.
    let client_jar = base
        .join("versions")
        .join("1.21.1")
        .join("1.21.1.jar")
        .to_string_lossy()
        .into_owned();
    assert!(
        launch.classpath.contains(&client_jar),
        "classpath must contain client jar: {:?}",
        launch.classpath
    );

    // Natives non-empty for linux.
    assert!(
        !launch.natives.is_empty(),
        "linux natives must be non-empty"
    );
    assert!(
        launch.natives[0].contains("natives-linux"),
        "native path must reference natives-linux: {}",
        launch.natives[0]
    );

    // Modern args: game_args include ${auth_player_name}, jvm_args include -cp.
    assert!(
        launch
            .game_args
            .iter()
            .any(|a| a.contains("${auth_player_name}")),
        "game_args must contain auth_player_name template"
    );
    assert!(
        launch.jvm_args.iter().any(|a| a == "-cp"),
        "jvm_args must contain -cp"
    );

    // No logging config (not in modern fixture).
    assert!(launch.logging_config.is_none());
}

// -----------------------------------------------------------------------
// F-1: asset_objects_to_items skips objects with short/invalid hash
// -----------------------------------------------------------------------

#[test]
fn asset_objects_to_items_skips_too_short_hash() {
    // One valid object (hash len >= 2) and one invalid (hash = "a", len 1).
    // The function must not panic and must exclude the invalid object.
    let mut objects = std::collections::HashMap::new();
    objects.insert(
        "valid-object".to_string(),
        AssetObject {
            hash: "bdf48ef6b5d0d23bbb02e17d04865216179f510a".to_string(),
            size: 1234,
        },
    );
    objects.insert(
        "bad-object".to_string(),
        AssetObject {
            hash: "a".to_string(), // too short — would panic on [..2]
            size: 99,
        },
    );

    let base = std::path::Path::new("/data");
    let items = asset_objects_to_items(&objects, base);

    // Only the valid object should produce an item; the short-hash one is skipped.
    assert_eq!(
        items.len(),
        1,
        "expected 1 item (short-hash object skipped)"
    );
    assert!(
        items[0]
            .url
            .ends_with("bdf48ef6b5d0d23bbb02e17d04865216179f510a"),
        "surviving item must be the valid-hash object"
    );
}

#[test]
fn assemble_legacy_fixture_game_args_from_minecraft_arguments() {
    // Legacy manifest: minecraftArguments whitespace-split into game_args; jvm_args empty.
    let spec: VersionSpec =
        serde_json::from_str(LEGACY_FIXTURE).expect("legacy fixture must deserialize");
    // Minimal asset index (empty objects — we just need it to not panic).
    let asset_data: AssetIndexData =
        serde_json::from_str(r#"{"objects":{}}"#).expect("empty asset index must deserialize");
    let base = std::path::Path::new("/data");

    let (_plan, launch) = assemble(&spec, &asset_data, "linux", base);

    // Legacy: game_args come from minecraftArguments whitespace split.
    assert!(!launch.game_args.is_empty());
    assert!(
        launch
            .game_args
            .iter()
            .any(|a| a.contains("${auth_player_name}")),
        "legacy game_args must contain auth_player_name"
    );
    // jvm_args left empty for legacy (slice D provides defaults).
    assert!(launch.jvm_args.is_empty(), "legacy jvm_args must be empty");
}

// -----------------------------------------------------------------------
// CP2 (fabric-quilt-launch): merge_loader_profile
// -----------------------------------------------------------------------

/// Build a minimal `(DownloadPlan, LaunchMeta)` for testing merge.
fn make_test_resolve_result(
    data_dir: &std::path::Path,
) -> (crate::core::download::DownloadPlan, LaunchMeta) {
    use crate::core::download::{DownloadItem, DownloadPlan, ExpectedHash};

    let client_jar = data_dir.join("versions").join("1.21.1").join("1.21.1.jar");

    // One vanilla library + client jar last (mirrors assemble contract).
    let vanilla_lib = data_dir
        .join("libraries")
        .join("com/mojang/authlib/6.0.54/authlib-6.0.54.jar");
    let plan = DownloadPlan::new(vec![
        DownloadItem {
            url: "https://example.com/authlib.jar".to_string(),
            dest: vanilla_lib.clone(),
            expected_hash: Some(ExpectedHash::Sha1("aabbccdd".to_string())),
            size: Some(12345),
        },
        DownloadItem {
            url: "https://example.com/client.jar".to_string(),
            dest: client_jar.clone(),
            expected_hash: Some(ExpectedHash::Sha1("deadbeef".to_string())),
            size: Some(26000000),
        },
    ]);

    let launch = LaunchMeta {
        version_id: "1.21.1".to_string(),
        version_type: "release".to_string(),
        main_class: "net.minecraft.client.main.Main".to_string(),
        jvm_args: vec!["-cp".to_string(), "${classpath}".to_string()],
        game_args: vec!["--version".to_string(), "${version_name}".to_string()],
        asset_index_id: "17".to_string(),
        assets_legacy: false,
        java_major: 21,
        classpath: vec![
            vanilla_lib.to_string_lossy().into_owned(),
            client_jar.to_string_lossy().into_owned(), // client jar LAST
        ],
        natives: vec![],
        logging_config: None,
    };

    (plan, launch)
}

/// Build a `LoaderProfile` matching the fabric fixture (from CP1).
fn fabric_profile() -> crate::core::loader_profile::LoaderProfile {
    let json = include_str!("fixtures/fabric_profile.json");
    serde_json::from_str(json).expect("fabric profile fixture must parse")
}

#[test]
fn merge_loader_profile_overrides_main_class() {
    let base = std::path::Path::new("/data");
    let (mut plan, mut launch) = make_test_resolve_result(base);
    let profile = fabric_profile();

    merge_loader_profile(&mut plan, &mut launch, &profile, "linux", base);

    assert_eq!(
        launch.main_class, "net.fabricmc.loader.impl.launch.knot.KnotClient",
        "main_class must be overridden to the loader's mainClass"
    );
}

#[test]
fn merge_loader_profile_client_jar_still_last() {
    let base = std::path::Path::new("/data");
    let (mut plan, mut launch) = make_test_resolve_result(base);
    let profile = fabric_profile();
    let client_jar = base
        .join("versions")
        .join("1.21.1")
        .join("1.21.1.jar")
        .to_string_lossy()
        .into_owned();

    merge_loader_profile(&mut plan, &mut launch, &profile, "linux", base);

    assert_eq!(
        launch.classpath.last().unwrap(),
        &client_jar,
        "client jar must remain the last classpath entry after merge"
    );
}

#[test]
fn merge_loader_profile_loader_libs_on_classpath() {
    let base = std::path::Path::new("/data");
    let (mut plan, mut launch) = make_test_resolve_result(base);
    let profile = fabric_profile();
    let original_cp_len = launch.classpath.len();

    merge_loader_profile(&mut plan, &mut launch, &profile, "linux", base);

    // Classpath must grow by the number of loader libraries.
    assert_eq!(
        launch.classpath.len(),
        original_cp_len + profile.libraries.len(),
        "classpath must contain all loader libs plus original entries"
    );

    // Each loader lib's dest must appear on the classpath.
    for lib in &profile.libraries {
        let maven_path = crate::core::loader_profile::maven_coord_to_path(&lib.name);
        let expected_dest = base
            .join("libraries")
            .join(&maven_path)
            .to_string_lossy()
            .into_owned();
        assert!(
            launch.classpath.contains(&expected_dest),
            "loader lib {} must be on classpath; got: {:?}",
            lib.name,
            launch.classpath
        );
    }
}

#[test]
fn merge_loader_profile_plan_gains_loader_items_with_no_hash() {
    let base = std::path::Path::new("/data");
    let (mut plan, mut launch) = make_test_resolve_result(base);
    let profile = fabric_profile();
    let original_item_count = plan.items.len();

    merge_loader_profile(&mut plan, &mut launch, &profile, "linux", base);

    assert_eq!(
        plan.items.len(),
        original_item_count + profile.libraries.len(),
        "plan must gain one item per loader library"
    );

    // All newly added items must have expected_hash == None.
    let new_items = &plan.items[original_item_count..];
    for item in new_items {
        assert!(
            item.expected_hash.is_none(),
            "loader lib items must have expected_hash None; got: {:?}",
            item.expected_hash
        );
    }
}

#[test]
fn merge_loader_profile_url_join_trailing_slash() {
    // Fabric uses "https://maven.fabricmc.net/" (trailing slash).
    // Joining with the maven path must NOT produce "//" or lose the separator.
    let base = std::path::Path::new("/data");
    let (mut plan, mut launch) = make_test_resolve_result(base);
    let profile = fabric_profile();

    merge_loader_profile(&mut plan, &mut launch, &profile, "linux", base);

    // The first library is net.fabricmc:fabric-loader:0.16.10 at https://maven.fabricmc.net/
    // Expected URL: https://maven.fabricmc.net/net/fabricmc/fabric-loader/0.16.10/fabric-loader-0.16.10.jar
    let first_new_item = &plan.items[2]; // 2 original items (vanilla lib + client jar)
    assert!(
        !first_new_item.url.contains("//net/"),
        "URL must not contain double slash before maven path: {}",
        first_new_item.url
    );
    assert!(
        first_new_item
            .url
            .contains("https://maven.fabricmc.net/net/fabricmc/fabric-loader"),
        "URL must join base and maven path correctly: {}",
        first_new_item.url
    );
}

#[test]
fn merge_loader_profile_jvm_args_appended_after_vanilla() {
    let base = std::path::Path::new("/data");
    let (mut plan, mut launch) = make_test_resolve_result(base);
    let original_jvm_args = launch.jvm_args.clone();
    let profile = fabric_profile();

    merge_loader_profile(&mut plan, &mut launch, &profile, "linux", base);

    // Original vanilla args must still be at the start.
    assert!(
        launch.jvm_args.starts_with(&original_jvm_args),
        "vanilla jvm_args must remain at the front"
    );

    // Fabric fixture has one jvm arg: "-DFabricMcEmu=net.minecraft.client.main.Main"
    assert!(
        launch
            .jvm_args
            .contains(&"-DFabricMcEmu=net.minecraft.client.main.Main".to_string()),
        "loader jvm arg must be appended: {:?}",
        launch.jvm_args
    );
}

#[test]
fn merge_loader_profile_game_args_appended_after_vanilla() {
    let base = std::path::Path::new("/data");
    let (mut plan, mut launch) = make_test_resolve_result(base);
    let original_game_args = launch.game_args.clone();
    let profile = fabric_profile();

    merge_loader_profile(&mut plan, &mut launch, &profile, "linux", base);

    // Vanilla args remain at front.
    assert!(
        launch.game_args.starts_with(&original_game_args),
        "vanilla game_args must remain at the front"
    );

    // Fabric fixture has zero game args — length should be unchanged.
    assert_eq!(
        launch.game_args.len(),
        original_game_args.len(),
        "no loader game args to append (fixture has empty game args)"
    );
}

#[test]
fn merge_loader_profile_classpath_order() {
    // Asserts exact classpath order: [vanilla libs..., loader libs in PROFILE order..., client jar].
    // This test would fail against the original reversed-insertion implementation.
    let base = std::path::Path::new("/data");
    let (mut plan, mut launch) = make_test_resolve_result(base);
    let profile = fabric_profile();

    // Record the vanilla lib (first entry) and client jar (last entry) before merge.
    let vanilla_lib = launch.classpath[0].clone();
    let client_jar = launch.classpath.last().unwrap().clone();

    merge_loader_profile(&mut plan, &mut launch, &profile, "linux", base);

    // Build expected loader dest paths in profile order.
    let expected_loader_paths: Vec<String> = profile
        .libraries
        .iter()
        .map(|lib| {
            let maven_path = crate::core::loader_profile::maven_coord_to_path(&lib.name);
            base.join("libraries")
                .join(&maven_path)
                .to_string_lossy()
                .into_owned()
        })
        .collect();

    // Expected full order: vanilla_lib, loader_lib0, loader_lib1, …, client_jar.
    let mut expected = vec![vanilla_lib];
    expected.extend(expected_loader_paths);
    expected.push(client_jar);

    assert_eq!(
        launch.classpath, expected,
        "classpath must be [vanilla libs, loader libs in profile order, client jar]"
    );
}

// -----------------------------------------------------------------------
// CP2 (neoforge-forge-launch): merge with forge profile fixtures
// -----------------------------------------------------------------------

#[test]
fn merge_neoforge_profile_none_url_lib_on_classpath_no_download_item() {
    // Load the neoforge fixture via load_forge_profile (the forge-format parse path).
    // The fixture has 5 libs: 4 with url, 1 with url=None (processor-produced).
    // After merge: plan gains 4 items (not 5); classpath gains 5 entries (all 5 libs).
    use crate::core::loader_profile::load_forge_profile;

    let neoforge_json = include_str!("fixtures/neoforge_profile.json");
    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    std::fs::write(tmp.path(), neoforge_json).expect("write fixture");
    let profile = load_forge_profile(tmp.path()).expect("neoforge fixture must parse");

    let base = std::path::Path::new("/data");
    let (mut plan, mut launch) = make_test_resolve_result(base);
    let original_plan_len = plan.items.len();
    let original_cp_len = launch.classpath.len();

    merge_loader_profile(&mut plan, &mut launch, &profile, "linux", base);

    // 4 libs with url → 4 DownloadItems added.
    assert_eq!(
        plan.items.len(),
        original_plan_len + 4,
        "4 url libs → 4 DownloadItems; got plan len {}",
        plan.items.len()
    );
    // All 5 libs go on classpath (including the url=None one).
    assert_eq!(
        launch.classpath.len(),
        original_cp_len + 5,
        "all 5 libs → 5 classpath entries"
    );
    // url=None lib (client-extra) IS on classpath.
    let client_extra_path = base
        .join("libraries")
        .join("net/neoforged/client-extra/26.1.2/client-extra-26.1.2.jar")
        .to_string_lossy()
        .into_owned();
    assert!(
        launch.classpath.contains(&client_extra_path),
        "url=None lib must be on classpath: {:?}",
        launch.classpath
    );
    // client jar still last.
    let client_jar = base
        .join("versions")
        .join("1.21.1")
        .join("1.21.1.jar")
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        launch.classpath.last().unwrap(),
        &client_jar,
        "client jar must remain last"
    );

    // The 4 DownloadItems must use the FULL artifact URLs from the fixture —
    // not base-url + double maven path.
    let item_urls: Vec<&str> = plan.items[original_plan_len..]
        .iter()
        .map(|i| i.url.as_str())
        .collect();
    assert!(
            item_urls.contains(&"https://maven.neoforged.net/releases/net/neoforged/fancymodloader/earlydisplay/11.0.13/earlydisplay-11.0.13.jar"),
            "earlydisplay URL must be the full artifact URL; got: {:?}",
            item_urls
        );
    assert!(
            item_urls.contains(&"https://maven.neoforged.net/releases/net/neoforged/fancymodloader/loader/11.0.13/loader-11.0.13.jar"),
            "loader URL must be the full artifact URL; got: {:?}",
            item_urls
        );
    assert!(
            item_urls.contains(&"https://maven.neoforged.net/releases/net/neoforged/accesstransformers/11.0.2/accesstransformers-11.0.2.jar"),
            "accesstransformers URL must be the full artifact URL; got: {:?}",
            item_urls
        );
    assert!(
        item_urls
            .contains(&"https://maven.neoforged.net/releases/org/ow2/asm/asm/9.9.1/asm-9.9.1.jar"),
        "asm URL must be the full artifact URL; got: {:?}",
        item_urls
    );
}

#[test]
fn merge_loader_profile_none_url_lib_classpath_only() {
    // A loader library with url=None (processor-produced, no download URL) must
    // be added to the classpath but NOT produce a DownloadItem.  The old empty-string
    // guard is superseded: url=None and url=Some("") both skip the download step
    // but still add the classpath entry so the JVM can find the locally-produced jar.
    use crate::core::loader_profile::{LoaderLibrary, LoaderProfile};
    use crate::core::resolver::Arguments;

    let base = std::path::Path::new("/data");
    let (mut plan, mut launch) = make_test_resolve_result(base);

    let profile = LoaderProfile {
        main_class: "net.fabricmc.loader.impl.launch.knot.KnotClient".to_string(),
        inherits_from: None,
        libraries: vec![
            LoaderLibrary {
                name: "net.fabricmc:fabric-loader:0.16.10".to_string(),
                url: Some("https://maven.fabricmc.net/".to_string()),
            },
            LoaderLibrary {
                // url=None (processor-produced) — classpath-only, no DownloadItem.
                name: "org.example:no-url-lib:1.0".to_string(),
                url: None,
            },
            LoaderLibrary {
                name: "net.fabricmc:intermediary:1.21.1".to_string(),
                url: Some("https://maven.fabricmc.net/".to_string()),
            },
        ],
        arguments: Arguments {
            jvm: vec![],
            game: vec![],
        },
    };

    let original_item_count = plan.items.len();

    merge_loader_profile(&mut plan, &mut launch, &profile, "linux", base);

    // Only 2 of 3 libs produce DownloadItems (the url=None one is download-skipped).
    assert_eq!(
        plan.items.len(),
        original_item_count + 2,
        "url=None library must not produce a DownloadItem; plan should gain 2 items"
    );

    // No plan item URL should start with "/" (malformed).
    for item in &plan.items {
        assert!(
            !item.url.starts_with('/'),
            "plan must not contain a malformed (leading-slash) URL: {}",
            item.url
        );
    }

    // The url=None lib IS on the classpath (classpath-only entry).
    let no_url_path = base
        .join("libraries")
        .join("org/example/no-url-lib/1.0/no-url-lib-1.0.jar")
        .to_string_lossy()
        .into_owned();
    assert!(
        launch.classpath.contains(&no_url_path),
        "url=None library must appear on classpath (classpath-only entry): {:?}",
        launch.classpath
    );
}
