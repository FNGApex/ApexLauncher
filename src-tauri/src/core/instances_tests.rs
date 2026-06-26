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
        pending_manual: vec![],
        suppress_pending_launch_warning: false,
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
        name: None,
        icon_url: None,
        summary: None,
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

// -----------------------------------------------------------------------
// AM-B1: ModEntry old-manifest round-trip (new metadata fields default to None)
// -----------------------------------------------------------------------

/// An old `instance.json` whose mod entries lack `name`, `iconUrl`, and `summary`
/// must deserialize with all three new fields defaulting to `None`.
/// Guards backward-compat: no schema bump, no migration needed.
#[test]
fn amb1_old_mod_entry_without_metadata_defaults_to_none() {
    let json = r#"{
        "schema": 1,
        "id": "old-id-4",
        "name": "Old Instance",
        "slug": "old-instance-4",
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
                "side": "client",
                "fromPack": false
            }
        ],
        "created": "2024-01-01T00:00:00Z",
        "lastPlayed": null,
        "totalPlaytimeSec": 0
    }"#;

    let inst: Instance = serde_json::from_str(json).expect("old manifest must deserialize");

    assert_eq!(inst.mods.len(), 1);
    let m = &inst.mods[0];
    assert_eq!(m.name, None, "name must default to None when absent from old ModEntry");
    assert_eq!(m.icon_url, None, "iconUrl must default to None when absent from old ModEntry");
    assert_eq!(m.summary, None, "summary must default to None when absent from old ModEntry");
}

/// Round-trip: a ModEntry that HAS metadata fields serialises them and they survive
/// a write-then-read cycle.
#[test]
fn amb1_mod_entry_with_metadata_survives_round_trip() {
    let tmp = TempDir::new().unwrap();

    let entry = ModEntry {
        provider: "modrinth".into(),
        project_id: "proj-rt".into(),
        version_id: "ver-rt".into(),
        file_name: "sodium.jar".into(),
        hashes: BTreeMap::new(),
        enabled: true,
        side: "client".into(),
        from_pack: false,
        name: Some("Sodium".into()),
        icon_url: Some("https://cdn.modrinth.com/sodium.png".into()),
        summary: Some("A rendering optimisation mod".into()),
    };

    let inst = stub_instance(vec![entry]);
    let manifest_path = write_inst(tmp.path(), &inst);
    let reloaded = read_manifest_pub(&manifest_path).unwrap();

    let m = &reloaded.mods[0];
    assert_eq!(m.name.as_deref(), Some("Sodium"));
    assert_eq!(m.icon_url.as_deref(), Some("https://cdn.modrinth.com/sodium.png"));
    assert_eq!(m.summary.as_deref(), Some("A rendering optimisation mod"));
}

// -----------------------------------------------------------------------
// AM-B3: Source.page_url old-manifest round-trip
// -----------------------------------------------------------------------

/// An old `instance.json` whose source lacks `pageUrl` must deserialize with
/// `page_url == None`. Guards backward-compat.
#[test]
fn amb3_old_source_without_page_url_defaults_to_none() {
    let json = r#"{
        "schema": 1,
        "id": "old-id-5",
        "name": "Old Pack",
        "slug": "old-pack-5",
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
        source.page_url, None,
        "pageUrl must default to None when absent from old source"
    );
}

/// Round-trip: a Source that HAS page_url serialises it and it survives a
/// write-then-read cycle.
#[test]
fn amb3_source_with_page_url_survives_round_trip() {
    let tmp = TempDir::new().unwrap();

    let mut inst = stub_instance(vec![]);
    inst.source = Some(Source {
        provider: "modrinth".into(),
        project_id: "proj-rt2".into(),
        file_id: "file-rt2".into(),
        pack_version: "2.0.0".into(),
        recommended: None,
        page_url: Some("https://modrinth.com/modpack/mypack".into()),
        icon_url: None,
        author: None,
        last_update_check: None,
        latest_version: None,
        latest_version_id: None,
        summary: None,
        categories: vec![],
    });

    let manifest_path = write_inst(tmp.path(), &inst);
    let reloaded = read_manifest_pub(&manifest_path).unwrap();

    let source = reloaded.source.expect("source must be Some");
    assert_eq!(
        source.page_url.as_deref(),
        Some("https://modrinth.com/modpack/mypack"),
        "pageUrl must survive a manifest round-trip"
    );
}

