//! Unit tests for `modpack`. Extracted from the source module; wired back
//! via `#[cfg(test)] #[path = "modpack_tests.rs"] mod tests;` so private items
//! remain accessible through `super::*`.

use super::*;
use std::io::Write as IoWrite;
use tempfile::TempDir;
use zip::write::SimpleFileOptions;

// ── CP1: parse_modrinth_index ─────────────────────────────────────────────

#[test]
fn cp1_parse_fabric_manifest() {
    let json = include_str!("fixtures/mrpack_fabric.json");
    let manifest = parse_modrinth_index(json).expect("should parse");
    assert_eq!(manifest.name, "Test Fabric Pack");
    assert_eq!(manifest.version_id, "1.0.0");
    assert_eq!(manifest.minecraft, "1.21.1");
    assert_eq!(manifest.loader.kind, "fabric");
    assert_eq!(manifest.loader.version.as_deref(), Some("0.16.0"));
    assert_eq!(manifest.files.len(), 3);
}

#[test]
fn cp1_loader_fabric_key_maps_correctly() {
    let json = include_str!("fixtures/mrpack_fabric.json");
    let manifest = parse_modrinth_index(json).unwrap();
    assert_eq!(manifest.loader.kind, "fabric");
    assert_eq!(manifest.loader.version.as_deref(), Some("0.16.0"));
}

#[test]
fn cp1_loader_quilt_key_maps_correctly() {
    let json = include_str!("fixtures/mrpack_quilt.json");
    let manifest = parse_modrinth_index(json).unwrap();
    assert_eq!(manifest.loader.kind, "quilt");
    assert_eq!(manifest.loader.version.as_deref(), Some("0.27.1"));
}

#[test]
fn cp1_loader_forge_key_maps_correctly() {
    let json = r#"{
            "formatVersion": 1,
            "game": "minecraft",
            "name": "Forge Pack",
            "versionId": "1.0",
            "dependencies": { "minecraft": "1.20.1", "forge": "47.2.0" },
            "files": []
        }"#;
    let manifest = parse_modrinth_index(json).unwrap();
    assert_eq!(manifest.loader.kind, "forge");
    assert_eq!(manifest.loader.version.as_deref(), Some("47.2.0"));
}

#[test]
fn cp1_loader_neoforge_key_maps_correctly() {
    let json = r#"{
            "formatVersion": 1,
            "game": "minecraft",
            "name": "NeoForge Pack",
            "versionId": "1.0",
            "dependencies": { "minecraft": "1.21.1", "neoforge": "21.1.0" },
            "files": []
        }"#;
    let manifest = parse_modrinth_index(json).unwrap();
    assert_eq!(manifest.loader.kind, "neoforge");
    assert_eq!(manifest.loader.version.as_deref(), Some("21.1.0"));
}

#[test]
fn cp1_no_loader_key_maps_to_vanilla() {
    let json = include_str!("fixtures/mrpack_vanilla.json");
    let manifest = parse_modrinth_index(json).unwrap();
    assert_eq!(manifest.loader.kind, "vanilla");
    assert!(manifest.loader.version.is_none());
    assert_eq!(manifest.minecraft, "1.20.4");
}

#[test]
fn cp1_env_unsupported_client_is_flagged() {
    let json = include_str!("fixtures/mrpack_fabric.json");
    let manifest = parse_modrinth_index(json).unwrap();
    // Second file has env.client == "unsupported".
    let server_file = manifest
        .files
        .iter()
        .find(|f| f.path == "mods/server-only-mod.jar")
        .unwrap();
    assert!(!server_file.client_supported());
}

#[test]
fn cp1_absent_env_is_client_supported() {
    let json = include_str!("fixtures/mrpack_quilt.json");
    let manifest = parse_modrinth_index(json).unwrap();
    // qfapi.jar has no env field.
    let file = &manifest.files[0];
    assert!(file.env.is_none());
    assert!(file.client_supported());
}

#[test]
fn cp1_malformed_json_returns_error() {
    let result = parse_modrinth_index("{ not json }");
    assert!(matches!(result, Err(ModpackError::MalformedManifest(_))));
}

#[test]
fn cp1_missing_minecraft_dep_returns_error() {
    let json = r#"{
            "formatVersion": 1,
            "game": "minecraft",
            "name": "Bad Pack",
            "versionId": "1.0",
            "dependencies": {},
            "files": []
        }"#;
    let result = parse_modrinth_index(json);
    assert!(matches!(result, Err(ModpackError::MalformedManifest(_))));
}

// ── B1: parse_cf_manifest ─────────────────────────────────────────────────

#[test]
fn b1_parse_forge_manifest() {
    let json = include_str!("fixtures/cf_manifest_forge.json");
    let manifest = parse_cf_manifest(json).expect("should parse");
    assert_eq!(manifest.name, "Test Forge Pack");
    assert_eq!(manifest.version, "1.0.0");
    assert_eq!(manifest.minecraft, "1.20.1");
    assert_eq!(manifest.loader.kind, "forge");
    assert_eq!(manifest.loader.version.as_deref(), Some("47.2.0"));
    assert_eq!(manifest.files.len(), 2);
    assert_eq!(manifest.files[0].project_id, 238222);
    assert_eq!(manifest.files[0].file_id, 4536804);
    assert!(manifest.files[0].required);
    assert!(!manifest.files[1].required);
}

#[test]
fn b1_loader_fabric_prefix_maps_correctly() {
    let json = include_str!("fixtures/cf_manifest_fabric.json");
    let manifest = parse_cf_manifest(json).unwrap();
    assert_eq!(manifest.loader.kind, "fabric");
    assert_eq!(manifest.loader.version.as_deref(), Some("0.16.0"));
}

#[test]
fn b1_loader_quilt_prefix_maps_correctly() {
    let json = r#"{
            "minecraft": { "version": "1.21.1", "modLoaders": [{ "id": "quilt-0.27.1", "primary": true }] },
            "manifestType": "minecraftModpack",
            "manifestVersion": 1,
            "name": "Quilt Pack",
            "version": "1.0",
            "author": "X",
            "files": [],
            "overrides": "overrides"
        }"#;
    let manifest = parse_cf_manifest(json).unwrap();
    assert_eq!(manifest.loader.kind, "quilt");
    assert_eq!(manifest.loader.version.as_deref(), Some("0.27.1"));
}

#[test]
fn b1_loader_neoforge_prefix_maps_correctly() {
    let json = r#"{
            "minecraft": { "version": "1.21.1", "modLoaders": [{ "id": "neoforge-21.1.0", "primary": true }] },
            "manifestType": "minecraftModpack",
            "manifestVersion": 1,
            "name": "NeoForge Pack",
            "version": "1.0",
            "author": "X",
            "files": [],
            "overrides": "overrides"
        }"#;
    let manifest = parse_cf_manifest(json).unwrap();
    assert_eq!(manifest.loader.kind, "neoforge");
    assert_eq!(manifest.loader.version.as_deref(), Some("21.1.0"));
}

#[test]
fn b1_no_loader_entry_maps_to_vanilla() {
    let json = include_str!("fixtures/cf_manifest_vanilla.json");
    let manifest = parse_cf_manifest(json).unwrap();
    assert_eq!(manifest.loader.kind, "vanilla");
    assert!(manifest.loader.version.is_none());
    assert_eq!(manifest.minecraft, "1.20.4");
    assert!(manifest.files.is_empty());
}

#[test]
fn b1_non_primary_loader_entry_is_ignored() {
    // Only the primary modLoaders[] entry should be used.
    let json = r#"{
            "minecraft": { "version": "1.20.1", "modLoaders": [
                { "id": "forge-47.2.0", "primary": true },
                { "id": "fabric-0.16.0", "primary": false }
            ] },
            "manifestType": "minecraftModpack",
            "manifestVersion": 1,
            "name": "Mixed Pack",
            "version": "1.0",
            "author": "X",
            "files": [],
            "overrides": "overrides"
        }"#;
    let manifest = parse_cf_manifest(json).unwrap();
    assert_eq!(manifest.loader.kind, "forge");
    assert_eq!(manifest.loader.version.as_deref(), Some("47.2.0"));
}

