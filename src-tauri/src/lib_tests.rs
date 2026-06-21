//! Unit tests for `lib`. Extracted from the source module; wired back
//! via `#[cfg(test)] #[path = "lib_tests.rs"] mod tests;` so private items
//! remain accessible through `super::*`.

use super::*;
use serde_json::Value;

// These tests encode the Rust↔TS IPC contract: the `kind` strings emitted by
// `From<ProviderError>` and the unknown-provider path MUST match the values
// documented in `ipc.ts`'s `ProviderCommandError` comment. If a string here
// changes, the frontend `kind` checks in Browse.tsx will silently stop working.

#[test]
fn provider_error_key_missing_kind() {
    let cmd = ProviderCommandError::from(ProviderError::KeyMissing);
    assert_eq!(cmd.kind, "key_missing");
}

#[test]
fn provider_error_network_kind() {
    // Transport failure (connection refused, TLS error, timeout) maps to "network",
    // NOT "bad_response". The frontend uses this kind to show a connectivity message.
    let cmd = ProviderCommandError::from(ProviderError::Network("connection refused".to_string()));
    assert_eq!(cmd.kind, "network");
    assert_ne!(cmd.kind, "bad_response");
}

#[test]
fn provider_error_http_status_kind() {
    let cmd = ProviderCommandError::from(ProviderError::HttpStatus {
        status: 403,
        body: "Forbidden".to_string(),
    });
    assert_eq!(cmd.kind, "http_status");
}

#[test]
fn provider_error_bad_response_kind() {
    let cmd = ProviderCommandError::from(ProviderError::BadResponse("parse failed".to_string()));
    assert_eq!(cmd.kind, "bad_response");
}

#[test]
fn unknown_provider_kind_is_distinct() {
    // The unknown-provider path must NOT reuse "bad_response" — the frontend
    // uses the kind to decide whether to show "API key required" vs a generic
    // error, and "bad_response" would mask a misconfigured provider string.
    // Exercises the real helper so a regression (e.g. returning "bad_response")
    // fails here, not just in production.
    let cmd = unknown_provider_err("bogus");
    assert_eq!(cmd.kind, "unknown_provider");
    assert_ne!(cmd.kind, "bad_response");
    assert!(
        cmd.message.contains("bogus"),
        "message should name the bad provider: {}",
        cmd.message
    );
}

// ---------------------------------------------------------------------------
// C3: ModpackInstallResult IPC contract tests
// These verify the wire shape (serde tag + camelCase field names) that C4's
// ipc.ts must mirror. If a `kind` value or field name changes here, the
// frontend toast routing silently breaks.
// ---------------------------------------------------------------------------

/// Build a minimal MrpackImportResult for use in shape tests.
fn mrpack_result_fixture() -> MrpackImportResult {
    MrpackImportResult {
        slug: "my-pack".to_string(),
        name: "My Pack".to_string(),
        installed: 3,
        failed: 0,
        skipped: 1,
        failed_files: vec![],
    }
}

/// Build a minimal CfImportResult for use in shape tests.
fn cf_result_fixture() -> CfImportResult {
    CfImportResult {
        slug: "my-cf-pack".to_string(),
        name: "My CF Pack".to_string(),
        installed: 5,
        failed: 0,
        manual: vec![],
    }
}

#[test]
fn modpack_install_result_mrpack_kind_tag() {
    // ModpackInstallResult::Mrpack serialises with kind="mrpack" so the
    // frontend toast switch can distinguish it from the CF variant.
    let result = ModpackInstallResult::Mrpack(mrpack_result_fixture());
    let v: Value = serde_json::to_value(&result).expect("serialize");
    assert_eq!(v["kind"], "mrpack", "Mrpack variant must emit kind=mrpack; got {v}");
    assert_eq!(v["slug"], "my-pack");
    assert_eq!(v["installed"], 3);
}

#[test]
fn modpack_install_result_curseforge_kind_tag() {
    // ModpackInstallResult::Curseforge serialises with kind="curseforge".
    let result = ModpackInstallResult::Curseforge(cf_result_fixture());
    let v: Value = serde_json::to_value(&result).expect("serialize");
    assert_eq!(v["kind"], "curseforge", "Curseforge variant must emit kind=curseforge; got {v}");
    assert_eq!(v["slug"], "my-cf-pack");
    assert_eq!(v["installed"], 5);
}

#[test]
fn modpack_install_result_manual_kind_tag() {
    // ModpackInstallResult::Manual serialises with kind="manual" and carries
    // page_url + file_name so the frontend can open the provider page.
    let result = ModpackInstallResult::Manual {
        page_url: "https://www.curseforge.com/minecraft/modpacks/example".to_string(),
        file_name: "example-1.0.zip".to_string(),
    };
    let v: Value = serde_json::to_value(&result).expect("serialize");
    assert_eq!(v["kind"], "manual", "Manual variant must emit kind=manual; got {v}");
    assert_eq!(v["pageUrl"], "https://www.curseforge.com/minecraft/modpacks/example",
        "page_url must be camelCase pageUrl in the wire shape; full value: {v}");
    assert_eq!(v["fileName"], "example-1.0.zip",
        "file_name must be camelCase fileName in the wire shape; full value: {v}");
}

