//! Unit tests for `instances`. Extracted from the source module; wired back
//! via `#[cfg(test)] #[path = "instances_tests.rs"] mod tests;` so private items
//! remain accessible through `super::*`.

use super::*;
use std::collections::BTreeMap;
use tempfile::TempDir;

// -----------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------

fn stub_instance(mods: Vec<ModEntry>) -> Instance {
    Instance {
        schema: SCHEMA_VERSION,
        id: "test-id".into(),
        name: "Test".into(),
        slug: "test".into(),
        icon: None,
        minecraft: "1.20.1".into(),
        loader: Loader {
            kind: "vanilla".into(),
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
        mods,
        created: "2024-01-01T00:00:00Z".into(),
        last_played: None,
        total_playtime_sec: 0,
    }
}

fn stub_mod_entry(file_name: &str, enabled: bool) -> ModEntry {
    ModEntry {
        provider: "modrinth".into(),
        project_id: "proj-1".into(),
        version_id: "ver-1".into(),
        file_name: file_name.to_string(),
        hashes: BTreeMap::new(),
        enabled,
        side: "unknown".into(),
        from_pack: false,
    }
}

/// Write a manifest and return the path.
fn write_inst(dir: &Path, inst: &Instance) -> std::path::PathBuf {
    let p = dir.join("instance.json");
    let raw = serde_json::to_string_pretty(inst).unwrap();
    fs::write(&p, raw).unwrap();
    p
}

// -----------------------------------------------------------------------
// validate_mod_file_name
// -----------------------------------------------------------------------

#[test]
fn validate_mod_file_name_accepts_normal_jar() {
    assert!(validate_mod_file_name("sodium-0.5.jar").is_ok());
    assert!(validate_mod_file_name("Mod_Name-1.0.jar").is_ok());
}

#[test]
fn validate_mod_file_name_rejects_non_jar() {
    assert!(validate_mod_file_name("mod.zip").is_err());
    assert!(validate_mod_file_name("mod").is_err());
    assert!(validate_mod_file_name("").is_err());
}

#[test]
fn validate_mod_file_name_rejects_slash() {
    assert!(validate_mod_file_name("subdir/mod.jar").is_err());
}

#[test]
fn validate_mod_file_name_rejects_backslash() {
    assert!(validate_mod_file_name("subdir\\mod.jar").is_err());
}

#[test]
fn validate_mod_file_name_rejects_dotdot() {
    assert!(validate_mod_file_name("../../etc/passwd.jar").is_err());
    assert!(validate_mod_file_name("..").is_err());
}

#[test]
fn validate_mod_file_name_rejects_absolute() {
    assert!(validate_mod_file_name("/absolute/path.jar").is_err());
}

#[test]
fn validate_mod_file_name_rejects_windows_drive_prefix() {
    assert!(validate_mod_file_name("C:mod.jar").is_err());
    assert!(validate_mod_file_name("c:evil.jar").is_err());
    assert!(validate_mod_file_name("Z:whatever.jar").is_err());
}

// -----------------------------------------------------------------------
// set_mod_enabled_on_disk — disable
// -----------------------------------------------------------------------

#[test]
fn disable_renames_jar_to_disabled_and_flips_flag() {
    let tmp = TempDir::new().unwrap();
    let mods_dir = tmp.path().join("mods");
    fs::create_dir_all(&mods_dir).unwrap();

    // Place the enabled jar
    fs::write(mods_dir.join("sodium.jar"), b"fake").unwrap();

    let inst = stub_instance(vec![stub_mod_entry("sodium.jar", true)]);
    let manifest = write_inst(tmp.path(), &inst);

    set_mod_enabled_on_disk(&mods_dir, &manifest, "sodium.jar", false).unwrap();

    assert!(
        !mods_dir.join("sodium.jar").exists(),
        "enabled form should be gone"
    );
    assert!(
        mods_dir.join("sodium.jar.disabled").exists(),
        "disabled form should exist"
    );

    let saved = read_manifest(&manifest).unwrap();
    assert_eq!(saved.mods[0].enabled, false);
}

// -----------------------------------------------------------------------
// set_mod_enabled_on_disk — enable
// -----------------------------------------------------------------------

#[test]
fn enable_renames_disabled_to_jar_and_flips_flag() {
    let tmp = TempDir::new().unwrap();
    let mods_dir = tmp.path().join("mods");
    fs::create_dir_all(&mods_dir).unwrap();

    // Place the disabled jar
    fs::write(mods_dir.join("sodium.jar.disabled"), b"fake").unwrap();

    let inst = stub_instance(vec![stub_mod_entry("sodium.jar", false)]);
    let manifest = write_inst(tmp.path(), &inst);

    set_mod_enabled_on_disk(&mods_dir, &manifest, "sodium.jar", true).unwrap();

    assert!(
        mods_dir.join("sodium.jar").exists(),
        "enabled form should exist"
    );
    assert!(
        !mods_dir.join("sodium.jar.disabled").exists(),
        "disabled form should be gone"
    );

    let saved = read_manifest(&manifest).unwrap();
    assert_eq!(saved.mods[0].enabled, true);
}

// -----------------------------------------------------------------------
// set_mod_enabled_on_disk — idempotency
// -----------------------------------------------------------------------

#[test]
fn disable_already_disabled_is_noop() {
    let tmp = TempDir::new().unwrap();
    let mods_dir = tmp.path().join("mods");
    fs::create_dir_all(&mods_dir).unwrap();

    fs::write(mods_dir.join("sodium.jar.disabled"), b"fake").unwrap();

    let inst = stub_instance(vec![stub_mod_entry("sodium.jar", false)]);
    let manifest = write_inst(tmp.path(), &inst);

    // Should succeed without touching anything
    set_mod_enabled_on_disk(&mods_dir, &manifest, "sodium.jar", false).unwrap();

    assert!(mods_dir.join("sodium.jar.disabled").exists());
    assert!(!mods_dir.join("sodium.jar").exists());
}

#[test]
fn enable_already_enabled_is_noop() {
    let tmp = TempDir::new().unwrap();
    let mods_dir = tmp.path().join("mods");
    fs::create_dir_all(&mods_dir).unwrap();

    fs::write(mods_dir.join("sodium.jar"), b"fake").unwrap();

    let inst = stub_instance(vec![stub_mod_entry("sodium.jar", true)]);
    let manifest = write_inst(tmp.path(), &inst);

    set_mod_enabled_on_disk(&mods_dir, &manifest, "sodium.jar", true).unwrap();

    assert!(mods_dir.join("sodium.jar").exists());
    assert!(!mods_dir.join("sodium.jar.disabled").exists());
}

// -----------------------------------------------------------------------
// remove_mod_from_disk — enabled file
// -----------------------------------------------------------------------

#[test]
fn remove_deletes_enabled_jar_and_drops_entry() {
    let tmp = TempDir::new().unwrap();
    let mods_dir = tmp.path().join("mods");
    fs::create_dir_all(&mods_dir).unwrap();

    fs::write(mods_dir.join("sodium.jar"), b"fake").unwrap();

    let inst = stub_instance(vec![stub_mod_entry("sodium.jar", true)]);
    let manifest = write_inst(tmp.path(), &inst);

    remove_mod_from_disk(&mods_dir, &manifest, "sodium.jar").unwrap();

    assert!(!mods_dir.join("sodium.jar").exists());
    let saved = read_manifest(&manifest).unwrap();
    assert!(saved.mods.is_empty());
}

// -----------------------------------------------------------------------
// remove_mod_from_disk — disabled file
// -----------------------------------------------------------------------

#[test]
fn remove_deletes_disabled_jar_and_drops_entry() {
    let tmp = TempDir::new().unwrap();
    let mods_dir = tmp.path().join("mods");
    fs::create_dir_all(&mods_dir).unwrap();

    fs::write(mods_dir.join("sodium.jar.disabled"), b"fake").unwrap();

    let inst = stub_instance(vec![stub_mod_entry("sodium.jar", false)]);
    let manifest = write_inst(tmp.path(), &inst);

    remove_mod_from_disk(&mods_dir, &manifest, "sodium.jar").unwrap();

    assert!(!mods_dir.join("sodium.jar.disabled").exists());
    let saved = read_manifest(&manifest).unwrap();
    assert!(saved.mods.is_empty());
}

// -----------------------------------------------------------------------
// remove_mod_from_disk — missing file still drops entry
// -----------------------------------------------------------------------

#[test]
fn remove_missing_file_still_drops_entry() {
    let tmp = TempDir::new().unwrap();
    let mods_dir = tmp.path().join("mods");
    fs::create_dir_all(&mods_dir).unwrap();

    // No file on disk
    let inst = stub_instance(vec![stub_mod_entry("sodium.jar", true)]);
    let manifest = write_inst(tmp.path(), &inst);

    remove_mod_from_disk(&mods_dir, &manifest, "sodium.jar").unwrap();

    let saved = read_manifest(&manifest).unwrap();
    assert!(
        saved.mods.is_empty(),
        "entry must be dropped even when file is absent"
    );
}

// -----------------------------------------------------------------------
// D4: ensure_not_locked (pure guard)
// -----------------------------------------------------------------------

#[test]
fn ensure_not_locked_ok_when_unlocked() {
    let inst = stub_instance(vec![]);
    // pack_locked defaults to false in stub_instance
    assert!(ensure_not_locked(&inst).is_ok());
}

#[test]
fn ensure_not_locked_err_when_locked() {
    let mut inst = stub_instance(vec![]);
    inst.pack_locked = true;
    let result = ensure_not_locked(&inst);
    assert!(result.is_err());
    let msg = result.unwrap_err();
    assert!(
        msg.contains("pack-locked"),
        "error message must mention 'pack-locked': {msg}"
    );
}

// -----------------------------------------------------------------------
// D4: set_pack_lock persistence
// -----------------------------------------------------------------------

#[test]
fn set_pack_lock_persists_true_then_false() {
    let tmp = TempDir::new().unwrap();
    // Write a minimal instance.json in a slug directory (slug = "test")
    let slug_dir = tmp.path().join("test");
    fs::create_dir_all(&slug_dir).unwrap();
    let manifest_path = slug_dir.join("instance.json");

    let inst = stub_instance(vec![]);
    let raw = serde_json::to_string_pretty(&inst).unwrap();
    fs::write(&manifest_path, raw).unwrap();

    // Lock it via the disk helper
    set_pack_lock_on_disk(&manifest_path, true).unwrap();
    let saved = read_manifest(&manifest_path).unwrap();
    assert!(saved.pack_locked, "pack_locked must be true after locking");

    // Unlock it
    set_pack_lock_on_disk(&manifest_path, false).unwrap();
    let saved2 = read_manifest(&manifest_path).unwrap();
    assert!(!saved2.pack_locked, "pack_locked must be false after unlocking");
}

// -----------------------------------------------------------------------
// D1: backward-compat deserialization + new fields
// -----------------------------------------------------------------------

/// An old `instance.json` that lacks both `fromPack` (on mod entries) and
/// `packLocked` (on the instance) must deserialize with both defaulting to `false`.
/// This guards against a schema bump that would break existing installations.
#[test]
fn d1_old_manifest_deserializes_with_default_false_fields() {
    let json = r#"{
        "schema": 1,
        "id": "old-id",
        "name": "Old Instance",
        "slug": "old-instance",
        "icon": null,
        "minecraft": "1.20.1",
        "loader": { "kind": "vanilla", "version": null },
        "java": { "major": null, "argsOverride": null, "memoryMb": 2048 },
        "source": null,
        "mods": [
            {
                "provider": "modrinth",
                "projectId": "proj-1",
                "versionId": "ver-1",
                "fileName": "sodium.jar",
                "hashes": {},
                "enabled": true,
                "side": "client"
            }
        ],
        "created": "2024-01-01T00:00:00Z",
        "lastPlayed": null,
        "totalPlaytimeSec": 0
    }"#;

    let inst: Instance = serde_json::from_str(json).expect("old manifest must deserialize");

    assert!(
        !inst.pack_locked,
        "packLocked must default to false when absent from old manifest"
    );
    assert_eq!(inst.mods.len(), 1);
    assert!(
        !inst.mods[0].from_pack,
        "fromPack must default to false when absent from old manifest"
    );
}

