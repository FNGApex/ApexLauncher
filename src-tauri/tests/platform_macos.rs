//! macOS-only integration tests — compiled on macOS, absent elsewhere.
//!
//! Scaffold for macOS-divergent behavior as it appears: Application Support
//! data dir resolution (`~/Library/Application Support/ApexLauncher`), the
//! apple-native keyring backend, and app-bundle launch paths. Add real cases
//! here rather than gating them inline in the source modules.

#![cfg(target_os = "macos")]

#[test]
fn macos_target_smoke() {
    assert_eq!(std::env::consts::OS, "macos");
}