// ---------------------------------------------------------------------------
// PB-B1: needs_update_check tests
// ---------------------------------------------------------------------------

#[test]
fn pbb1_needs_update_check_none_returns_true() {
    let now = chrono::Utc::now();
    assert!(
        needs_update_check(None, now),
        "None last_update_check must return true"
    );
}

#[test]
fn pbb1_needs_update_check_1h_ago_returns_false() {
    let now = chrono::Utc::now();
    let one_hour_ago = now - chrono::Duration::hours(1);
    let last = one_hour_ago.to_rfc3339();
    assert!(
        !needs_update_check(Some(&last), now),
        "1h ago must return false (within 24h window)"
    );
}

#[test]
fn pbb1_needs_update_check_25h_ago_returns_true() {
    let now = chrono::Utc::now();
    let twenty_five_hours_ago = now - chrono::Duration::hours(25);
    let last = twenty_five_hours_ago.to_rfc3339();
    assert!(
        needs_update_check(Some(&last), now),
        "25h ago must return true (>24h stale)"
    );
}

#[test]
fn pbb1_needs_update_check_garbage_returns_true() {
    let now = chrono::Utc::now();
    assert!(
        needs_update_check(Some("not-a-timestamp"), now),
        "unparseable timestamp must return true"
    );
}

/// Round-trip: all five new Source fields survive JSON serialization.
#[test]
fn pbb1_new_source_fields_round_trip() {
    let tmp = TempDir::new().unwrap();

    let mut inst = stub_instance(vec![]);
    inst.source = Some(Source {
        provider: "curseforge".into(),
        project_id: "12345".into(),
        file_id: "67890".into(),
        pack_version: "1.5.0".into(),
        recommended: None,
        page_url: None,
        icon_url: Some("https://example.com/icon.png".into()),
        author: Some("TestAuthor".into()),
        last_update_check: Some("2026-06-20T00:00:00Z".into()),
        latest_version: Some("1.6.0".into()),
        latest_version_id: Some("99999".into()),
        summary: None,
        categories: vec![],
    });

    let manifest_path = write_inst(tmp.path(), &inst);
    let reloaded = read_manifest_pub(&manifest_path).unwrap();
    let src = reloaded.source.unwrap();

    assert_eq!(src.icon_url.as_deref(), Some("https://example.com/icon.png"));
    assert_eq!(src.author.as_deref(), Some("TestAuthor"));
    assert_eq!(src.last_update_check.as_deref(), Some("2026-06-20T00:00:00Z"));
    assert_eq!(src.latest_version.as_deref(), Some("1.6.0"));
    assert_eq!(src.latest_version_id.as_deref(), Some("99999"));
}

/// Old manifests without the new fields deserialize with None (backward compat).
#[test]
fn pbb1_old_manifest_missing_new_fields_defaults_to_none() {
    let tmp = TempDir::new().unwrap();
    // Write a manifest that lacks the five new fields (old shape).
    let old_json = r#"{
        "schema": 1,
        "id": "test-id",
        "name": "Test",
        "slug": "test",
        "icon": null,
        "minecraft": "1.20.1",
        "loader": { "kind": "vanilla", "version": null },
        "java": { "major": null, "argsOverride": null, "memoryMb": 2048 },
        "source": {
            "provider": "modrinth",
            "projectId": "proj-old",
            "fileId": "file-old",
            "packVersion": "1.0"
        },
        "packLocked": false,
        "mods": [],
        "created": "2024-01-01T00:00:00Z",
        "lastPlayed": null,
        "totalPlaytimeSec": 0
    }"#;
    let path = tmp.path().join("instance.json");
    std::fs::write(&path, old_json).unwrap();
    let inst = read_manifest_pub(&path).unwrap();
    let src = inst.source.unwrap();

    assert!(src.icon_url.is_none(), "icon_url must default to None");
    assert!(src.author.is_none(), "author must default to None");
    assert!(src.last_update_check.is_none(), "last_update_check must default to None");
    assert!(src.latest_version.is_none(), "latest_version must default to None");
    assert!(src.latest_version_id.is_none(), "latest_version_id must default to None");
}

// ---------------------------------------------------------------------------
// CP3: summary + categories on Source
// ---------------------------------------------------------------------------