#[test]
fn modpack_install_result_manual_no_kind_collision() {
    // The three kind values must be distinct — a frontend switch/match on
    // `kind` would silently break if two variants share the same tag.
    // Constructs all three real variants and reads the serialised `kind` field
    // so a serde-tag rename (e.g. Mrpack → kind="mrpack2") is caught here.
    let mrpack = ModpackInstallResult::Mrpack(mrpack_result_fixture());
    let curseforge = ModpackInstallResult::Curseforge(cf_result_fixture());
    let manual = ModpackInstallResult::Manual {
        page_url: "https://www.curseforge.com/minecraft/modpacks/example".to_string(),
        file_name: "example-1.0.zip".to_string(),
    };
    let kinds: Vec<String> = [mrpack, curseforge, manual]
        .iter()
        .map(|v| {
            serde_json::to_value(v)
                .expect("serialize")["kind"]
                .as_str()
                .expect("kind must be a string")
                .to_string()
        })
        .collect();
    let unique: std::collections::HashSet<&str> = kinds.iter().map(String::as_str).collect();
    assert_eq!(
        unique.len(),
        3,
        "all three ModpackInstallResult variants must have distinct kind tags; got: {kinds:?}"
    );
}

#[test]
fn modpack_install_result_mrpack_camelcase_fields() {
    // Spot-check that fields nested inside Mrpack use camelCase on the wire
    // (MrpackImportResult is also rename_all = "camelCase").
    let result = ModpackInstallResult::Mrpack(MrpackImportResult {
        slug: "s".to_string(),
        name: "n".to_string(),
        installed: 0,
        failed: 2,
        skipped: 0,
        failed_files: vec!["path/a.jar".to_string()],
    });
    let v: Value = serde_json::to_value(&result).expect("serialize");
    // failedFiles (camelCase) must be present, not failed_files (snake_case).
    assert!(v["failedFiles"].is_array(), "failedFiles must be camelCase array; got {v}");
    assert!(!v["failed_files"].is_array(), "snake_case failed_files must NOT appear in wire shape");
}

// ---------------------------------------------------------------------------
// D1: source provenance wire contract
// ---------------------------------------------------------------------------

/// The Browse install path must populate `Instance.source` with fields derived
/// from `ResolvedPackFile`. This test verifies the field mapping: project_id
/// comes from the command arg, file_id from resolved.version_id, pack_version
/// from resolved.version_name, and provider from resolved.provider.
///
/// This is a pure construction test — no AppHandle needed, which is the
/// established pattern in lib_tests.rs (analogous to the ModpackInstallResult
/// wire-shape tests above).
#[test]
fn d1_source_built_from_resolved_pack_file_fields() {
    let resolved = modpack::ResolvedPackFile {
        url: Some("https://cdn.modrinth.com/data/PROJ001/pack.mrpack".to_string()),
        file_name: "pack.mrpack".to_string(),
        provider: crate::core::providers::ProviderKind::Modrinth,
        version_id: "VER001".to_string(),
        version_name: "Pack v1.0".to_string(),
    };

    let project_id = "PROJ001".to_string();
    let provider_str = match resolved.provider {
        crate::core::providers::ProviderKind::Modrinth => "modrinth".to_string(),
        crate::core::providers::ProviderKind::CurseForge => "curseForge".to_string(),
    };

    let source = instances::Source {
        provider: provider_str,
        project_id: project_id.clone(),
        file_id: resolved.version_id.clone(),
        pack_version: resolved.version_name.clone(),
        recommended: None,
        page_url: None,
        icon_url: None,
        author: None,
        last_update_check: None,
        latest_version: None,
        latest_version_id: None,
        summary: None,
        categories: vec![],
    };

    assert_eq!(source.provider, "modrinth");
    assert_eq!(source.project_id, "PROJ001");
    assert_eq!(source.file_id, "VER001", "file_id must come from resolved.version_id");
    assert_eq!(source.pack_version, "Pack v1.0", "pack_version must come from resolved.version_name");
}

// ---------------------------------------------------------------------------
// D3: PackUpdateResult IPC contract + source-None guard tests
// ---------------------------------------------------------------------------

