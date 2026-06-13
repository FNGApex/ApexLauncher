//! Platform-agnostic integration tests — must pass on every target OS.
//!
//! The rule these tests enforce: path assertions compare the ordered `Normal`
//! components of a `Path`, never its raw `to_str()` against a hardcoded
//! separator. A string like `"/data/ApexLauncher/cache"` is only correct on
//! Unix; on Windows `Path::join` emits `\`, so a literal-`/` comparison fails.
//! Comparing components is separator-agnostic while still pinning the intended
//! layout (segment names and order).

use modloader_lib::core::store::{cache_subdir_path, data_root_from_base};
use std::path::{Component, Path};

/// Ordered `Normal` path segments — the separator-agnostic shape of a path.
fn segments(p: &Path) -> Vec<&str> {
    p.components()
        .filter_map(|c| match c {
            Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect()
}

#[test]
fn data_root_appends_apexlauncher() {
    let root = data_root_from_base(Path::new("/data"));
    assert_eq!(segments(&root), vec!["data", "ApexLauncher"]);
}

#[test]
fn cache_subdir_shape() {
    let root = data_root_from_base(Path::new("/data"));
    let assets = cache_subdir_path(&root, "assets");
    assert_eq!(
        segments(&assets),
        vec!["data", "ApexLauncher", "cache", "assets"]
    );
}
