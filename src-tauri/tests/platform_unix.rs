//! Unix-only integration tests — compiled on Linux and macOS, absent elsewhere.
//!
//! Holds behavior shared across all Unix targets. OS-divergent specifics
//! (XDG vs Application Support data dirs, keyring backends) belong in
//! `platform_linux.rs` / `platform_macos.rs`.

#![cfg(unix)]

use std::path::{Path, MAIN_SEPARATOR};

#[test]
fn separator_is_forward_slash() {
    assert_eq!(MAIN_SEPARATOR, '/');
}

#[test]
fn join_uses_forward_slash() {
    let p = Path::new("/data").join("ApexLauncher");
    assert_eq!(p.to_str(), Some("/data/ApexLauncher"));
}