#[test]
fn b1_malformed_json_returns_error() {
    let result = parse_cf_manifest("{ not json }");
    assert!(matches!(result, Err(ModpackError::MalformedManifest(_))));
}

#[test]
fn b1_missing_minecraft_field_returns_error() {
    let json = r#"{
            "manifestType": "minecraftModpack",
            "manifestVersion": 1,
            "name": "Bad Pack",
            "version": "1.0",
            "author": "X",
            "files": [],
            "overrides": "overrides"
        }"#;
    let result = parse_cf_manifest(json);
    assert!(matches!(result, Err(ModpackError::MalformedManifest(_))));
}

#[test]
fn b1_loader_id_without_dash_returns_error() {
    // A modLoaders[].id with no '-' separator can't be split into kind+version.
    let json = r#"{
            "minecraft": { "version": "1.20.1", "modLoaders": [{ "id": "forgeonly", "primary": true }] },
            "manifestType": "minecraftModpack",
            "manifestVersion": 1,
            "name": "Bad Loader Pack",
            "version": "1.0",
            "author": "X",
            "files": [],
            "overrides": "overrides"
        }"#;
    let result = parse_cf_manifest(json);
    assert!(matches!(result, Err(ModpackError::MalformedManifest(_))));
}

// ── B3: build_cf_pack_plan ─────────────────────────────────────────────────

use crate::core::providers::VersionFile;

fn cf_manifest_file(project_id: u64, file_id: u64) -> CfManifestFile {
    CfManifestFile {
        project_id,
        file_id,
        required: true,
    }
}

fn version_file_with(url: Option<&str>, file_name: &str, hashes: &[(&str, &str)]) -> VersionFile {
    VersionFile {
        url: url.map(|s| s.to_string()),
        file_name: file_name.to_string(),
        size: Some(1024),
        hashes: hashes
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        primary: true,
    }
}

#[test]
fn b3_dest_resolves_under_mc_mods() {
    let tmp = mc_dir();
    let resolved = vec![(
        cf_manifest_file(238222, 4536804),
        version_file_with(
            Some("https://edge.forgecdn.net/files/1/2/jei.jar"),
            "jei.jar",
            &[("sha1", "aabbcc")],
        ),
    )];
    let plan = build_cf_pack_plan(&resolved, tmp.path()).unwrap();
    assert_eq!(plan.items.len(), 1);
    assert_eq!(plan.items[0].dest, tmp.path().join("mods/jei.jar"));
}

#[test]
fn b3_mod_entry_ids_are_project_and_file_id() {
    let tmp = mc_dir();
    let resolved = vec![(
        cf_manifest_file(238222, 4536804),
        version_file_with(
            Some("https://edge.forgecdn.net/files/1/2/jei.jar"),
            "jei.jar",
            &[("sha1", "aabbcc")],
        ),
    )];
    let plan = build_cf_pack_plan(&resolved, tmp.path()).unwrap();
    assert_eq!(plan.mods.len(), 1);
    assert_eq!(plan.mods[0].provider, "curseforge");
    assert_eq!(plan.mods[0].project_id, "238222");
    assert_eq!(plan.mods[0].version_id, "4536804");
    assert!(plan.mods[0].enabled);
}

#[test]
fn b3_url_none_routes_to_manual() {
    let tmp = mc_dir();
    let resolved = vec![(
        cf_manifest_file(238222, 4536804),
        version_file_with(None, "jei.jar", &[("sha1", "aabbcc")]),
    )];
    let plan = build_cf_pack_plan(&resolved, tmp.path()).unwrap();
    assert!(plan.items.is_empty());
    assert!(plan.mods.is_empty());
    assert_eq!(plan.manual.len(), 1);
    assert_eq!(plan.manual[0].project_id, 238222);
    assert_eq!(plan.manual[0].file_id, 4536804);
    assert_eq!(
        plan.manual[0].page_url,
        "https://www.curseforge.com/projects/238222"
    );
}

#[test]
fn b3_no_sha1_md5_only_routes_to_manual() {
    let tmp = mc_dir();
    let resolved = vec![(
        cf_manifest_file(238222, 4536804),
        version_file_with(
            Some("https://edge.forgecdn.net/files/1/2/jei.jar"),
            "jei.jar",
            &[("md5", "ddeeff")],
        ),
    )];
    let plan = build_cf_pack_plan(&resolved, tmp.path()).unwrap();
    assert!(plan.items.is_empty());
    assert_eq!(plan.manual.len(), 1);
}

#[test]
fn b3_manual_file_does_not_abort_other_entries() {
    let tmp = mc_dir();
    let resolved = vec![
        (
            cf_manifest_file(1, 2),
            version_file_with(None, "manual-mod.jar", &[("sha1", "aabbcc")]),
        ),
        (
            cf_manifest_file(238222, 4536804),
            version_file_with(
                Some("https://edge.forgecdn.net/files/1/2/jei.jar"),
                "jei.jar",
                &[("sha1", "aabbcc")],
            ),
        ),
    ];
    let plan = build_cf_pack_plan(&resolved, tmp.path()).unwrap();
    assert_eq!(plan.manual.len(), 1);
    assert_eq!(plan.items.len(), 1);
    assert_eq!(plan.mods.len(), 1);
}

#[test]
fn b3_unsafe_filename_returns_error() {
    let tmp = mc_dir();
    let resolved = vec![(
        cf_manifest_file(238222, 4536804),
        version_file_with(
            Some("https://edge.forgecdn.net/files/1/2/evil.jar"),
            "../../etc/passwd",
            &[("sha1", "aabbcc")],
        ),
    )];
    let result = build_cf_pack_plan(&resolved, tmp.path());
    assert!(matches!(result, Err(ModpackError::UnsafePath(_))));
}

// ── CP2: build_pack_plan ──────────────────────────────────────────────────

fn mc_dir() -> TempDir {
    tempfile::tempdir().unwrap()
}

#[test]
fn cp2_dest_path_resolves_under_mc_dir() {
    let tmp = mc_dir();
    let mc = tmp.path();
    let json = include_str!("fixtures/mrpack_fabric.json");
    let manifest = parse_modrinth_index(json).unwrap();
    let plan = build_pack_plan(&manifest, mc).unwrap();
    for item in &plan.items {
        assert!(
            item.dest.starts_with(mc),
            "dest {} does not start with mc_dir {}",
            item.dest.display(),
            mc.display()
        );
    }
}

#[test]
fn cp2_sha512_preferred_over_sha1() {
    let tmp = mc_dir();
    let json = include_str!("fixtures/mrpack_fabric.json");
    let manifest = parse_modrinth_index(json).unwrap();
    let plan = build_pack_plan(&manifest, tmp.path()).unwrap();
    // Sodium mod has both sha1 and sha512; sha512 must win.
    let sodium = plan
        .items
        .iter()
        .find(|i| i.dest.to_string_lossy().contains("sodium"))
        .expect("sodium item");
    assert!(matches!(
        sodium.expected_hash,
        Some(ExpectedHash::Sha512(_))
    ));
}

#[test]
fn cp2_sha1_used_when_sha512_absent() {
    let json = r#"{
            "formatVersion": 1,
            "game": "minecraft",
            "name": "Pack",
            "versionId": "1.0",
            "dependencies": { "minecraft": "1.21.1" },
            "files": [{
                "path": "mods/only-sha1.jar",
                "hashes": { "sha1": "aabbccddee" },
                "downloads": ["https://cdn.modrinth.com/data/X/versions/1.0/only-sha1.jar"],
                "fileSize": 100
            }]
        }"#;
    let tmp = mc_dir();
    let manifest = parse_modrinth_index(json).unwrap();
    let plan = build_pack_plan(&manifest, tmp.path()).unwrap();
    assert!(matches!(
        plan.items[0].expected_hash,
        Some(ExpectedHash::Sha1(_))
    ));
}

#[test]
fn cp2_no_hash_returns_error() {
    let json = r#"{
            "formatVersion": 1,
            "game": "minecraft",
            "name": "Pack",
            "versionId": "1.0",
            "dependencies": { "minecraft": "1.21.1" },
            "files": [{
                "path": "mods/no-hash.jar",
                "hashes": {},
                "downloads": ["https://cdn.modrinth.com/data/X/versions/1.0/no-hash.jar"],
                "fileSize": 100
            }]
        }"#;
    let tmp = mc_dir();
    let manifest = parse_modrinth_index(json).unwrap();
    let result = build_pack_plan(&manifest, tmp.path());
    assert!(matches!(result, Err(ModpackError::MissingHash(_))));
}

