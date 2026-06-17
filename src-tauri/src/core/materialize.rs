//! Instance file-tree materialization — storage-auth-reorg, Slice C, CP1;
//! copy isolation, download-runner-rework CP-5.
//!
//! Given a shared `cache/` root and a per-instance target directory, builds
//! the same relative file tree under the instance directory by writing an
//! independent **byte copy** of each artifact (`std::fs::copy`). Each instance
//! therefore owns its libs + version jars outright — editing or deleting one
//! instance's file never affects another instance or the shared cache. Assets
//! stay shared (immutable, content-addressed) and are not materialized here.
//! Idempotent: an already-present destination is silently skipped.
//!
//! ## Injectable link seam
//!
//! The per-file copy operation is injectable via a `link_fn` closure so that
//! unit tests can force a failure (cross-device error) and assert the copy
//! fallback path runs. The public wrapper [`materialize`] passes
//! `std::fs::copy` as the default; the cross-device fallback (also a byte copy)
//! is now moot for the default path but kept for any injected linker that
//! reports `EXDEV`.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

// ---------------------------------------------------------------------------
// EXDEV detection
// ---------------------------------------------------------------------------

/// Returns `true` if `e` represents a cross-device link error.
///
/// Only this error class justifies a silent byte-copy fallback; all other link
/// errors (permission denied, missing parent, etc.) must propagate as `Err`.
fn is_cross_device(e: &io::Error) -> bool {
    // ErrorKind::CrossesDevices (stable Rust ≥ 1.85, toolchain 1.96) covers:
    //   - Linux/macOS: EXDEV = 18
    //   - Windows: ERROR_NOT_SAME_DEVICE = 17
    // The raw_os_error == Some(18) arm keeps injected-EXDEV unit tests green on
    // all platforms (ErrorKind::CrossesDevices may not round-trip through
    // from_raw_os_error on non-Windows hosts).
    e.kind() == io::ErrorKind::CrossesDevices || e.raw_os_error() == Some(18)
}

// ---------------------------------------------------------------------------
// Core materializer (injectable link seam)
// ---------------------------------------------------------------------------

/// Materialize `rel_paths` from `cache_root` into `instance_dir` using the
/// provided `link_fn` for the hardlink attempt, falling back to a byte copy
/// on failure.
///
/// For each `rel` in `rel_paths`:
/// - source = `cache_root/rel`
/// - dest   = `instance_dir/rel`
///
/// Behavior:
/// - Creates parent directories for `dest` as needed.
/// - If `dest` already exists → skips (idempotent).
/// - Tries `link_fn(source, dest)`; on `io::Error` falls back to `fs::copy`.
/// - If `source` does not exist → returns `Err(String)` with a clear message.
pub fn materialize_core<F>(
    cache_root: &Path,
    instance_dir: &Path,
    rel_paths: &[PathBuf],
    link_fn: F,
) -> Result<(), String>
where
    F: Fn(&Path, &Path) -> io::Result<()>,
{
    for rel in rel_paths {
        let src = cache_root.join(rel);
        let dst = instance_dir.join(rel);

        // Missing source → caller error.
        if !src.exists() {
            return Err(format!("materialize: source not found: {}", src.display()));
        }

        // Already present → idempotent skip.
        // Correctness assumption: artifacts are content-addressed / version-pinned
        // maven coordinates, so an existing file at `dst` is byte-for-byte identical
        // to the source. No content check is performed; presence is sufficient.
        if dst.exists() {
            continue;
        }

        // Ensure parent directories exist.
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                format!(
                    "materialize: failed to create parent dir {}: {e}",
                    parent.display()
                )
            })?;
        }

        // Attempt hardlink; fall back to byte copy ONLY on cross-device error
        // (EXDEV, raw OS error 18 / ErrorKind::CrossesDevices). Any other link
        // error (permission denied, missing parent, unsupported FS) propagates
        // as Err — those are not transient cross-volume issues and silently
        // copying would mask a real misconfiguration.
        match link_fn(&src, &dst) {
            Ok(()) => {}
            Err(e) if is_cross_device(&e) => {
                fs::copy(&src, &dst).map_err(|ce| {
                    format!(
                        "materialize: copy fallback failed for {} → {}: {ce}",
                        src.display(),
                        dst.display()
                    )
                })?;
            }
            Err(e) => {
                return Err(format!(
                    "materialize: hard_link failed for {} → {}: {e}",
                    src.display(),
                    dst.display()
                ));
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Public wrapper (uses std::fs::copy for per-instance isolation)
// ---------------------------------------------------------------------------

/// Materialize `rel_paths` from `cache_root` into `instance_dir` as independent
/// **byte copies** (`std::fs::copy`).
///
/// Each instance owns its libs + version jars outright — no shared inode, so
/// mutating or deleting one instance's file leaves the cache source and every
/// other instance untouched. See [`materialize_core`] for the full contract.
pub fn materialize(
    cache_root: &Path,
    instance_dir: &Path,
    rel_paths: &[PathBuf],
) -> Result<(), String> {
    materialize_core(cache_root, instance_dir, rel_paths, |src, dst| {
        fs::copy(src, dst).map(|_| ())
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "materialize_tests.rs"]
mod tests;