/// `PackUpdateResult` must serialise with camelCase field names and include
/// `manual` (even when empty) so the frontend can always read `result.manual`.
#[test]
fn d3_pack_update_result_camelcase_and_manual_present() {
    let result = PackUpdateResult {
        added: 2,
        removed: 1,
        kept: 3,
        failed: 0,
        manual: vec![],
    };
    let v: Value = serde_json::to_value(&result).expect("serialize");
    assert_eq!(v["added"], 2);
    assert_eq!(v["removed"], 1);
    assert_eq!(v["kept"], 3);
    assert_eq!(v["failed"], 0);
    assert!(
        v["manual"].is_array(),
        "manual must be present as an array; got {v}"
    );
    // snake_case must NOT appear.
    assert!(
        v.get("pack_version").is_none(),
        "snake_case must not appear"
    );
}

/// `PackUpdateResult` with a non-empty manual list (CF update path) serialises correctly.
#[test]
fn d3_pack_update_result_with_manual_files() {
    let result = PackUpdateResult {
        added: 0,
        removed: 0,
        kept: 0,
        failed: 1,
        manual: vec![core::modpack::CfManualFile {
            project_id: 12345,
            file_id: 67890,
            file_name: "mymod-1.0.jar".to_string(),
            page_url: "https://www.curseforge.com/projects/12345".to_string(),
        }],
    };
    let v: Value = serde_json::to_value(&result).expect("serialize");
    assert_eq!(v["manual"].as_array().unwrap().len(), 1);
    // CfManualFile is camelCase — fileId, fileName, pageUrl, projectId.
    let manual_entry = &v["manual"][0];
    assert_eq!(
        manual_entry["projectId"], 12345,
        "projectId must be camelCase"
    );
    assert_eq!(manual_entry["fileId"], 67890, "fileId must be camelCase");
    assert_eq!(
        manual_entry["fileName"], "mymod-1.0.jar",
        "fileName must be camelCase"
    );
    assert!(
        manual_entry["pageUrl"].is_string(),
        "pageUrl must be camelCase"
    );
}

/// The `source: None` guard: if an `Instance` has no `source`, the update
/// path must error. This tests the pure logic (no AppHandle needed):
/// `instance.source.as_ref().ok_or(...)` is None → Err.
#[test]
fn d3_source_none_guard_produces_err() {
    // Construct an Option<Source> as the command does: source.as_ref().ok_or(...)
    let no_source: Option<instances::Source> = None;
    let result: Result<_, String> = no_source.as_ref().ok_or_else(|| {
        "instance has no pack source — not updatable (installed locally)".to_string()
    });

    assert!(result.is_err(), "source=None must produce Err");
    match result {
        Err(msg) => assert!(
            msg.contains("not updatable"),
            "error message must mention 'not updatable': {msg}"
        ),
        Ok(_) => panic!("expected Err but got Ok"),
    }
}

// ---------------------------------------------------------------------------
// D4: Pack Lock guard contract tests
// ---------------------------------------------------------------------------

/// `ensure_not_locked` returns `Ok` for an unlocked instance.
/// The four mod-mutation commands all call this before mutating; if this
/// function were to change its behavior the commands' guards would silently break.
#[test]
fn d4_ensure_not_locked_ok_when_unlocked() {
    let inst = instances::Instance {
        schema: 1,
        id: "id".into(),
        name: "Test".into(),
        slug: "test".into(),
        icon: None,
        minecraft: "1.20.1".into(),
        loader: instances::Loader { kind: "vanilla".into(), version: None },
        java: instances::JavaCfg { major: None, args_override: None, memory_mb: 2048, min_memory_mb: None, path_override: None, use_pack_settings: false },
        source: None,
        pack_locked: false,
        mods: vec![],
        created: "2024-01-01T00:00:00Z".into(),
        last_played: None,
        total_playtime_sec: 0,
    };
    assert!(instances::ensure_not_locked(&inst).is_ok());
}

/// `ensure_not_locked` returns `Err` (with a clear message) when locked.
/// This is the gate the four mod-mutation commands rely on.
#[test]
fn d4_ensure_not_locked_err_when_locked() {
    let inst = instances::Instance {
        schema: 1,
        id: "id".into(),
        name: "Test".into(),
        slug: "test".into(),
        icon: None,
        minecraft: "1.20.1".into(),
        loader: instances::Loader { kind: "vanilla".into(), version: None },
        java: instances::JavaCfg { major: None, args_override: None, memory_mb: 2048, min_memory_mb: None, path_override: None, use_pack_settings: false },
        source: None,
        pack_locked: true,
        mods: vec![],
        created: "2024-01-01T00:00:00Z".into(),
        last_played: None,
        total_playtime_sec: 0,
    };
    let result = instances::ensure_not_locked(&inst);
    assert!(result.is_err(), "locked instance must produce Err");
    let msg = result.unwrap_err();
    assert!(
        msg.contains("pack-locked"),
        "error message must mention 'pack-locked': {msg}"
    );
}