#[test]
fn cp2_disallowed_host_returns_error() {
    let json = r#"{
            "formatVersion": 1,
            "game": "minecraft",
            "name": "Pack",
            "versionId": "1.0",
            "dependencies": { "minecraft": "1.21.1" },
            "files": [{
                "path": "mods/evil.jar",
                "hashes": { "sha512": "aabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccdd" },
                "downloads": ["https://evil.example.com/evil.jar"],
                "fileSize": 100
            }]
        }"#;
    let tmp = mc_dir();
    let manifest = parse_modrinth_index(json).unwrap();
    let result = build_pack_plan(&manifest, tmp.path());
    assert!(matches!(result, Err(ModpackError::DisallowedHost { .. })));
}

#[test]
fn cp2_dotdot_path_returns_error() {
    let json = r#"{
            "formatVersion": 1,
            "game": "minecraft",
            "name": "Pack",
            "versionId": "1.0",
            "dependencies": { "minecraft": "1.21.1" },
            "files": [{
                "path": "../../../etc/passwd",
                "hashes": { "sha512": "aabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccdd" },
                "downloads": ["https://cdn.modrinth.com/data/X/versions/1.0/evil.jar"],
                "fileSize": 100
            }]
        }"#;
    let tmp = mc_dir();
    let manifest = parse_modrinth_index(json).unwrap();
    let result = build_pack_plan(&manifest, tmp.path());
    assert!(matches!(result, Err(ModpackError::UnsafePath(_))));
}

#[test]
fn cp2_absolute_path_returns_error() {
    let json = r#"{
            "formatVersion": 1,
            "game": "minecraft",
            "name": "Pack",
            "versionId": "1.0",
            "dependencies": { "minecraft": "1.21.1" },
            "files": [{
                "path": "/etc/passwd",
                "hashes": { "sha512": "aabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccdd" },
                "downloads": ["https://cdn.modrinth.com/data/X/versions/1.0/evil.jar"],
                "fileSize": 100
            }]
        }"#;
    let tmp = mc_dir();
    let manifest = parse_modrinth_index(json).unwrap();
    let result = build_pack_plan(&manifest, tmp.path());
    assert!(matches!(result, Err(ModpackError::UnsafePath(_))));
}

#[test]
fn cp2_env_unsupported_goes_to_skipped() {
    let tmp = mc_dir();
    let json = include_str!("fixtures/mrpack_fabric.json");
    let manifest = parse_modrinth_index(json).unwrap();
    let plan = build_pack_plan(&manifest, tmp.path()).unwrap();
    assert!(plan
        .skipped
        .contains(&"mods/server-only-mod.jar".to_string()));
}

#[test]
fn cp2_only_mods_prefix_files_get_mod_entry() {
    let tmp = mc_dir();
    let json = include_str!("fixtures/mrpack_fabric.json");
    let manifest = parse_modrinth_index(json).unwrap();
    let plan = build_pack_plan(&manifest, tmp.path()).unwrap();
    // Only sodium (mods/) should have a ModEntry; config file should not.
    assert_eq!(plan.mods.len(), 1);
    assert_eq!(plan.mods[0].file_name, "sodium-fabric-0.6.0.jar");
    assert_eq!(plan.mods[0].provider, "modrinth");
    assert!(plan.mods[0].project_id.is_empty());
    assert!(plan.mods[0].version_id.is_empty());
    assert!(plan.mods[0].enabled);
}

#[test]
fn cp2_mod_entry_side_from_env() {
    let tmp = mc_dir();
    let json = include_str!("fixtures/mrpack_fabric.json");
    let manifest = parse_modrinth_index(json).unwrap();
    let plan = build_pack_plan(&manifest, tmp.path()).unwrap();
    // Sodium has client=required, server=unsupported → side should be "client".
    assert_eq!(plan.mods[0].side, "client");
}

#[test]
fn cp2_size_populated_on_download_item() {
    let tmp = mc_dir();
    let json = include_str!("fixtures/mrpack_fabric.json");
    let manifest = parse_modrinth_index(json).unwrap();
    let plan = build_pack_plan(&manifest, tmp.path()).unwrap();
    let sodium = plan
        .items
        .iter()
        .find(|i| i.dest.to_string_lossy().contains("sodium"))
        .unwrap();
    assert_eq!(sodium.size, Some(1048576));
}

#[test]
fn cp2_empty_downloads_returns_error() {
    // A file with downloads: [] must produce a clear NoDownloadUrls error,
    // not a misleading DisallowedHost with an empty host string.
    let json = r#"{
            "formatVersion": 1,
            "game": "minecraft",
            "name": "Pack",
            "versionId": "1.0",
            "dependencies": { "minecraft": "1.21.1" },
            "files": [{
                "path": "mods/no-url.jar",
                "hashes": { "sha512": "aabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccdd" },
                "downloads": [],
                "fileSize": 100
            }]
        }"#;
    let tmp = mc_dir();
    let manifest = parse_modrinth_index(json).unwrap();
    let result = build_pack_plan(&manifest, tmp.path());
    assert!(
        matches!(result, Err(ModpackError::NoDownloadUrls(ref p)) if p == "mods/no-url.jar"),
        "expected NoDownloadUrls(\"mods/no-url.jar\"), got: {:?}",
        result
    );
}

#[test]
fn cp2_is_safe_dest_rejects_dotdot_in_rel() {
    // Exercises is_safe_dest directly: a joined path whose relative suffix
    // contains ".." must be rejected even if validate_relative_path already
    // catches it upstream.  This test is the primary coverage gate for the
    // structural guard — do not remove it if validate_relative_path is ever
    // refactored or bypassed.
    use std::path::PathBuf;
    let base = PathBuf::from("/tmp/mc");
    // Simulate what mc_dir.join("../escape") produces.
    let dest = base.join("..").join("escape");
    assert!(
        !is_safe_dest(&dest, &base),
        "is_safe_dest should reject a path with '..' that escapes the base"
    );
}

#[test]
fn cp2_github_host_is_allowed() {
    let tmp = mc_dir();
    let json = include_str!("fixtures/mrpack_fabric.json");
    let manifest = parse_modrinth_index(json).unwrap();
    let plan = build_pack_plan(&manifest, tmp.path()).unwrap();
    // config/some-config.toml downloads from raw.githubusercontent.com.
    let config_item = plan
        .items
        .iter()
        .find(|i| i.dest.to_string_lossy().contains("some-config"))
        .unwrap();
    assert!(config_item.url.contains("raw.githubusercontent.com"));
}

// ── CP3: extract_overrides ────────────────────────────────────────────────

/// Build an in-memory zip archive for testing.
fn build_test_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let buf = Vec::new();
    let cursor = std::io::Cursor::new(buf);
    let mut zip = zip::ZipWriter::new(cursor);
    let options = SimpleFileOptions::default();
    for (name, data) in entries {
        zip.start_file(*name, options).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn cp3_overrides_files_land_under_mc_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let mc_dir = tmp.path();

    let zip_bytes = build_test_zip(&[("overrides/config/my.toml", b"[settings]")]);
    let cursor = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor).unwrap();
    let count = extract_overrides(&mut archive, mc_dir).unwrap();
    assert_eq!(count, 1);
    assert!(mc_dir.join("config/my.toml").exists());
}

#[test]
fn cp3_client_overrides_applied() {
    let tmp = tempfile::tempdir().unwrap();
    let mc_dir = tmp.path();

    let zip_bytes = build_test_zip(&[("client-overrides/options.txt", b"renderDistance:12")]);
    let cursor = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor).unwrap();
    let count = extract_overrides(&mut archive, mc_dir).unwrap();
    assert_eq!(count, 1);
    assert!(mc_dir.join("options.txt").exists());
}