/// CP3: `summary` and `categories` survive a manifest round-trip.
#[test]
fn cp3_source_summary_and_categories_round_trip() {
    let tmp = TempDir::new().unwrap();

    let mut inst = stub_instance(vec![]);
    inst.source = Some(Source {
        provider: "modrinth".into(),
        project_id: "AANobbMI".into(),
        file_id: "mc1.21-0.5.11".into(),
        pack_version: "0.5.11".into(),
        recommended: None,
        page_url: None,
        icon_url: None,
        author: None,
        last_update_check: None,
        latest_version: None,
        latest_version_id: None,
        summary: Some("A modern rendering engine for Minecraft.".into()),
        categories: vec!["optimization".into(), "utility".into()],
    });

    let manifest_path = write_inst(tmp.path(), &inst);
    let reloaded = read_manifest_pub(&manifest_path).unwrap();
    let src = reloaded.source.unwrap();

    assert_eq!(
        src.summary.as_deref(),
        Some("A modern rendering engine for Minecraft."),
        "summary must survive round-trip"
    );
    assert_eq!(
        src.categories,
        vec!["optimization".to_string(), "utility".to_string()],
        "categories must survive round-trip"
    );
}

/// CP3: old manifests without `summary`/`categories` fields deserialize with
/// `None`/`[]` respectively (backward-compat via `#[serde(default)]`).
#[test]
fn cp3_old_manifest_missing_summary_categories_defaults() {
    let tmp = TempDir::new().unwrap();
    let old_json = r#"{
        "schema": 1,
        "id": "test-id",
        "name": "Test",
        "slug": "test",
        "icon": null,
        "minecraft": "1.20.1",
        "loader": { "kind": "vanilla", "version": null },
        "java": { "major": null, "argsOverride": null, "memoryMb": 2048 },
        "source": {
            "provider": "modrinth",
            "projectId": "proj-old",
            "fileId": "file-old",
            "packVersion": "1.0"
        },
        "packLocked": false,
        "mods": [],
        "created": "2024-01-01T00:00:00Z",
        "lastPlayed": null,
        "totalPlaytimeSec": 0
    }"#;
    let path = tmp.path().join("instance.json");
    std::fs::write(&path, old_json).unwrap();
    let inst = read_manifest_pub(&path).unwrap();
    let src = inst.source.unwrap();

    assert!(src.summary.is_none(), "summary must default to None for old manifests");
    assert!(src.categories.is_empty(), "categories must default to [] for old manifests");
}

// -----------------------------------------------------------------------
// CP-2: pending_manual / suppress_pending_launch_warning back-compat
// -----------------------------------------------------------------------

/// An old `instance.json` with neither `pendingManual` nor
/// `suppressPendingLaunchWarning` must deserialize to `[]` / `false` (serde
/// defaults, no schema bump).
#[test]
fn cp2_old_manifest_missing_pending_fields_defaults() {
    let tmp = TempDir::new().unwrap();
    let old_json = r#"{
        "schema": 1,
        "id": "test-id",
        "name": "Test",
        "slug": "test",
        "icon": null,
        "minecraft": "1.20.1",
        "loader": { "kind": "vanilla", "version": null },
        "java": { "major": null, "argsOverride": null, "memoryMb": 2048 },
        "source": null,
        "packLocked": false,
        "mods": [],
        "created": "2024-01-01T00:00:00Z",
        "lastPlayed": null,
        "totalPlaytimeSec": 0
    }"#;
    let path = tmp.path().join("instance.json");
    std::fs::write(&path, old_json).unwrap();
    let inst = read_manifest_pub(&path).unwrap();

    assert!(
        inst.pending_manual.is_empty(),
        "pendingManual must default to [] for old manifests"
    );
    assert!(
        !inst.suppress_pending_launch_warning,
        "suppressPendingLaunchWarning must default to false for old manifests"
    );
}

