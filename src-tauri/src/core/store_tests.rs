//! Unit tests for `store`. Extracted from the source module; wired back
//! via `#[cfg(test)] #[path = "store_tests.rs"] mod tests;` so private items
//! remain accessible through `super::*`.

use super::*;
use std::path::{Component, Path};

/// Ordered `Normal` path segments, separator-agnostic.
///
/// Path shape (segment order) is the contract these tests verify; the OS
/// separator is not. Comparing `to_str()` against a hardcoded `/` string
/// fails on Windows, where `Path::join` emits `\`. Comparing the `Normal`
/// components sidesteps that while still pinning the intended layout.
fn segments(p: &Path) -> Vec<&str> {
    p.components()
        .filter_map(|c| match c {
            Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect()
}

// --- data_root_from_base ---

#[test]
fn data_root_ends_with_apex_launcher() {
    let base = Path::new("/home/user/.local/share");
    let root = data_root_from_base(base);
    assert_eq!(
        root.file_name().and_then(|n| n.to_str()),
        Some("ApexLauncher"),
        "last component must be ApexLauncher"
    );
}

#[test]
fn data_root_does_not_contain_reverse_dns_identifier() {
    let base = Path::new("/home/user/.local/share");
    let root = data_root_from_base(base);
    let s = root.to_string_lossy();
    assert!(
        !s.contains("com.bear.modloader"),
        "path must not contain old bundle id: {s}"
    );
    assert!(
        !s.contains("com.apex.apexlauncher"),
        "path must not contain any bundle id: {s}"
    );
}

#[test]
fn data_root_on_macos_shape() {
    let base = Path::new("/Users/bear/Library/Application Support");
    let root = data_root_from_base(base);
    assert_eq!(
        segments(&root),
        vec![
            "Users",
            "bear",
            "Library",
            "Application Support",
            "ApexLauncher"
        ]
    );
}

#[test]
fn data_root_on_windows_shape() {
    let base = Path::new(r"C:\Users\Bear\AppData\Roaming");
    let root = data_root_from_base(base);
    assert!(
        root.to_str().unwrap().ends_with("ApexLauncher"),
        "Windows path should end with ApexLauncher"
    );
}

// --- cache_subdir_path ---

#[test]
fn cache_subdir_assets_shape() {
    let root = Path::new("/data/ApexLauncher");
    let p = cache_subdir_path(root, "assets");
    assert_eq!(
        segments(&p),
        vec!["data", "ApexLauncher", "cache", "assets"]
    );
}

#[test]
fn cache_subdir_libraries_shape() {
    let root = Path::new("/data/ApexLauncher");
    let p = cache_subdir_path(root, "libraries");
    assert_eq!(
        segments(&p),
        vec!["data", "ApexLauncher", "cache", "libraries"]
    );
}

#[test]
fn cache_subdir_versions_shape() {
    let root = Path::new("/data/ApexLauncher");
    let p = cache_subdir_path(root, "versions");
    assert_eq!(
        segments(&p),
        vec!["data", "ApexLauncher", "cache", "versions"]
    );
}

#[test]
fn cache_subdir_java_shape() {
    let root = Path::new("/data/ApexLauncher");
    let p = cache_subdir_path(root, "java");
    assert_eq!(segments(&p), vec!["data", "ApexLauncher", "cache", "java"]);
}

#[test]
fn cache_subdir_meta_shape() {
    let root = Path::new("/data/ApexLauncher");
    let p = cache_subdir_path(root, "meta");
    assert_eq!(segments(&p), vec!["data", "ApexLauncher", "cache", "meta"]);
}

#[test]
fn cache_subdir_installers_shape() {
    let root = Path::new("/data/ApexLauncher");
    let p = cache_subdir_path(root, "installers");
    assert_eq!(
        segments(&p),
        vec!["data", "ApexLauncher", "cache", "installers"]
    );
}

// --- composed path shape (base → root → cache subdir) ---

// --- cache_dir path shape (pure helper equivalent) ---

#[test]
fn cache_dir_path_shape() {
    // cache_dir delegates to data_root_from_base(base).join("cache").
    // Verify: last component is "cache", parent is the data root.
    let base = Path::new("/home/user/.local/share");
    let root = data_root_from_base(base);
    let cache = root.join("cache");
    assert_eq!(
        cache.file_name().and_then(|n| n.to_str()),
        Some("cache"),
        "last component of cache dir must be 'cache'"
    );
    assert_eq!(
        segments(&cache),
        vec!["home", "user", ".local", "share", "ApexLauncher", "cache"]
    );
}

#[test]
fn full_path_composition_assets() {
    let base = Path::new("/home/user/.local/share");
    let root = data_root_from_base(base);
    let assets = cache_subdir_path(&root, "assets");
    assert_eq!(
        segments(&assets),
        vec![
            "home",
            "user",
            ".local",
            "share",
            "ApexLauncher",
            "cache",
            "assets"
        ]
    );
}

#[test]
fn full_path_composition_java() {
    let base = Path::new("/home/user/.local/share");
    let root = data_root_from_base(base);
    let java = cache_subdir_path(&root, "java");
    assert_eq!(
        segments(&java),
        vec![
            "home",
            "user",
            ".local",
            "share",
            "ApexLauncher",
            "cache",
            "java"
        ]
    );
}