#[test]
fn cp3_server_overrides_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    let mc_dir = tmp.path();

    let zip_bytes = build_test_zip(&[
        ("overrides/client.txt", b"client"),
        ("server-overrides/server.txt", b"server"),
    ]);
    let cursor = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor).unwrap();
    let count = extract_overrides(&mut archive, mc_dir).unwrap();
    assert_eq!(count, 1);
    assert!(mc_dir.join("client.txt").exists());
    assert!(!mc_dir.join("server.txt").exists());
}

#[test]
fn cp3_zipslip_entry_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let mc_dir = tmp.path();

    // A zip-slip entry: overrides/../../../etc/passwd would escape.
    let zip_bytes = build_test_zip(&[("overrides/../../../escape.txt", b"pwned")]);
    let cursor = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor).unwrap();
    let result = extract_overrides(&mut archive, mc_dir);
    // Should return either ZipSlip or UnsafePath.
    assert!(
        matches!(
            result,
            Err(ModpackError::ZipSlip(_)) | Err(ModpackError::UnsafePath(_))
        ),
        "expected ZipSlip or UnsafePath, got: {:?}",
        result
    );
}

#[test]
fn cp3_collision_override_wins() {
    let tmp = tempfile::tempdir().unwrap();
    let mc_dir = tmp.path();

    // Write a file that looks like it came from the download step.
    let target = mc_dir.join("options.txt");
    std::fs::write(&target, b"original").unwrap();

    // Override it.
    let zip_bytes = build_test_zip(&[("client-overrides/options.txt", b"overridden")]);
    let cursor = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor).unwrap();
    extract_overrides(&mut archive, mc_dir).unwrap();

    let content = std::fs::read(&target).unwrap();
    assert_eq!(content, b"overridden");
}

#[test]
fn cp3_count_excludes_server_overrides() {
    let tmp = tempfile::tempdir().unwrap();
    let mc_dir = tmp.path();

    let zip_bytes = build_test_zip(&[
        ("overrides/a.txt", b"a"),
        ("client-overrides/b.txt", b"b"),
        ("server-overrides/c.txt", b"c"),
    ]);
    let cursor = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor).unwrap();
    let count = extract_overrides(&mut archive, mc_dir).unwrap();
    assert_eq!(count, 2);
}

// ── CP4: read_mrpack ──────────────────────────────────────────────────────

/// Build an in-memory `.mrpack` zip with a `modrinth.index.json` entry.
fn build_mrpack(index_json: &str) -> Vec<u8> {
    build_test_zip(&[("modrinth.index.json", index_json.as_bytes())])
}

#[test]
fn cp4_read_mrpack_happy_path_parses_manifest_and_plan() {
    let tmp = mc_dir();
    // Use the fabric fixture JSON (has 3 files: 1 client mod + 1 server-only + 1 config).
    let index_json = include_str!("fixtures/mrpack_fabric.json");
    let mrpack_bytes = build_mrpack(index_json);

    let (manifest, plan) =
        read_mrpack(&mrpack_bytes, tmp.path()).expect("should parse happy-path mrpack");

    assert_eq!(manifest.name, "Test Fabric Pack");
    assert_eq!(manifest.minecraft, "1.21.1");
    assert_eq!(manifest.loader.kind, "fabric");

    // 1 client mod (sodium) + 1 config file = 2 download items
    // (server-only-mod.jar is skipped).
    assert_eq!(plan.items.len(), 2, "expected 2 download items");
    assert_eq!(plan.skipped.len(), 1, "expected 1 skipped file");
    assert_eq!(plan.mods.len(), 1, "expected 1 mod entry");
    assert_eq!(plan.mods[0].file_name, "sodium-fabric-0.6.0.jar");

    // All dest paths must be under the mc_dir.
    for item in &plan.items {
        assert!(item.dest.starts_with(tmp.path()));
    }
}

#[test]
fn cp4_read_mrpack_missing_index_returns_error() {
    // A zip with no modrinth.index.json entry.
    let mrpack_bytes = build_test_zip(&[("README.txt", b"hello")]);
    let tmp = mc_dir();
    let result = read_mrpack(&mrpack_bytes, tmp.path());
    assert!(
        matches!(result, Err(ModpackError::IndexNotFound)),
        "expected IndexNotFound, got: {:?}",
        result
    );
}

#[test]
fn cp4_read_mrpack_malformed_json_returns_error() {
    let mrpack_bytes = build_mrpack("{ not valid json }");
    let tmp = mc_dir();
    let result = read_mrpack(&mrpack_bytes, tmp.path());
    assert!(
        matches!(result, Err(ModpackError::MalformedManifest(_))),
        "expected MalformedManifest, got: {:?}",
        result
    );
}

#[test]
fn cp4_read_mrpack_not_a_zip_returns_error() {
    // Random bytes that are not a valid zip.
    let not_a_zip = b"this is definitely not a zip file";
    let tmp = mc_dir();
    let result = read_mrpack(not_a_zip, tmp.path());
    assert!(
        matches!(result, Err(ModpackError::Zip(_))),
        "expected Zip error for non-zip bytes, got: {:?}",
        result
    );
}

#[test]
fn cp4_read_mrpack_disallowed_host_propagates_error() {
    let index_json = r#"{
            "formatVersion": 1,
            "game": "minecraft",
            "name": "Evil Pack",
            "versionId": "1.0",
            "dependencies": { "minecraft": "1.21.1", "fabric-loader": "0.15.0" },
            "files": [{
                "path": "mods/evil.jar",
                "hashes": { "sha512": "aabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccdd" },
                "downloads": ["https://evil.example.com/evil.jar"],
                "fileSize": 100
            }]
        }"#;
    let mrpack_bytes = build_mrpack(index_json);
    let tmp = mc_dir();
    let result = read_mrpack(&mrpack_bytes, tmp.path());
    assert!(
        matches!(result, Err(ModpackError::DisallowedHost { .. })),
        "expected DisallowedHost from read_mrpack, got: {:?}",
        result
    );
}

// ── B4: resolve_and_build_cf_plan + read_cf_manifest ─────────────────────

use crate::core::curseforge::CurseForgeProvider;
use crate::core::providers::ProviderHttpClient;
use std::collections::VecDeque;
use tokio::sync::Mutex as TokioMutex;

struct MockResp(u16, String);

/// Minimal mock `ProviderHttpClient` returning canned responses in order —
/// same pattern as `curseforge.rs`'s test client, kept local to avoid
/// widening that module's test-only visibility.
struct MockCfClient {
    responses: TokioMutex<VecDeque<MockResp>>,
}

impl MockCfClient {
    fn new(responses: Vec<MockResp>) -> Self {
        Self {
            responses: TokioMutex::new(responses.into_iter().collect()),
        }
    }
}

#[async_trait::async_trait]
impl ProviderHttpClient for MockCfClient {
    async fn get(
        &self,
        _url: &str,
        _headers: &[(&str, &str)],
    ) -> Result<(u16, String), reqwest::Error> {
        let mut q = self.responses.lock().await;
        let MockResp(s, b) = q
            .pop_front()
            .expect("MockCfClient: no more canned responses");
        Ok((s, b))
    }
}

fn build_cf_zip(manifest_json: &str) -> Vec<u8> {
    build_test_zip(&[("manifest.json", manifest_json.as_bytes())])
}

#[test]
fn b4_read_cf_manifest_happy_path() {
    let json = include_str!("fixtures/cf_manifest_forge.json");
    let zip_bytes = build_cf_zip(json);
    let manifest = read_cf_manifest(&zip_bytes).expect("should parse");
    assert_eq!(manifest.name, "Test Forge Pack");
    assert_eq!(manifest.minecraft, "1.20.1");
    assert_eq!(manifest.loader.kind, "forge");
    assert_eq!(manifest.files.len(), 2);
}

#[test]
fn b4_read_cf_manifest_malformed_zip_returns_error() {
    let not_a_zip = b"this is definitely not a zip file";
    let result = read_cf_manifest(not_a_zip);
    assert!(
        matches!(result, Err(ModpackError::Zip(_))),
        "expected Zip error for non-zip bytes, got: {:?}",
        result
    );
}

#[test]
fn b4_read_cf_manifest_missing_manifest_returns_error() {
    let zip_bytes = build_test_zip(&[("README.txt", b"hello")]);
    let result = read_cf_manifest(&zip_bytes);
    assert!(
        matches!(result, Err(ModpackError::IndexNotFound)),
        "expected IndexNotFound, got: {:?}",
        result
    );
}