/// A populated `pending_manual` plus `suppress_pending_launch_warning = true`
/// survives a write→read round-trip.
#[test]
fn cp2_pending_manual_round_trips() {
    let tmp = TempDir::new().unwrap();
    let mut inst = stub_instance(vec![]);
    inst.pending_manual = vec![PendingManual {
        project_id: "238222".into(),
        file_id: "4536804".into(),
        file_name: "jei-1.20.1.jar".into(),
        page_url: "https://www.curseforge.com/minecraft/mc-mods/jei/files/4536804".into(),
        expected_sha1: Some("aabbcc".into()),
        size: Some(1024),
    }];
    inst.suppress_pending_launch_warning = true;

    let path = write_inst(tmp.path(), &inst);
    let reloaded = read_manifest_pub(&path).unwrap();

    assert_eq!(reloaded.pending_manual.len(), 1);
    assert_eq!(reloaded.pending_manual[0].file_name, "jei-1.20.1.jar");
    assert_eq!(
        reloaded.pending_manual[0].expected_sha1.as_deref(),
        Some("aabbcc")
    );
    assert_eq!(reloaded.pending_manual[0].size, Some(1024));
    assert!(reloaded.suppress_pending_launch_warning);
}

// -----------------------------------------------------------------------
// CP-5: reconcile_pending_manual
// -----------------------------------------------------------------------

fn pending(file_name: &str, expected_sha1: Option<&str>) -> PendingManual {
    PendingManual {
        project_id: "238222".into(),
        file_id: "4536804".into(),
        file_name: file_name.to_string(),
        page_url: "https://www.curseforge.com/minecraft/mc-mods/jei/files/4536804".into(),
        expected_sha1: expected_sha1.map(|s| s.to_string()),
        size: None,
    }
}

/// Write `content` into `mods_dir/<name>` and return the file's hex SHA-1.
fn write_jar(mods_dir: &Path, name: &str, content: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    fs::write(mods_dir.join(name), content).unwrap();
    let mut h = Sha1::new();
    h.update(content);
    hex::encode(h.finalize())
}

#[test]
fn cp5_reconcile_exact_name_matching_sha1_resolves_enabled() {
    let tmp = TempDir::new().unwrap();
    let mods = tmp.path();
    let sha = write_jar(mods, "jei.jar", b"jar bytes");

    let mut inst = stub_instance(vec![]);
    inst.pending_manual = vec![pending("jei.jar", Some(&sha))];

    let resolved = reconcile_pending_manual(&mut inst, mods);

    assert_eq!(resolved.len(), 1);
    assert!(resolved[0].enabled);
    assert!(inst.pending_manual.is_empty(), "resolved entry removed");
    assert_eq!(inst.mods.len(), 1);
    assert_eq!(inst.mods[0].file_name, "jei.jar");
    assert!(inst.mods[0].enabled);
    assert_eq!(inst.mods[0].hashes.get("sha1"), Some(&sha));
    assert_eq!(inst.mods[0].provider, "curseforge");
    assert!(inst.mods[0].from_pack);
}

#[test]
fn cp5_reconcile_disabled_form_resolves_disabled() {
    let tmp = TempDir::new().unwrap();
    let mods = tmp.path();
    write_jar(mods, "jei.jar.disabled", b"jar bytes");

    let mut inst = stub_instance(vec![]);
    // name-only acceptance (no sha) so the .disabled form is accepted on name.
    inst.pending_manual = vec![pending("jei.jar", None)];

    let resolved = reconcile_pending_manual(&mut inst, mods);

    assert_eq!(resolved.len(), 1);
    assert!(!resolved[0].enabled, "disabled-form match → enabled=false");
    assert!(inst.pending_manual.is_empty());
    assert_eq!(inst.mods.len(), 1);
    assert!(!inst.mods[0].enabled);
}

#[test]
fn cp5_reconcile_exact_wins_over_disabled() {
    let tmp = TempDir::new().unwrap();
    let mods = tmp.path();
    write_jar(mods, "jei.jar", b"enabled bytes");
    write_jar(mods, "jei.jar.disabled", b"disabled bytes");

    let mut inst = stub_instance(vec![]);
    inst.pending_manual = vec![pending("jei.jar", None)];

    let resolved = reconcile_pending_manual(&mut inst, mods);

    assert_eq!(resolved.len(), 1);
    assert!(resolved[0].enabled, "exact name wins when both present");
}

#[test]
fn cp5_reconcile_wrong_sha1_not_resolved() {
    let tmp = TempDir::new().unwrap();
    let mods = tmp.path();
    write_jar(mods, "jei.jar", b"jar bytes");

    let mut inst = stub_instance(vec![]);
    inst.pending_manual = vec![pending("jei.jar", Some("deadbeefdeadbeef"))];

    let resolved = reconcile_pending_manual(&mut inst, mods);

    assert!(resolved.is_empty(), "hash mismatch → not resolved");
    assert_eq!(inst.pending_manual.len(), 1, "entry stays pending");
    assert!(inst.mods.is_empty());
}