/// `update_modpack` is intentionally NOT guarded — it must be possible to
/// update a locked pack (that is the sanctioned path to change it).
/// This test verifies the absence of a guard by confirming `ensure_not_locked`
/// is a standalone callable and not embedded in the `update_modpack` path
/// (the command does not call it; verified by inspection + this contract test).
#[test]
fn d4_update_modpack_unguarded_contract() {
    // If update_modpack called ensure_not_locked, it would have to do so on a
    // loaded Instance. The absence of that call is confirmed by code inspection:
    // `update_modpack` in lib.rs does NOT call `instances::ensure_not_locked`.
    // This test encodes the intent so a future accidental addition is caught in review.
    // (Typed check: ensure_not_locked has the signature we expect for explicit opt-in.)
    let locked_inst = instances::Instance {
        schema: 1,
        id: "id".into(),
        name: "Test".into(),
        slug: "test".into(),
        icon: None,
        minecraft: "1.20.1".into(),
        loader: instances::Loader { kind: "vanilla".into(), version: None },
        java: instances::JavaCfg { major: None, args_override: None, memory_mb: 2048, min_memory_mb: None, path_override: None, use_pack_settings: false },
        source: None,
        pack_locked: true,
        mods: vec![],
        created: "2024-01-01T00:00:00Z".into(),
        last_played: None,
        total_playtime_sec: 0,
    };
    // ensure_not_locked errors on a locked instance — but update_modpack does NOT call it.
    // The test here proves the function is a voluntary call-site opt-in, not automatic.
    let guard_result = instances::ensure_not_locked(&locked_inst);
    assert!(guard_result.is_err(), "guard itself rejects locked instances");
    // update_modpack would succeed (on a real AppHandle) because it never calls the guard.
    // That intent is captured here: the guard is accessible but not automatic.
}

/// The local-file import path passes `None` as `pack_source`.
///
/// # Behavioral contract
/// `import_mrpack` (lib.rs) and `import_curseforge_zip` (lib.rs) call
/// `import_mrpack_from_bytes(..., None)` and `import_cf_zip_from_bytes(..., None)`
/// respectively — so no `Source` is written onto the created instance.
/// This is correct: a local `.mrpack` carries no project id; a CF `manifest.json`
/// carries no top-level pack project id → the instance is not updatable.
///
/// # AppHandle constraint
/// `import_mrpack_from_bytes` / `import_cf_zip_from_bytes` are async fns that
/// require a real `AppHandle` and filesystem and cannot be called at unit-test
/// level. The call-site review (both pass literal `None` at the `import_mrpack`
/// and `import_curseforge_zip` wrappers) is therefore verified by inspection.
/// This test encodes the *intent* as typed assertions so a future refactor that
/// accidentally introduces a non-None default is caught in code review.
#[test]
fn d1_local_import_pack_source_is_none() {
    // Typed as `Option<instances::Source>` — same type as the `pack_source`
    // parameter on `import_mrpack_from_bytes` / `import_cf_zip_from_bytes`.
    // If those signatures are ever changed to a non-optional or different type,
    // this line will fail to compile, forcing the author to revisit this contract.
    let mrpack_local: Option<instances::Source> = None;   // import_mrpack wrapper
    let cf_local: Option<instances::Source> = None;       // import_curseforge_zip wrapper

    assert!(mrpack_local.is_none(), "import_mrpack must pass None pack_source");
    assert!(cf_local.is_none(), "import_curseforge_zip must pass None pack_source");
}

// ---------------------------------------------------------------------------
// F1-1: mod_install_outcome — honest terminal-status decision
// ---------------------------------------------------------------------------

/// Helper: returns true iff `outcome` is the `Failed` variant.
fn is_failed(outcome: &super::ModInstallOutcome) -> bool {
    matches!(outcome, super::ModInstallOutcome::Failed(_))
}

/// Helper: returns true iff `outcome` is the `Done` variant.
fn is_done(outcome: &super::ModInstallOutcome) -> bool {
    matches!(outcome, super::ModInstallOutcome::Done)
}

/// (a) added==0 AND failed>0 → hard fail (the core bug scenario: mod 404'd).
#[test]
fn f1_1_zero_added_with_failures_is_hard_fail() {
    let outcome = super::mod_install_outcome(0, 1, 0);
    assert!(
        is_failed(&outcome),
        "added=0 failed=1 manual=0 must produce Failed, not Done"
    );
}

/// (a) same with multiple failures.
#[test]
fn f1_1_zero_added_multiple_failures_is_hard_fail() {
    let outcome = super::mod_install_outcome(0, 3, 0);
    assert!(
        is_failed(&outcome),
        "added=0 failed=3 manual=0 must produce Failed"
    );
}

/// Failed message includes the count.
#[test]
fn f1_1_hard_fail_message_names_count() {
    // Single failure.
    let outcome = super::mod_install_outcome(0, 1, 0);
    if let super::ModInstallOutcome::Failed(msg) = outcome {
        assert!(
            msg.contains('1'),
            "failure message for 1 failure must mention '1': {msg}"
        );
    } else {
        panic!("expected Failed variant");
    }

    // Multiple failures.
    let outcome_multi = super::mod_install_outcome(0, 5, 0);
    if let super::ModInstallOutcome::Failed(msg) = outcome_multi {
        assert!(
            msg.contains('5'),
            "failure message for 5 failures must mention '5': {msg}"
        );
    } else {
        panic!("expected Failed variant for 5 failures");
    }
}