#[tokio::test]
async fn b4_resolve_and_build_cf_plan_happy_path_counts() {
    let json = include_str!("fixtures/cf_manifest_forge.json");
    let manifest = parse_cf_manifest(json).unwrap();
    let tmp = mc_dir();

    // Forge fixture has 2 files; resolve both to the auto-installable fixture.
    let cf_file_fixture = include_str!("fixtures/cf_file.json");
    let client = MockCfClient::new(vec![
        MockResp(200, cf_file_fixture.to_string()),
        MockResp(200, cf_file_fixture.to_string()),
    ]);
    let provider = CurseForgeProvider::new(Some("key".to_string()));

    let plan = resolve_and_build_cf_plan(&provider, &client, &manifest, tmp.path())
        .await
        .expect("should resolve and plan");

    assert_eq!(plan.items.len(), 2, "both files should be downloadable");
    assert_eq!(plan.mods.len(), 2);
    assert!(plan.manual.is_empty());
}

#[tokio::test]
async fn b4_resolve_and_build_cf_plan_routes_through_pure_build_cf_pack_plan() {
    // One file resolves with a usable url+hash (installed), one resolves
    // with downloadUrl: null (manual) — proves the wiring routes resolved
    // pairs through the same `build_cf_pack_plan` decision logic tested in B3.
    let json = include_str!("fixtures/cf_manifest_forge.json");
    let manifest = parse_cf_manifest(json).unwrap();
    let tmp = mc_dir();

    let cf_file_fixture = include_str!("fixtures/cf_file.json");
    let cf_file_no_url = include_str!("fixtures/cf_file_no_url_md5_only.json");
    let client = MockCfClient::new(vec![
        MockResp(200, cf_file_fixture.to_string()),
        MockResp(200, cf_file_no_url.to_string()),
    ]);
    let provider = CurseForgeProvider::new(Some("key".to_string()));

    let plan = resolve_and_build_cf_plan(&provider, &client, &manifest, tmp.path())
        .await
        .unwrap();

    assert_eq!(
        plan.items.len(),
        1,
        "only the resolved-with-hash file installs"
    );
    assert_eq!(plan.mods.len(), 1);
    assert_eq!(
        plan.manual.len(),
        1,
        "null-url/no-sha1 file routes to manual"
    );
}

#[tokio::test]
async fn b4_resolve_and_build_cf_plan_get_file_error_routes_to_failed_not_manual() {
    // A CF outage / 404 for one entry must not abort the whole import —
    // but it is a resolution *error*, not a legitimate "must download
    // manually" entry, so it must land in `failed`, not `manual`.
    let json = include_str!("fixtures/cf_manifest_forge.json");
    let manifest = parse_cf_manifest(json).unwrap();
    let tmp = mc_dir();

    let cf_file_fixture = include_str!("fixtures/cf_file.json");
    let client = MockCfClient::new(vec![
        MockResp(404, "Not Found".to_string()),
        MockResp(200, cf_file_fixture.to_string()),
    ]);
    let provider = CurseForgeProvider::new(Some("key".to_string()));

    let plan = resolve_and_build_cf_plan(&provider, &client, &manifest, tmp.path())
        .await
        .expect("a per-entry resolution failure must not abort the whole plan");

    assert_eq!(
        plan.items.len(),
        1,
        "the successfully-resolved entry still installs"
    );
    assert!(
        plan.manual.is_empty(),
        "a resolution error is not a manual entry"
    );
    assert_eq!(plan.failed.len(), 1, "the failed entry routes to failed");
    assert!(plan.failed[0].reason.contains("404"));
}

#[tokio::test]
async fn b4_resolve_and_build_cf_plan_key_missing_aborts_import() {
    // No CF API key configured means no file can possibly resolve —
    // the whole import must abort instead of silently producing a pack
    // full of misleading "manual" entries.
    let json = include_str!("fixtures/cf_manifest_forge.json");
    let manifest = parse_cf_manifest(json).unwrap();
    let tmp = mc_dir();

    let client = MockCfClient::new(vec![]);
    let provider = CurseForgeProvider::new(None); // no key configured

    let result = resolve_and_build_cf_plan(&provider, &client, &manifest, tmp.path()).await;

    assert!(
        matches!(result, Err(ModpackError::ResolverKeyMissing)),
        "expected ResolverKeyMissing, got: {:?}",
        result
    );
}

// ── C2: resolve_pack_file ─────────────────────────────────────────────────

use crate::core::modrinth::ModrinthProvider;
use crate::core::providers::{ModProvider, ProviderKind};

/// Modrinth hash object — both sha1 and sha512 are required by the Modrinth API.
const MR_HASHES: &str = r#"{"sha1": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "sha512": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#;
const MR_HASHES_B: &str = r#"{"sha1": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "sha512": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}"#;

/// Build a minimal Modrinth versions JSON array with one version, one file.
///
/// Modrinth `MrFile.url` is a non-optional String — `url` param must be a valid URL string.
fn mr_versions_json(primary: bool, url: &str) -> String {
    format!(
        r#"[{{
            "id": "VER001",
            "project_id": "PROJ001",
            "name": "Pack v1.0",
            "version_number": "1.0.0",
            "game_versions": ["1.21.1"],
            "loaders": ["fabric"],
            "files": [{{
                "url": "{}",
                "filename": "mypack-1.0.mrpack",
                "size": 2048,
                "hashes": {},
                "primary": {}
            }}],
            "dependencies": []
        }}]"#,
        url, MR_HASHES, primary
    )
}

/// Build a Modrinth versions JSON with two files: non-primary then primary.
fn mr_versions_two_files_json() -> String {
    format!(
        r#"[{{
            "id": "VER001",
            "project_id": "PROJ001",
            "name": "Pack v1.0",
            "version_number": "1.0.0",
            "game_versions": ["1.21.1"],
            "loaders": ["fabric"],
            "files": [
                {{
                    "url": "https://cdn.modrinth.com/data/PROJ001/secondary.mrpack",
                    "filename": "secondary.mrpack",
                    "size": 1024,
                    "hashes": {},
                    "primary": false
                }},
                {{
                    "url": "https://cdn.modrinth.com/data/PROJ001/primary.mrpack",
                    "filename": "primary.mrpack",
                    "size": 2048,
                    "hashes": {},
                    "primary": true
                }}
            ],
            "dependencies": []
        }}]"#,
        MR_HASHES_B, MR_HASHES
    )
}

/// Build an empty Modrinth versions JSON array.
fn mr_versions_empty_json() -> String {
    "[]".to_string()
}

/// Build a Modrinth versions JSON with a version that has no files.
fn mr_versions_no_files_json() -> String {
    r#"[{
        "id": "VER001",
        "project_id": "PROJ001",
        "name": "Pack v1.0",
        "version_number": "1.0.0",
        "game_versions": ["1.21.1"],
        "loaders": ["fabric"],
        "files": [],
        "dependencies": []
    }]"#
    .to_string()
}

/// Build a CF files response with one entry that has `downloadUrl: null` (distribution-disabled).
fn cf_versions_url_none_json() -> String {
    r#"{
        "data": [{
            "id": 9999001,
            "modId": 12345,
            "displayName": "mypack-1.0.zip",
            "fileName": "mypack-1.0.zip",
            "fileDate": "2024-01-01T00:00:00.000Z",
            "fileLength": 2048,
            "downloadCount": 0,
            "downloadUrl": null,
            "gameVersions": ["1.21.1", "Fabric"],
            "sortableGameVersions": [],
            "dependencies": [],
            "isAvailable": true,
            "isServerPack": false,
            "fileFingerprint": 0,
            "hashes": [
                { "value": "abc123sha1hashvalue000000000000000000000", "algo": 1 }
            ],
            "modules": []
        }]
    }"#
    .to_string()
}

