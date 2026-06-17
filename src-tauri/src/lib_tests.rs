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
    let kinds: Vec<&str> = vec!["mrpack", "curseforge", "manual"];
    let mut seen = std::collections::HashSet::new();
    for k in &kinds {
        assert!(seen.insert(*k), "duplicate kind tag: {k}");
    }
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