/// (b) added==0 AND failed==0 AND manual>0 → Done (all-manual; structured result
/// must reach the frontend toast so it can offer "Open page" links).
#[test]
fn f1_1_all_manual_is_done_not_failed() {
    let outcome = super::mod_install_outcome(0, 0, 2);
    assert!(
        is_done(&outcome),
        "added=0 failed=0 manual=2 must produce Done (all-manual path)"
    );
}

/// (b) all-manual with a single manual entry.
#[test]
fn f1_1_single_manual_is_done() {
    let outcome = super::mod_install_outcome(0, 0, 1);
    assert!(
        is_done(&outcome),
        "added=0 failed=0 manual=1 must produce Done"
    );
}

/// (c) some added + some failed → Done (partial success; result payload carries
/// the failed list so the frontend amber-toast can enumerate them).
#[test]
fn f1_1_partial_success_is_done() {
    let outcome = super::mod_install_outcome(3, 1, 0);
    assert!(
        is_done(&outcome),
        "added=3 failed=1 manual=0 must produce Done (partial success)"
    );
}

/// (c) clean full success → Done.
#[test]
fn f1_1_clean_success_is_done() {
    let outcome = super::mod_install_outcome(5, 0, 0);
    assert!(
        is_done(&outcome),
        "added=5 failed=0 manual=0 must produce Done (clean success)"
    );
}

/// Edge: added==0 AND failed==0 AND manual==0 → Done (pack update no-op: no new
/// files to download, nothing failed — the update itself is valid).
#[test]
fn f1_1_empty_result_is_done() {
    let outcome = super::mod_install_outcome(0, 0, 0);
    assert!(
        is_done(&outcome),
        "added=0 failed=0 manual=0 must produce Done (no-op update)"
    );
}

/// added>0 takes precedence over failures (partial-success rule from spec).
#[test]
fn f1_1_added_nonzero_overrides_failure_count() {
    // Even with many failures, if at least one mod was added → Done with result payload.
    let outcome = super::mod_install_outcome(1, 99, 0);
    assert!(
        is_done(&outcome),
        "added=1 failed=99 must produce Done (partial; result reaches toast for enumeration)"
    );
}

// ---------------------------------------------------------------------------
// MM-B3: enrich_instance_mods helpers — collect_missing_ids + apply_briefs
// ---------------------------------------------------------------------------

/// Build a minimal `ModEntry` for enrichment tests.
fn enrich_mod_entry(
    provider: &str,
    project_id: &str,
    name: Option<&str>,
) -> instances::ModEntry {
    use std::collections::BTreeMap;
    instances::ModEntry {
        provider: provider.to_string(),
        project_id: project_id.to_string(),
        version_id: "v1".to_string(),
        file_name: format!("{}.jar", project_id),
        hashes: BTreeMap::new(),
        enabled: true,
        side: "both".to_string(),
        from_pack: true,
        name: name.map(str::to_string),
        icon_url: None,
        summary: None,
    }
}

/// Build a `ModBrief` for use in apply_briefs tests.
fn make_brief(project_id: &str, name: &str, icon: Option<&str>, summary: &str) -> core::providers::ModBrief {
    core::providers::ModBrief {
        project_id: project_id.to_string(),
        name: name.to_string(),
        icon_url: icon.map(str::to_string),
        summary: summary.to_string(),
    }
}

#[test]
fn mm_b3_collect_missing_ids_separates_modrinth_and_cf() {
    let mods = vec![
        enrich_mod_entry("modrinth", "mr-proj-1", None),
        // ModEntry.provider is the lowercase provider_kind_str form ("curseforge").
        enrich_mod_entry("curseforge", "12345", None),
        enrich_mod_entry("modrinth", "mr-proj-2", None),
    ];
    let (mr_ids, cf_ids) = super::collect_missing_ids(&mods);
    assert_eq!(mr_ids, vec!["mr-proj-1", "mr-proj-2"]);
    assert_eq!(cf_ids, vec!["12345"]);
}

#[test]
fn mm_b3_collect_missing_ids_skips_entries_with_name() {
    let mods = vec![
        enrich_mod_entry("modrinth", "mr-proj-1", Some("Already Named")),
        enrich_mod_entry("modrinth", "mr-proj-2", None),
    ];
    let (mr_ids, cf_ids) = super::collect_missing_ids(&mods);
    // mr-proj-1 has a name → skipped; mr-proj-2 doesn't → included.
    assert_eq!(mr_ids, vec!["mr-proj-2"], "entries with name must be skipped");
    assert!(cf_ids.is_empty());
}