#[tokio::test]
async fn c2_modrinth_picks_first_version_primary_file() {
    // Two-version response (newest first); first version must be returned, not the second.
    let two_versions = format!(
        r#"[
            {{
                "id": "VER002",
                "project_id": "PROJ001",
                "name": "Pack v2.0 (newest)",
                "version_number": "2.0.0",
                "game_versions": ["1.21.1"],
                "loaders": ["fabric"],
                "files": [{{
                    "url": "https://cdn.modrinth.com/data/PROJ001/v2.mrpack",
                    "filename": "mypack-2.0.mrpack",
                    "size": 3000,
                    "hashes": {},
                    "primary": true
                }}],
                "dependencies": []
            }},
            {{
                "id": "VER001",
                "project_id": "PROJ001",
                "name": "Pack v1.0 (older)",
                "version_number": "1.0.0",
                "game_versions": ["1.21.1"],
                "loaders": ["fabric"],
                "files": [{{
                    "url": "https://cdn.modrinth.com/data/PROJ001/v1.mrpack",
                    "filename": "mypack-1.0.mrpack",
                    "size": 2048,
                    "hashes": {},
                    "primary": true
                }}],
                "dependencies": []
            }}
        ]"#,
        MR_HASHES, MR_HASHES_B
    );
    let client = MockCfClient::new(vec![MockResp(200, two_versions)]);
    let provider = ModrinthProvider;

    let resolved = resolve_pack_file(&provider, &client, "PROJ001", None)
        .await
        .expect("should resolve");

    assert_eq!(resolved.file_name, "mypack-2.0.mrpack");
    assert_eq!(
        resolved.url.as_deref(),
        Some("https://cdn.modrinth.com/data/PROJ001/v2.mrpack")
    );
    assert_eq!(resolved.provider, ProviderKind::Modrinth);
}

#[tokio::test]
async fn c2_primary_file_picked_when_present() {
    let client = MockCfClient::new(vec![MockResp(200, mr_versions_two_files_json())]);
    let provider = ModrinthProvider;

    let resolved = resolve_pack_file(&provider, &client, "PROJ001", None)
        .await
        .expect("should resolve");

    assert_eq!(resolved.file_name, "primary.mrpack");
    assert_eq!(
        resolved.url.as_deref(),
        Some("https://cdn.modrinth.com/data/PROJ001/primary.mrpack")
    );
}

#[tokio::test]
async fn c2_no_primary_falls_back_to_first_file() {
    // Single file, primary=false — should still be picked as first file.
    let json = mr_versions_json(false, "https://cdn.modrinth.com/data/PROJ001/only.mrpack");
    let client = MockCfClient::new(vec![MockResp(200, json)]);
    let provider = ModrinthProvider;

    let resolved = resolve_pack_file(&provider, &client, "PROJ001", None)
        .await
        .expect("should resolve with first file when no primary");

    assert_eq!(resolved.file_name, "mypack-1.0.mrpack");
    assert_eq!(
        resolved.url.as_deref(),
        Some("https://cdn.modrinth.com/data/PROJ001/only.mrpack")
    );
}

#[tokio::test]
async fn c2_url_none_is_valid_resolved_state_not_error() {
    // url: null on a CF file is a valid "distribution-disabled" outcome; must NOT error.
    // Modrinth always has a URL; only CF can produce url: None.
    let client = MockCfClient::new(vec![MockResp(200, cf_versions_url_none_json())]);
    let provider = CurseForgeProvider::new(Some("test-key".to_string()));

    let resolved = resolve_pack_file(&provider, &client, "12345", None)
        .await
        .expect("url: None must be a valid resolved state, not an error");

    assert!(resolved.url.is_none(), "url should be None for distribution-disabled CF pack");
    assert_eq!(resolved.file_name, "mypack-1.0.zip");
}

#[tokio::test]
async fn c2_empty_version_list_returns_error() {
    let client = MockCfClient::new(vec![MockResp(200, mr_versions_empty_json())]);
    let provider = ModrinthProvider;

    let result = resolve_pack_file(&provider, &client, "PROJ001", None).await;

    assert!(
        matches!(result, Err(ModpackError::NoVersions)),
        "expected NoVersions, got: {:?}",
        result
    );
}

#[tokio::test]
async fn c2_empty_files_list_returns_error() {
    let client = MockCfClient::new(vec![MockResp(200, mr_versions_no_files_json())]);
    let provider = ModrinthProvider;

    let result = resolve_pack_file(&provider, &client, "PROJ001", None).await;

    assert!(
        matches!(result, Err(ModpackError::NoFiles)),
        "expected NoFiles, got: {:?}",
        result
    );
}

#[tokio::test]
async fn c2_curseforge_provider_picks_first_version_primary_file() {
    // CF returns versions via `cf_files.json` shape; first entry should be picked.
    let cf_versions = include_str!("fixtures/cf_files.json");
    let client = MockCfClient::new(vec![MockResp(200, cf_versions.to_string())]);
    let provider = CurseForgeProvider::new(Some("test-key".to_string()));

    let resolved = resolve_pack_file(&provider, &client, "238222", None)
        .await
        .expect("should resolve CF pack file");

    // cf_files.json has 2 entries (newest first); first entry must be picked.
    assert_eq!(resolved.file_name, "jei-1.20.1-forge-15.3.0.4.jar");
    assert_eq!(
        resolved.url.as_deref(),
        Some("https://edge.forgecdn.net/files/5034/058/jei-1.20.1-forge-15.3.0.4.jar")
    );
    assert_eq!(resolved.provider, ProviderKind::CurseForge);
}

// ── D1: from_pack provenance in planners ──────────────────────────────────

/// `build_pack_plan` (mrpack path) must set `from_pack = true` on all `ModEntry`s
/// it produces. This is the slice-D provenance tag used by the update reconciler.
#[test]
fn d1_build_pack_plan_mod_entries_have_from_pack_true() {
    let json = include_str!("fixtures/mrpack_fabric.json");
    let manifest = parse_modrinth_index(json).expect("should parse");
    let tmp = mc_dir();
    let plan = build_pack_plan(&manifest, tmp.path()).expect("should build plan");

    // fabric fixture has at least one mod under mods/
    assert!(
        !plan.mods.is_empty(),
        "fixture must produce at least one ModEntry"
    );
    for entry in &plan.mods {
        assert!(
            entry.from_pack,
            "mrpack ModEntry '{}' must have from_pack=true",
            entry.file_name
        );
    }
}