#[test]
fn cp5_reconcile_name_only_accepts() {
    let tmp = TempDir::new().unwrap();
    let mods = tmp.path();
    write_jar(mods, "jei.jar", b"whatever");

    let mut inst = stub_instance(vec![]);
    inst.pending_manual = vec![pending("jei.jar", None)];

    let resolved = reconcile_pending_manual(&mut inst, mods);

    assert_eq!(resolved.len(), 1);
    assert!(inst.mods[0].hashes.get("sha1").is_none(), "no hash recorded for name-only");
}

#[test]
fn cp5_reconcile_missing_file_unchanged() {
    let tmp = TempDir::new().unwrap();
    let mut inst = stub_instance(vec![]);
    inst.pending_manual = vec![pending("absent.jar", None)];

    let resolved = reconcile_pending_manual(&mut inst, tmp.path());

    assert!(resolved.is_empty());
    assert_eq!(inst.pending_manual.len(), 1);
    assert!(inst.mods.is_empty());
}

#[test]
fn cp5_reconcile_idempotent_and_empties_when_all_resolved() {
    let tmp = TempDir::new().unwrap();
    let mods = tmp.path();
    write_jar(mods, "a.jar", b"a");
    write_jar(mods, "b.jar", b"b");

    let mut inst = stub_instance(vec![]);
    inst.pending_manual = vec![pending("a.jar", None), pending("b.jar", None)];

    let first = reconcile_pending_manual(&mut inst, mods);
    assert_eq!(first.len(), 2);
    assert!(inst.pending_manual.is_empty(), "list empties when all resolved");
    assert_eq!(inst.mods.len(), 2);

    // Second call is a no-op — nothing pending, no duplicate ModEntry.
    let second = reconcile_pending_manual(&mut inst, mods);
    assert!(second.is_empty());
    assert_eq!(inst.mods.len(), 2, "idempotent — no duplicate entries");
}

// -----------------------------------------------------------------------
// CP-6: manual file import (copy + reconcile)
// -----------------------------------------------------------------------

/// Copying a correctly-named jar into mods/ then reconciling resolves the entry
/// (the copy step is `import_manual_file`'s only side effect before reconcile).
#[test]
fn cp6_copy_named_jar_then_reconcile_resolves() {
    let tmp = TempDir::new().unwrap();
    let src_dir = tmp.path().join("downloads");
    let mods = tmp.path().join("mods");
    fs::create_dir_all(&src_dir).unwrap();
    fs::create_dir_all(&mods).unwrap();
    fs::write(src_dir.join("jei.jar"), b"jar bytes").unwrap();
    // import_manual_file copies the picked file under its own basename.
    fs::copy(src_dir.join("jei.jar"), mods.join("jei.jar")).unwrap();

    let mut inst = stub_instance(vec![]);
    inst.pending_manual = vec![pending("jei.jar", None)];
    let resolved = reconcile_pending_manual(&mut inst, &mods);

    assert_eq!(resolved.len(), 1);
    assert!(inst.pending_manual.is_empty());
    assert_eq!(inst.mods.len(), 1);
}

/// Copying an unrelated jar leaves the pending entry untouched.
#[test]
fn cp6_unrelated_jar_leaves_pending() {
    let tmp = TempDir::new().unwrap();
    let mods = tmp.path();
    fs::write(mods.join("some-other-mod.jar"), b"jar bytes").unwrap();

    let mut inst = stub_instance(vec![]);
    inst.pending_manual = vec![pending("jei.jar", None)];
    let resolved = reconcile_pending_manual(&mut inst, mods);

    assert!(resolved.is_empty());
    assert_eq!(inst.pending_manual.len(), 1);
    assert!(inst.mods.is_empty());
}

// -----------------------------------------------------------------------
// Instance icons: write_instance_icon / clear_instance_icon_file
// -----------------------------------------------------------------------

/// Count files in a dir whose name starts with `icon-`.
fn icon_files(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|n| n.starts_with("icon-"))
        .collect();
    v.sort();
    v
}