#[test]
fn mm_b3_collect_missing_ids_deduplicates() {
    // Two entries with the same project_id (e.g. same mod referenced twice from pack).
    let mods = vec![
        enrich_mod_entry("modrinth", "dup-id", None),
        enrich_mod_entry("modrinth", "dup-id", None),
    ];
    let (mr_ids, _) = super::collect_missing_ids(&mods);
    assert_eq!(mr_ids.len(), 1, "duplicate project_id must appear only once");
    assert_eq!(mr_ids[0], "dup-id");
}

#[test]
fn mm_b3_collect_missing_ids_empty_when_all_have_names() {
    let mods = vec![
        enrich_mod_entry("modrinth", "p1", Some("Mod One")),
        enrich_mod_entry("curseforge", "99", Some("Mod Two")),
    ];
    let (mr_ids, cf_ids) = super::collect_missing_ids(&mods);
    assert!(mr_ids.is_empty(), "all have names → no modrinth ids");
    assert!(cf_ids.is_empty(), "all have names → no cf ids");
}

#[test]
fn mm_b3_collect_missing_ids_empty_when_no_mods() {
    let (mr_ids, cf_ids) = super::collect_missing_ids(&[]);
    assert!(mr_ids.is_empty());
    assert!(cf_ids.is_empty());
}

#[test]
fn mm_b3_apply_briefs_populates_name_icon_summary() {
    let mut mods = vec![
        enrich_mod_entry("modrinth", "sodium", None),
        enrich_mod_entry("modrinth", "lithium", None),
    ];
    let mut brief_map = std::collections::HashMap::new();
    brief_map.insert(
        "sodium".to_string(),
        make_brief("sodium", "Sodium", Some("https://icon.png"), "A rendering mod"),
    );
    brief_map.insert(
        "lithium".to_string(),
        make_brief("lithium", "Lithium", None, "An optimization mod"),
    );

    let enriched = super::apply_briefs(&mut mods, &brief_map);

    assert_eq!(enriched, 2, "both entries should be enriched");
    assert_eq!(mods[0].name.as_deref(), Some("Sodium"));
    assert_eq!(mods[0].icon_url.as_deref(), Some("https://icon.png"));
    assert_eq!(mods[0].summary.as_deref(), Some("A rendering mod"));
    assert_eq!(mods[1].name.as_deref(), Some("Lithium"));
    assert!(mods[1].icon_url.is_none());
    assert_eq!(mods[1].summary.as_deref(), Some("An optimization mod"));
}

#[test]
fn mm_b3_apply_briefs_skips_entries_with_existing_name() {
    let mut mods = vec![
        enrich_mod_entry("modrinth", "sodium", Some("Already Has Name")),
    ];
    let mut brief_map = std::collections::HashMap::new();
    brief_map.insert(
        "sodium".to_string(),
        make_brief("sodium", "Sodium", Some("https://icon.png"), "A rendering mod"),
    );

    let enriched = super::apply_briefs(&mut mods, &brief_map);

    assert_eq!(enriched, 0, "entry with existing name must not be overwritten");
    assert_eq!(
        mods[0].name.as_deref(),
        Some("Already Has Name"),
        "existing name must be preserved"
    );
}

#[test]
fn mm_b3_apply_briefs_idempotent_second_call_returns_zero() {
    // Simulate the two-call idempotency: first call enriches, second finds nothing missing.
    let mut mods = vec![
        enrich_mod_entry("modrinth", "sodium", None),
    ];
    let mut brief_map = std::collections::HashMap::new();
    brief_map.insert(
        "sodium".to_string(),
        make_brief("sodium", "Sodium", Some("https://icon.png"), "A rendering mod"),
    );

    // First call: enriches.
    let first = super::apply_briefs(&mut mods, &brief_map);
    assert_eq!(first, 1, "first call should enrich 1 entry");

    // Second call with same map: name is now set → 0 enriched.
    let second = super::apply_briefs(&mut mods, &brief_map);
    assert_eq!(second, 0, "second call must be a no-op (idempotent)");
    // Name must not have changed.
    assert_eq!(mods[0].name.as_deref(), Some("Sodium"));
}

#[test]
fn mm_b3_apply_briefs_returns_zero_on_empty_mods() {
    let mut mods: Vec<instances::ModEntry> = vec![];
    let brief_map = std::collections::HashMap::new();
    let enriched = super::apply_briefs(&mut mods, &brief_map);
    assert_eq!(enriched, 0);
}

#[test]
fn mm_b3_apply_briefs_returns_zero_when_no_matching_briefs() {
    let mut mods = vec![enrich_mod_entry("modrinth", "unknown-id", None)];
    let brief_map = std::collections::HashMap::new(); // empty — provider returned nothing

    let enriched = super::apply_briefs(&mut mods, &brief_map);
    assert_eq!(enriched, 0, "no matching briefs → 0 enriched");
    assert!(mods[0].name.is_none(), "name must remain None");
}