/// `build_cf_pack_plan` (CF path) must set `from_pack = true` on all `ModEntry`s.
#[test]
fn d1_build_cf_pack_plan_mod_entries_have_from_pack_true() {
    let tmp = mc_dir();
    let resolved = vec![(
        cf_manifest_file(238222, 4536804),
        version_file_with(
            Some("https://edge.forgecdn.net/files/5034/058/jei.jar"),
            "jei.jar",
            &[("sha1", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")],
        ),
    )];
    let plan = build_cf_pack_plan(&resolved, tmp.path()).unwrap();

    assert_eq!(plan.mods.len(), 1);
    assert!(
        plan.mods[0].from_pack,
        "CF ModEntry must have from_pack=true"
    );
}

// ── D1: version_id + version_name in ResolvedPackFile ─────────────────────

/// `resolve_pack_file` must surface the version id and display name from the
/// resolved `ProjectVersion` so `install_modpack` can build a full `Source`.
#[tokio::test]
async fn d1_resolve_pack_file_surfaces_version_id_and_name() {
    let json = mr_versions_json(true, "https://cdn.modrinth.com/data/PROJ001/pack.mrpack");
    let client = MockCfClient::new(vec![MockResp(200, json)]);
    let provider = ModrinthProvider;

    let resolved = resolve_pack_file(&provider, &client, "PROJ001", None)
        .await
        .expect("should resolve");

    assert_eq!(
        resolved.version_id, "VER001",
        "version_id must match the provider's version id"
    );
    assert_eq!(
        resolved.version_name, "Pack v1.0",
        "version_name must match the provider's version display name"
    );
}

#[test]
fn b4_manual_entries_never_have_empty_file_name() {
    // Once get_file errors are routed to `failed`, `manual` entries come
    // only from build_cf_pack_plan, which always carries the real
    // file_name from the resolved VersionFile.
    let tmp = mc_dir();
    let resolved = vec![(
        cf_manifest_file(238222, 4536804),
        version_file_with(None, "jei-real-name.jar", &[("sha1", "aabbcc")]),
    )];
    let plan = build_cf_pack_plan(&resolved, tmp.path()).unwrap();
    assert_eq!(plan.manual.len(), 1);
    assert!(!plan.manual[0].file_name.is_empty());
    assert_eq!(plan.manual[0].file_name, "jei-real-name.jar");
}

// ── D2: version-targeted resolve ──────────────────────────────────────────

/// Two-version Modrinth response: newest ("VER002") first, older ("VER001") second.
fn mr_two_versions_json() -> String {
    format!(
        r#"[
            {{
                "id": "VER002",
                "project_id": "PROJ001",
                "name": "Pack v2.0 (newest)",
                "version_number": "2.0.0",
                "game_versions": ["1.21.1"],
                "loaders": ["fabric"],
                "files": [{{
                    "url": "https://cdn.modrinth.com/data/PROJ001/v2.mrpack",
                    "filename": "mypack-2.0.mrpack",
                    "size": 3000,
                    "hashes": {},
                    "primary": true
                }}],
                "dependencies": []
            }},
            {{
                "id": "VER001",
                "project_id": "PROJ001",
                "name": "Pack v1.0 (older)",
                "version_number": "1.0.0",
                "game_versions": ["1.21.1"],
                "loaders": ["fabric"],
                "files": [{{
                    "url": "https://cdn.modrinth.com/data/PROJ001/v1.mrpack",
                    "filename": "mypack-1.0.mrpack",
                    "size": 2048,
                    "hashes": {},
                    "primary": true
                }}],
                "dependencies": []
            }}
        ]"#,
        MR_HASHES, MR_HASHES_B
    )
}

/// Two-version CF response: newest (id 5034058) first, older (5034059) second.
fn cf_two_versions_json() -> String {
    r#"{
        "data": [
            {
                "id": 5034058,
                "modId": 238222,
                "displayName": "pack-2.0.zip",
                "fileName": "pack-2.0.zip",
                "fileDate": "2024-02-01T00:00:00.000Z",
                "fileLength": 3000,
                "downloadCount": 100,
                "downloadUrl": "https://edge.forgecdn.net/files/5034/058/pack-2.0.zip",
                "gameVersions": ["1.21.1", "Fabric"],
                "sortableGameVersions": [],
                "dependencies": [],
                "isAvailable": true,
                "isServerPack": false,
                "fileFingerprint": 0,
                "hashes": [
                    { "value": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "algo": 1 }
                ],
                "modules": []
            },
            {
                "id": 5034059,
                "modId": 238222,
                "displayName": "pack-1.0.zip",
                "fileName": "pack-1.0.zip",
                "fileDate": "2024-01-01T00:00:00.000Z",
                "fileLength": 2048,
                "downloadCount": 50,
                "downloadUrl": "https://edge.forgecdn.net/files/5034/059/pack-1.0.zip",
                "gameVersions": ["1.21.1", "Fabric"],
                "sortableGameVersions": [],
                "dependencies": [],
                "isAvailable": true,
                "isServerPack": false,
                "fileFingerprint": 0,
                "hashes": [
                    { "value": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "algo": 1 }
                ],
                "modules": []
            }
        ]
    }"#
    .to_string()
}

/// None → first/latest (slice-C parity, Modrinth).
#[tokio::test]
async fn d2_none_target_picks_latest_modrinth() {
    let client = MockCfClient::new(vec![MockResp(200, mr_two_versions_json())]);
    let provider = ModrinthProvider;

    let resolved = resolve_pack_file(&provider, &client, "PROJ001", None)
        .await
        .expect("should resolve");

    assert_eq!(resolved.version_id, "VER002", "None must pick newest (VER002)");
    assert_eq!(resolved.file_name, "mypack-2.0.mrpack");
}

/// Some(matching id) → that version, not latest (Modrinth).
#[tokio::test]
async fn d2_some_target_picks_specified_version_modrinth() {
    let client = MockCfClient::new(vec![MockResp(200, mr_two_versions_json())]);
    let provider = ModrinthProvider;

    let resolved = resolve_pack_file(&provider, &client, "PROJ001", Some("VER001"))
        .await
        .expect("should resolve targeted version");

    assert_eq!(
        resolved.version_id, "VER001",
        "Some(VER001) must pick the older version, not the latest"
    );
    assert_eq!(resolved.file_name, "mypack-1.0.mrpack");
    assert_eq!(
        resolved.url.as_deref(),
        Some("https://cdn.modrinth.com/data/PROJ001/v1.mrpack")
    );
}

/// Some(unknown id) → VersionNotFound error naming the id (Modrinth).
#[tokio::test]
async fn d2_unknown_target_id_returns_version_not_found_modrinth() {
    let client = MockCfClient::new(vec![MockResp(200, mr_two_versions_json())]);
    let provider = ModrinthProvider;

    let result = resolve_pack_file(&provider, &client, "PROJ001", Some("DOES_NOT_EXIST")).await;

    match result {
        Err(ModpackError::VersionNotFound(id)) => {
            assert_eq!(id, "DOES_NOT_EXIST", "error must name the requested id");
        }
        other => panic!("expected VersionNotFound, got: {:?}", other),
    }
}

/// None → first/latest (slice-C parity, CurseForge).
#[tokio::test]
async fn d2_none_target_picks_latest_curseforge() {
    let client = MockCfClient::new(vec![MockResp(200, cf_two_versions_json())]);
    let provider = CurseForgeProvider::new(Some("test-key".to_string()));

    let resolved = resolve_pack_file(&provider, &client, "238222", None)
        .await
        .expect("should resolve");

    // CF id 5034058 is listed first (newest).
    assert_eq!(resolved.version_id, "5034058", "None must pick newest CF version");
    assert_eq!(resolved.file_name, "pack-2.0.zip");
}

/// Some(matching id) → that version, not latest (CurseForge).
#[tokio::test]
async fn d2_some_target_picks_specified_version_curseforge() {
    let client = MockCfClient::new(vec![MockResp(200, cf_two_versions_json())]);
    let provider = CurseForgeProvider::new(Some("test-key".to_string()));

    // Target the older version (id "5034059").
    let resolved = resolve_pack_file(&provider, &client, "238222", Some("5034059"))
        .await
        .expect("should resolve targeted CF version");

    assert_eq!(resolved.version_id, "5034059");
    assert_eq!(resolved.file_name, "pack-1.0.zip");
    assert_eq!(
        resolved.url.as_deref(),
        Some("https://edge.forgecdn.net/files/5034/059/pack-1.0.zip")
    );
}

/// Some(unknown id) → VersionNotFound error naming the id (CurseForge).
#[tokio::test]
async fn d2_unknown_target_id_returns_version_not_found_curseforge() {
    let client = MockCfClient::new(vec![MockResp(200, cf_two_versions_json())]);
    let provider = CurseForgeProvider::new(Some("test-key".to_string()));

    let result = resolve_pack_file(&provider, &client, "238222", Some("9999999")).await;

    match result {
        Err(ModpackError::VersionNotFound(id)) => {
            assert_eq!(id, "9999999", "error must name the requested id");
        }
        other => panic!("expected VersionNotFound, got: {:?}", other),
    }
}

// ── D3: plan_pack_update pure reconcile helper ────────────────────────────────

/// Build a ModEntry with the given file_name and from_pack flag.
fn mod_entry(file_name: &str, from_pack: bool) -> super::super::instances::ModEntry {
    super::super::instances::ModEntry {
        provider: "modrinth".to_string(),
        project_id: String::new(),
        version_id: String::new(),
        file_name: file_name.to_string(),
        hashes: Default::default(),
        enabled: true,
        side: "both".to_string(),
        from_pack,
    }
}

/// A pack mod absent from the new plan is added to `to_remove`.
#[test]
fn d3_vanished_pack_mod_is_in_to_remove() {
    let current = vec![
        mod_entry("sodium.jar", true),  // pack mod — will vanish
        mod_entry("lithium.jar", true), // pack mod — stays
    ];
    let new_pack = vec![
        mod_entry("lithium.jar", true), // only lithium in the new plan
    ];

    let plan = plan_pack_update(&current, &new_pack);

    assert_eq!(plan.to_remove.len(), 1, "one pack mod vanished");
    assert_eq!(plan.to_remove[0].file_name, "sodium.jar");
}