#[test]
fn icon_write_sets_field_and_copies_file() {
    let tmp = TempDir::new().unwrap();
    let inst_dir = tmp.path();
    let src = inst_dir.join("source.png");
    fs::write(&src, b"\x89PNG fake").unwrap();

    let mut inst = stub_instance(vec![]);
    write_instance_icon(inst_dir, &mut inst, &src).unwrap();

    let name = inst.icon.clone().expect("icon set");
    assert!(name.starts_with("icon-") && name.ends_with(".png"), "name: {name}");
    assert!(inst_dir.join(&name).is_file(), "icon file copied");
}

#[test]
fn icon_write_replaces_prior_icon() {
    let tmp = TempDir::new().unwrap();
    let inst_dir = tmp.path();
    let src1 = inst_dir.join("a.png");
    let src2 = inst_dir.join("b.jpg");
    fs::write(&src1, b"one").unwrap();
    fs::write(&src2, b"two").unwrap();

    let mut inst = stub_instance(vec![]);
    write_instance_icon(inst_dir, &mut inst, &src1).unwrap();
    write_instance_icon(inst_dir, &mut inst, &src2).unwrap();

    // Only one icon-* file remains, matching the current field.
    let icons = icon_files(inst_dir);
    assert_eq!(icons.len(), 1, "exactly one icon survives: {icons:?}");
    assert_eq!(Some(&icons[0]), inst.icon.as_ref());
    assert!(icons[0].ends_with(".jpg"));
}

#[test]
fn icon_write_rejects_bad_extension() {
    let tmp = TempDir::new().unwrap();
    let inst_dir = tmp.path();
    let src = inst_dir.join("evil.txt");
    fs::write(&src, b"nope").unwrap();

    let mut inst = stub_instance(vec![]);
    let err = write_instance_icon(inst_dir, &mut inst, &src).unwrap_err();
    assert!(err.contains("unsupported"), "err: {err}");
    assert!(inst.icon.is_none(), "icon unchanged on reject");
}

#[test]
fn icon_write_rejects_oversize() {
    let tmp = TempDir::new().unwrap();
    let inst_dir = tmp.path();
    let src = inst_dir.join("huge.png");
    // 4 MiB + 1 byte.
    fs::write(&src, vec![0u8; (4 * 1024 * 1024) + 1]).unwrap();

    let mut inst = stub_instance(vec![]);
    let err = write_instance_icon(inst_dir, &mut inst, &src).unwrap_err();
    assert!(err.contains("too large"), "err: {err}");
    assert!(inst.icon.is_none());
}

#[test]
fn icon_clear_removes_file_and_field() {
    let tmp = TempDir::new().unwrap();
    let inst_dir = tmp.path();
    let src = inst_dir.join("a.webp");
    fs::write(&src, b"img").unwrap();

    let mut inst = stub_instance(vec![]);
    write_instance_icon(inst_dir, &mut inst, &src).unwrap();
    assert!(inst.icon.is_some());

    clear_instance_icon_file(inst_dir, &mut inst).unwrap();
    assert!(inst.icon.is_none());
    assert!(icon_files(inst_dir).is_empty(), "icon file removed");
}

#[test]
fn icon_field_round_trips_through_manifest() {
    let tmp = TempDir::new().unwrap();
    let mut inst = stub_instance(vec![]);
    inst.icon = Some("icon-123.png".into());
    let path = write_inst(tmp.path(), &inst);
    let reloaded = read_manifest_pub(&path).unwrap();
    assert_eq!(reloaded.icon.as_deref(), Some("icon-123.png"));
}

/// `set_pending_launch_warning_suppressed_on_disk` flips and persists the flag.
#[test]
fn cp3_suppress_pending_launch_warning_persists() {
    let tmp = TempDir::new().unwrap();
    let inst = stub_instance(vec![]);
    let path = write_inst(tmp.path(), &inst);

    assert!(!read_manifest_pub(&path).unwrap().suppress_pending_launch_warning);

    set_pending_launch_warning_suppressed_on_disk(&path, true).unwrap();
    assert!(read_manifest_pub(&path).unwrap().suppress_pending_launch_warning);

    set_pending_launch_warning_suppressed_on_disk(&path, false).unwrap();
    assert!(!read_manifest_pub(&path).unwrap().suppress_pending_launch_warning);
}