// -----------------------------------------------------------------------
// A-1: old manifest round-trip (new JavaCfg + Source fields backward compat)
// -----------------------------------------------------------------------

/// An old `instance.json` that lacks the new `JavaCfg` fields (`minMemoryMb`,
/// `pathOverride`, `usePackSettings`) and the new `Source.recommended` field must
/// deserialize with all new fields at their zero/None defaults. Confirms no schema
/// bump is needed (A-1 spec).
#[test]
fn a1_old_manifest_new_java_cfg_fields_default() {
    let json = r#"{
        "schema": 1,
        "id": "old-id-2",
        "name": "Old Instance",
        "slug": "old-instance-2",
        "icon": null,
        "minecraft": "1.20.1",
        "loader": { "kind": "vanilla", "version": null },
        "java": { "major": null, "argsOverride": null, "memoryMb": 2048 },
        "source": null,
        "mods": [],
        "created": "2024-01-01T00:00:00Z",
        "lastPlayed": null,
        "totalPlaytimeSec": 0
    }"#;

    let inst: Instance = serde_json::from_str(json).expect("old manifest must deserialize");

    assert_eq!(
        inst.java.min_memory_mb, None,
        "minMemoryMb must default to None when absent"
    );
    assert_eq!(
        inst.java.path_override, None,
        "pathOverride must default to None when absent"
    );
    assert!(
        !inst.java.use_pack_settings,
        "usePackSettings must default to false when absent"
    );
    assert!(
        inst.source.is_none(),
        "source must remain None (no recommended field involved)"
    );
}