/// A user mod (`from_pack == false`) is NEVER added to `to_remove`,
/// even when its file_name is absent from the new pack plan.
#[test]
fn d3_user_mod_never_in_to_remove() {
    let current = vec![
        mod_entry("my-mod.jar", false), // user-added, not in the new pack
        mod_entry("sodium.jar", true),  // pack mod — also vanished
    ];
    let new_pack: Vec<super::super::instances::ModEntry> = vec![]; // empty new plan

    let plan = plan_pack_update(&current, &new_pack);

    // Only the pack mod vanishes; user mod is untouched.
    assert_eq!(plan.to_remove.len(), 1);
    assert_eq!(plan.to_remove[0].file_name, "sodium.jar");

    // User mod is in merged with from_pack=false.
    let kept = plan.merged.iter().find(|m| m.file_name == "my-mod.jar");
    assert!(kept.is_some(), "user mod must be in merged");
    assert!(
        !kept.unwrap().from_pack,
        "user mod must remain from_pack=false"
    );
}

/// When a user mod and a new pack mod share the same file_name, the pack entry
/// replaces the user entry — merged has exactly ONE record, from_pack=true.
#[test]
fn d3_same_filename_user_entry_replaced_by_pack() {
    let current = vec![
        mod_entry("sodium.jar", false), // user-added variant
    ];
    let new_pack = vec![
        mod_entry("sodium.jar", true), // pack supersedes by same file_name
    ];

    let plan = plan_pack_update(&current, &new_pack);

    assert!(
        plan.to_remove.is_empty(),
        "user mod must never be in to_remove"
    );

    // Exactly one record for sodium.jar in merged.
    let sodium_entries: Vec<_> = plan
        .merged
        .iter()
        .filter(|m| m.file_name == "sodium.jar")
        .collect();
    assert_eq!(
        sodium_entries.len(),
        1,
        "must be exactly one entry for sodium.jar"
    );
    assert!(
        sodium_entries[0].from_pack,
        "the pack entry must win (from_pack=true)"
    );
}

/// An unchanged pack mod (present in both current and new plan) stays in merged.
#[test]
fn d3_unchanged_pack_mod_stays_in_merged() {
    let current = vec![
        mod_entry("sodium.jar", true), // pack mod, unchanged
    ];
    let new_pack = vec![
        mod_entry("sodium.jar", true), // same file in new plan
    ];

    let plan = plan_pack_update(&current, &new_pack);

    assert!(
        plan.to_remove.is_empty(),
        "unchanged mod must not be removed"
    );
    assert_eq!(plan.merged.len(), 1);
    assert_eq!(plan.merged[0].file_name, "sodium.jar");
    assert!(plan.merged[0].from_pack);
}

/// Mixed scenario: user mod kept, pack mod vanished, pack mod unchanged, new pack mod added.
/// Verifies `merged` contains exactly the right set.
// ── CP-3: stage_and_promote ──────────────────────────────────────────────────

/// `remap_to_staging` replaces the `target_dir` prefix with `staging_dir` for
/// items under target, and leaves other items unchanged.
#[test]
fn cp3_remap_to_staging_replaces_target_prefix() {
    use crate::core::download::{DownloadItem, ExpectedHash};
    let target = std::path::Path::new("/instance/mc");
    let staging = std::path::Path::new("/instance/.staging-1");

    let items = vec![
        // Instance-bound (under target) — should be remapped.
        DownloadItem {
            url: "https://cdn.modrinth.com/sodium.jar".into(),
            dest: target.join("mods/sodium.jar"),
            expected_hash: Some(ExpectedHash::Sha1("abc".into())),
            size: None,
        },
        // Shared cache item (NOT under target) — unchanged.
        DownloadItem {
            url: "https://launcher.mojang.com/lib.jar".into(),
            dest: std::path::PathBuf::from("/cache/libraries/lib.jar"),
            expected_hash: None,
            size: None,
        },
    ];

    let remapped = remap_to_staging(items, target, staging);
    assert_eq!(
        remapped[0].dest,
        staging.join("mods/sodium.jar"),
        "instance-bound item remapped to staging"
    );
    assert_eq!(
        remapped[1].dest,
        std::path::PathBuf::from("/cache/libraries/lib.jar"),
        "cache item unchanged"
    );
}

/// `promote_staging` moves all files from staging into target and leaves
/// staging empty.
#[test]
fn cp3_promote_staging_moves_files_to_target() {
    let tmp = TempDir::new().unwrap();
    let staging = tmp.path().join("staging");
    let target = tmp.path().join("target");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::create_dir_all(&target).unwrap();

    // Create a nested file structure in staging.
    std::fs::create_dir_all(staging.join("mods")).unwrap();
    std::fs::write(staging.join("mods/sodium.jar"), b"sodium").unwrap();
    std::fs::write(staging.join("mods/lithium.jar"), b"lithium").unwrap();
    std::fs::create_dir_all(staging.join("config")).unwrap();
    std::fs::write(staging.join("config/settings.cfg"), b"cfg").unwrap();

    promote_staging(&staging, &target).unwrap();

    assert!(
        target.join("mods/sodium.jar").exists(),
        "sodium.jar promoted"
    );
    assert!(
        target.join("mods/lithium.jar").exists(),
        "lithium.jar promoted"
    );
    assert!(
        target.join("config/settings.cfg").exists(),
        "config promoted"
    );
    // Staging files are gone (renamed, not copied).
    assert!(
        !staging.join("mods/sodium.jar").exists(),
        "staging file removed after rename"
    );
}

/// `promote_staging` on an empty staging dir is a no-op (no error).
#[test]
fn cp3_promote_staging_empty_dir_is_noop() {
    let tmp = TempDir::new().unwrap();
    let staging = tmp.path().join("staging");
    let target = tmp.path().join("target");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::create_dir_all(&target).unwrap();

    // Should succeed without touching target.
    promote_staging(&staging, &target).unwrap();
    assert_eq!(
        std::fs::read_dir(&target).unwrap().count(),
        0,
        "target unchanged"
    );
}

/// Cancellation path: discarding the staging dir leaves the target (instance)
/// tree unmodified.
#[test]
fn cp3_discard_staging_leaves_target_clean() {
    let tmp = TempDir::new().unwrap();
    let staging = tmp.path().join("staging");
    let target = tmp.path().join("target");
    std::fs::create_dir_all(staging.join("mods")).unwrap();
    std::fs::write(staging.join("mods/badmod.jar"), b"partial").unwrap();
    std::fs::create_dir_all(&target).unwrap();
    // Target has a pre-existing file unrelated to the download.
    std::fs::write(target.join("existing.txt"), b"keep").unwrap();

    // Cancel: discard staging, never promote.
    std::fs::remove_dir_all(&staging).unwrap();

    // Target is completely unmodified.
    assert!(!target.join("mods/badmod.jar").exists(), "not promoted");
    assert!(target.join("existing.txt").exists(), "unrelated file intact");
}

#[test]
fn d3_mixed_scenario_merged_is_correct() {
    let current = vec![
        mod_entry("user-mod.jar", false), // user — not overwritten
        mod_entry("sodium.jar", true),    // pack — vanished in new plan
        mod_entry("lithium.jar", true),   // pack — stays unchanged
    ];
    let new_pack = vec![
        mod_entry("lithium.jar", true), // unchanged
        mod_entry("indium.jar", true),  // new addition in new pack version
    ];

    let plan = plan_pack_update(&current, &new_pack);

    // to_remove: sodium vanished
    assert_eq!(plan.to_remove.len(), 1);
    assert_eq!(plan.to_remove[0].file_name, "sodium.jar");

    // merged: user-mod + lithium + indium (3 entries)
    let merged_names: Vec<_> = plan.merged.iter().map(|m| m.file_name.as_str()).collect();
    assert!(
        merged_names.contains(&"user-mod.jar"),
        "user mod must survive"
    );
    assert!(
        merged_names.contains(&"lithium.jar"),
        "unchanged pack mod must survive"
    );
    assert!(
        merged_names.contains(&"indium.jar"),
        "new pack mod must be in merged"
    );
    assert!(
        !merged_names.contains(&"sodium.jar"),
        "vanished pack mod must NOT be in merged"
    );
    assert_eq!(plan.merged.len(), 3, "merged must have exactly 3 entries");
}