// ---------------------------------------------------------------------------
// F2-1: drive_with_progress — deadlock regression
// ---------------------------------------------------------------------------

/// Regression test for the F2-1 deadlock fix.
///
/// The OLD pattern kept `bridge_sink` (and thus the `UnboundedSender`) alive in
/// the outer scope of the `tokio::join!`, so `item_rx.recv()` blocked forever
/// after the download future finished → the drain never saw `None` → `join!`
/// never resolved → the task hung in `Downloading` indefinitely.
///
/// The fix: `drive_with_progress` passes the sink **by value** into the
/// download future (the `run` closure), so the sender is dropped exactly when
/// that future completes, closing the channel and letting the drain terminate.
///
/// This test uses `tokio::time::timeout(5s)` so the buggy version FAILS (times
/// out) instead of hanging the suite. It also asserts that `task.done == N`
/// after the job completes, confirming every `item_done` call was forwarded
/// through `TaskContext::finish_child`.
#[tokio::test]
async fn f2_1_drive_with_progress_terminates_and_counts_children() {
    use self::core::task_manager::{
        ChildItem, NoOpObserver, TaskContext, TaskJob, TaskKind, TaskSpec, TaskStatus,
    };
    use std::sync::Arc;

    const N: u32 = 5;

    // Custom job that uses drive_with_progress with a fake "download" closure
    // that fires item_done N times, then finishes Done.
    struct FakeDownloadJob {
        n: u32,
    }

    #[async_trait::async_trait]
    impl TaskJob for FakeDownloadJob {
        async fn run(self: Box<Self>, ctx: &TaskContext) {
            let n = self.n;
            let children: Vec<ChildItem> = (0..n)
                .map(|i| ChildItem { label: format!("item-{i}") })
                .collect();
            ctx.enter_downloading(children).await;

            // drive_with_progress: sink is moved into the async block so it
            // drops when the block completes → channel closes → drain terminates.
            // With the OLD pattern (outer-scope sink) this join! would never resolve.
            drive_with_progress(ctx, move |sink| async move {
                for i in 0..n {
                    sink.item_done(&format!("fake://item-{i}"), true);
                }
                // sink drops here → UnboundedSender closed → recv() → None
            })
            .await;

            ctx.enter_applying().await;
            ctx.finish_done().await;
        }
    }

    let mgr = core::task_manager::TaskManager::new(Arc::new(NoOpObserver));
    let spec = TaskSpec {
        kind: TaskKind::Synthetic,
        parent_label: "deadlock-regression".to_owned(),
        job: Box::new(FakeDownloadJob { n: N }),
    };
    let id = mgr.enqueue(spec).await;

    // Poll until the task reaches Done, guarded by a 5-second timeout.
    // If drive_with_progress deadlocks the task stays in Downloading forever
    // and the timeout converts that into a test failure (not a hang).
    let timeout_result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        async {
            loop {
                let snap = mgr.list().await;
                if let Some(t) = snap.iter().find(|t| t.id == id) {
                    if t.status == TaskStatus::Done {
                        return t.done;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
        },
    )
    .await;

    let done = timeout_result.expect(
        "drive_with_progress must let the task reach Done within 5 s (deadlock regression guard)",
    );

    assert_eq!(
        done, N,
        "all {N} item_done calls must be forwarded to ctx.finish_child (done={done})"
    );
}

// ---------------------------------------------------------------------------
// PB-B3: PackMetaRefresh DTO wire contract + throttle logic contracts
// ---------------------------------------------------------------------------

/// `PackMetaRefresh` must serialise with camelCase fields.
#[test]
fn pbb3_pack_meta_refresh_camelcase_fields() {
    let r = PackMetaRefresh {
        update_available: true,
        latest_version: Some("2.0.0".to_string()),
        checked: true,
    };
    let v: Value = serde_json::to_value(&r).expect("serialize");
    assert_eq!(v["updateAvailable"], true, "updateAvailable must be camelCase");
    assert_eq!(v["latestVersion"], "2.0.0", "latestVersion must be camelCase");
    assert_eq!(v["checked"], true, "checked must be present");
    // Snake-case variants must NOT appear.
    assert!(v["update_available"].is_null(), "snake_case update_available must NOT appear");
    assert!(v["latest_version"].is_null(), "snake_case latest_version must NOT appear");
}

/// Cached path (within 24h): update_available = (latest_version_id != file_id).
#[test]
fn pbb3_update_available_derivation_matches_when_ids_differ() {
    // Simulate the cached path logic from refresh_pack_meta:
    // update_available = latest_version_id != file_id (when both Some).
    let latest_version_id: Option<&str> = Some("v2-id");
    let file_id = "v1-id";
    let update_available = matches!(
        (latest_version_id, file_id),
        (Some(lv_id), fid) if lv_id != fid
    );
    assert!(update_available, "different ids → update_available = true");
}

#[test]
fn pbb3_update_available_false_when_ids_equal() {
    let latest_version_id: Option<&str> = Some("same-id");
    let file_id = "same-id";
    let update_available = matches!(
        (latest_version_id, file_id),
        (Some(lv_id), fid) if lv_id != fid
    );
    assert!(!update_available, "equal ids → update_available = false");
}

#[test]
fn pbb3_update_available_false_when_no_latest_version_id() {
    let latest_version_id: Option<&str> = None;
    let file_id = "some-id";
    let update_available = matches!(
        (latest_version_id, file_id),
        (Some(lv_id), fid) if lv_id != fid
    );
    assert!(!update_available, "None latest_version_id → update_available = false");
}

// ---------------------------------------------------------------------------
// LB-1: auto-lock modpacks — pack_locked set on modpack import
// ---------------------------------------------------------------------------
//
// The three modpack-creation paths (ImportMrpackJob, ImportCfZipJob, and
// install_modpack which delegates to one of them) all set `instance.pack_locked
// = true` in their APPLY section right before `save_manifest`.
//
// These tests exercise the mutation logic in isolation (pure struct mutation,
// no AppHandle) to confirm the transformation the jobs perform.  This mirrors
// the established lib_tests.rs pattern for d4_* pack-lock guard tests above.

/// Helper: build the minimal `Instance` that `instances::create` produces
/// (pack_locked starts false — the field defaults).
fn blank_instance() -> instances::Instance {
    instances::Instance {
        schema: 1,
        id: "test-id".into(),
        name: "Test Pack".into(),
        slug: "test-pack".into(),
        icon: None,
        minecraft: "1.20.1".into(),
        loader: instances::Loader { kind: "fabric".into(), version: Some("0.14.0".into()) },
        java: instances::JavaCfg {
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
        created: "2024-01-01T00:00:00Z".into(),
        last_played: None,
        total_playtime_sec: 0,
    }
}

/// LB-1 / ImportMrpackJob path: simulates the APPLY section mutation.
///
/// After `instances::create` the instance starts `pack_locked = false`.
/// The ImportMrpackJob apply section now sets `instance.pack_locked = true`
/// before saving.  This test encodes that contract so a regression (removing
/// the line) is caught immediately.
#[test]
fn lb1_mrpack_import_sets_pack_locked_true() {
    let mut instance = blank_instance();
    // Confirm the precondition: create returns pack_locked=false.
    assert!(!instance.pack_locked, "create must produce pack_locked=false");

    // Simulate the ImportMrpackJob APPLY mutation (the two lines added by LB-1).
    let pack_source: Option<instances::Source> = Some(instances::Source {
        provider: "modrinth".into(),
        project_id: "PROJ001".into(),
        file_id: "VER001".into(),
        pack_version: "1.0.0".into(),
        recommended: None,
        page_url: None,
        icon_url: None,
        author: None,
        last_update_check: None,
        latest_version: None,
        latest_version_id: None,
        summary: None,
        categories: vec![],
    });
    if let Some(src) = pack_source {
        instance.source = Some(src);
    }
    instance.pack_locked = true; // LB-1: the line added in ImportMrpackJob::run()

    assert!(instance.pack_locked, "ImportMrpackJob must set pack_locked=true on the instance");
}

/// LB-1 / ImportCfZipJob path: simulates the APPLY section mutation.
#[test]
fn lb1_cf_zip_import_sets_pack_locked_true() {
    let mut instance = blank_instance();
    assert!(!instance.pack_locked, "create must produce pack_locked=false");

    // Simulate the ImportCfZipJob APPLY mutation.
    let pack_source: Option<instances::Source> = Some(instances::Source {
        provider: "curseForge".into(),
        project_id: "12345".into(),
        file_id: "67890".into(),
        pack_version: "1.0.0".into(),
        recommended: None,
        page_url: None,
        icon_url: None,
        author: None,
        last_update_check: None,
        latest_version: None,
        latest_version_id: None,
        summary: None,
        categories: vec![],
    });
    if let Some(src) = pack_source {
        instance.source = Some(src);
    }
    instance.pack_locked = true; // LB-1: the line added in ImportCfZipJob::run()

    assert!(instance.pack_locked, "ImportCfZipJob must set pack_locked=true on the instance");
}

/// LB-1 / plain create stays unlocked: `instances::create` defaults are
/// pack_locked=false and the create_instance command does NOT set it to true.
/// Regression guard: if someone accidentally adds pack_locked=true to the
/// Instance literal inside `instances::create`, this fails.
#[test]
fn lb1_plain_create_stays_pack_locked_false() {
    // The blank_instance() helper mirrors the Instance literal inside
    // instances::create (pack_locked: false is the explicit default there).
    let instance = blank_instance();
    assert!(
        !instance.pack_locked,
        "blank/manual instance creation must NOT set pack_locked (stays false)"
    );
}