// -----------------------------------------------------------------------
// D-3: set_instance_java round-trip
// -----------------------------------------------------------------------

/// `set_instance_java_on_disk` persists every field of `JavaCfg` and they
/// survive a manifest round-trip (read back from disk with the new values).
#[test]
fn set_instance_java_round_trips_on_disk() {
    let tmp = TempDir::new().unwrap();

    // Write a baseline manifest with default java settings.
    let inst = stub_instance(vec![]);
    let manifest_path = write_inst(tmp.path(), &inst);

    let new_java = JavaCfg {
        major: Some(21),
        args_override: Some("-XX:+UseG1GC".to_string()),
        memory_mb: 4096,
        min_memory_mb: Some(512),
        path_override: Some("/usr/lib/jvm/java-21/bin/java".to_string()),
        use_pack_settings: true,
    };

    set_instance_java_on_disk(&manifest_path, new_java.clone()).unwrap();

    let reloaded = read_manifest_pub(&manifest_path).unwrap();
    assert_eq!(reloaded.java.major, Some(21));
    assert_eq!(reloaded.java.args_override.as_deref(), Some("-XX:+UseG1GC"));
    assert_eq!(reloaded.java.memory_mb, 4096);
    assert_eq!(reloaded.java.min_memory_mb, Some(512));
    assert_eq!(
        reloaded.java.path_override.as_deref(),
        Some("/usr/lib/jvm/java-21/bin/java")
    );
    assert!(reloaded.java.use_pack_settings);
}

