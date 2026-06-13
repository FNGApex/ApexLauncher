//! Windows-only integration tests — compiled on Windows, absent elsewhere.
//!
//! Windows is the project's primary build/test target (see the Windows cargo
//! toolchain notes). Path-separator behavior diverges here: `Path::join`
//! emits `\`, which is exactly what tripped the original `store` unit tests.

#![cfg(windows)]

use std::path::{Path, MAIN_SEPARATOR};

#[test]
fn separator_is_backslash() {
    assert_eq!(MAIN_SEPARATOR, '\\');
}

#[test]
fn join_uses_backslash() {
    let p = Path::new(r"C:\data").join("ApexLauncher");
    assert_eq!(p.to_str(), Some(r"C:\data\ApexLauncher"));
}
