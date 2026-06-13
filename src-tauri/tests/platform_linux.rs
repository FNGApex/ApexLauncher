//! Linux-only integration tests — compiled on Linux, absent elsewhere.
//!
//! Scaffold for Linux-divergent behavior as it appears: XDG data dir resolution
//! (`~/.local/share/ApexLauncher`), the secret-service keyring backend, and
//! hardlink/copy-fallback semantics on ext4 vs 9p mounts. Add real cases here
//! rather than gating them inline in the source modules.

#![cfg(target_os = "linux")]

#[test]
fn linux_target_smoke() {
    assert_eq!(std::env::consts::OS, "linux");
}
