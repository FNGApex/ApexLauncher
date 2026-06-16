//! Unit tests for `lib`. Extracted from the source module; wired back
//! via `#[cfg(test)] #[path = "lib_tests.rs"] mod tests;` so private items
//! remain accessible through `super::*`.

use super::*;

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