/// Verify that setting `min_memory_mb` to `None` and `path_override` to `None`
/// also persists correctly (the "clear" case).
#[test]
fn set_instance_java_clears_optional_fields() {
    let tmp = TempDir::new().unwrap();
    // Start with all fields set.
    let mut inst = stub_instance(vec![]);
    inst.java = JavaCfg {
        major: Some(17),
        args_override: Some("-Xmx4g".to_string()),
        memory_mb: 4096,
        min_memory_mb: Some(1024),
        path_override: Some("/some/path/java".to_string()),
        use_pack_settings: true,
    };
    let manifest_path = write_inst(tmp.path(), &inst);

    // Clear the optional fields.
    let cleared = JavaCfg {
        major: None,
        args_override: None,
        memory_mb: 2048,
        min_memory_mb: None,
        path_override: None,
        use_pack_settings: false,
    };
    set_instance_java_on_disk(&manifest_path, cleared).unwrap();

    let reloaded = read_manifest_pub(&manifest_path).unwrap();
    assert_eq!(reloaded.java.major, None);
    assert_eq!(reloaded.java.args_override, None);
    assert_eq!(reloaded.java.memory_mb, 2048);
    assert_eq!(reloaded.java.min_memory_mb, None);
    assert_eq!(reloaded.java.path_override, None);
    assert!(!reloaded.java.use_pack_settings);
}

/// Old manifest with a `source` object that lacks `recommended` must deserialize
/// with `recommended == None`.
#[test]
fn a1_old_source_without_recommended_defaults_to_none() {
    let json = r#"{
        "schema": 1,
        "id": "old-id-3",
        "name": "Old Pack",
        "slug": "old-pack",
        "icon": null,
        "minecraft": "1.20.1",
        "loader": { "kind": "fabric", "version": "0.15.0" },
        "java": { "major": null, "argsOverride": null, "memoryMb": 4096 },
        "source": {
            "provider": "modrinth",
            "projectId": "proj-abc",
            "fileId": "file-xyz",
            "packVersion": "1.0.0"
        },
        "mods": [],
        "created": "2024-01-01T00:00:00Z",
        "lastPlayed": null,
        "totalPlaytimeSec": 0
    }"#;

    let inst: Instance = serde_json::from_str(json).expect("old manifest with source must deserialize");

    let source = inst.source.expect("source must be Some");
    assert_eq!(
        source.recommended, None,
        "recommended must default to None when absent from old source"
    );
}
