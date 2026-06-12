//! Instance file-tree materialization — storage-auth-reorg, Slice C, CP1.
//!
//! Given a shared `cache/` root and a per-instance target directory, builds
//! the same relative file tree under the instance directory by hardlinking
//! each artifact. Falls back to a byte copy when hardlinking fails (e.g.
//! cross-volume `EXDEV`). Idempotent: an already-present destination is
//! silently skipped.
//!
//! ## Injectable link seam
//!
//! The per-file hardlink operation is injectable via a `link_fn` closure so
//! that unit tests can force a failure (cross-device error) and assert the
//! copy fallback ran and produced a correct byte copy. The public wrapper
//! [`materialize`] passes `std::fs::hard_link` as the default.

use std::{
    fs,
    io,
    path::{Path, PathBuf},
};

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
            return Err(format!(
                "materialize: source not found: {}",
                src.display()
            ));
        }

        // Already present → idempotent skip.
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

        // Attempt hardlink; fall back to copy on any IO error.
        match link_fn(&src, &dst) {
            Ok(()) => {}
            Err(_) => {
                fs::copy(&src, &dst).map_err(|e| {
                    format!(
                        "materialize: copy fallback failed for {} → {}: {e}",
                        src.display(),
                        dst.display()
                    )
                })?;
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Public wrapper (uses std::fs::hard_link)
// ---------------------------------------------------------------------------

/// Materialize `rel_paths` from `cache_root` into `instance_dir` using
/// `std::fs::hard_link` with a copy fallback.
///
/// See [`materialize_core`] for the full contract.
pub fn materialize(
    cache_root: &Path,
    instance_dir: &Path,
    rel_paths: &[PathBuf],
) -> Result<(), String> {
    materialize_core(cache_root, instance_dir, rel_paths, |src, dst| {
        fs::hard_link(src, dst)
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use tempfile::TempDir;

    // -----------------------------------------------------------------------
    // Hardlink path: dest created, links real content, nested rel dirs created.
    // -----------------------------------------------------------------------

    #[test]
    fn hardlink_path_creates_dest_with_real_content() {
        let cache_tmp = TempDir::new().unwrap();
        let inst_tmp = TempDir::new().unwrap();

        let cache_root = cache_tmp.path();
        let instance_dir = inst_tmp.path();

        // Create a nested source file under cache.
        let rel: PathBuf = PathBuf::from("libraries/com/example/foo/1.0/foo-1.0.jar");
        let src = cache_root.join(&rel);
        fs::create_dir_all(src.parent().unwrap()).unwrap();
        fs::write(&src, b"fake-jar-content").unwrap();

        // Run with real hard_link.
        materialize(cache_root, instance_dir, &[rel.clone()]).unwrap();

        let dst = instance_dir.join(&rel);
        assert!(dst.exists(), "destination must exist after materialize");

        let content = fs::read(&dst).unwrap();
        assert_eq!(content, b"fake-jar-content", "destination must contain source content");

        // Verify parent directories were created.
        assert!(dst.parent().unwrap().is_dir(), "parent dirs must be created");
    }

    // -----------------------------------------------------------------------
    // Copy fallback: injected failing linker → dest is a correct byte copy.
    // -----------------------------------------------------------------------

    #[test]
    fn copy_fallback_runs_when_link_fn_fails() {
        let cache_tmp = TempDir::new().unwrap();
        let inst_tmp = TempDir::new().unwrap();

        let cache_root = cache_tmp.path();
        let instance_dir = inst_tmp.path();

        let rel: PathBuf = PathBuf::from("versions/1.21.1/1.21.1.jar");
        let src = cache_root.join(&rel);
        fs::create_dir_all(src.parent().unwrap()).unwrap();
        fs::write(&src, b"mc-client-bytes").unwrap();

        // Link function simulates EXDEV (cross-device link).
        let exdev_linker = |_src: &Path, _dst: &Path| -> io::Result<()> {
            Err(io::Error::from_raw_os_error(libc_exdev()))
        };

        materialize_core(cache_root, instance_dir, &[rel.clone()], exdev_linker).unwrap();

        let dst = instance_dir.join(&rel);
        assert!(dst.exists(), "destination must exist after copy fallback");

        let content = fs::read(&dst).unwrap();
        assert_eq!(content, b"mc-client-bytes", "copy fallback must produce byte-identical file");
    }

    // -----------------------------------------------------------------------
    // Idempotency: second call with same inputs is a no-op and returns Ok.
    // -----------------------------------------------------------------------

    #[test]
    fn second_call_is_idempotent_no_error() {
        let cache_tmp = TempDir::new().unwrap();
        let inst_tmp = TempDir::new().unwrap();

        let cache_root = cache_tmp.path();
        let instance_dir = inst_tmp.path();

        let rel: PathBuf = PathBuf::from("libraries/net/minecraft/client/1.0/client.jar");
        let src = cache_root.join(&rel);
        fs::create_dir_all(src.parent().unwrap()).unwrap();
        fs::write(&src, b"idempotency-test-content").unwrap();

        // Track link_fn call count to verify it is NOT called on second run.
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = Arc::clone(&call_count);

        let counting_linker = move |src: &Path, dst: &Path| -> io::Result<()> {
            cc.fetch_add(1, Ordering::SeqCst);
            fs::hard_link(src, dst)
        };

        // First call — should link.
        materialize_core(cache_root, instance_dir, &[rel.clone()], &counting_linker).unwrap();
        assert_eq!(call_count.load(Ordering::SeqCst), 1, "link_fn must be called on first run");

        // Second call — destination already exists, link_fn must NOT be called again.
        materialize_core(cache_root, instance_dir, &[rel.clone()], &counting_linker).unwrap();
        assert_eq!(call_count.load(Ordering::SeqCst), 1, "link_fn must NOT be called on second run (idempotent)");

        // Destination content is still correct.
        let content = fs::read(instance_dir.join(&rel)).unwrap();
        assert_eq!(content, b"idempotency-test-content");
    }

    // -----------------------------------------------------------------------
    // Missing source: returns a clear Err.
    // -----------------------------------------------------------------------

    #[test]
    fn missing_source_returns_err() {
        let cache_tmp = TempDir::new().unwrap();
        let inst_tmp = TempDir::new().unwrap();

        let rel: PathBuf = PathBuf::from("libraries/does/not/exist.jar");

        let result = materialize(cache_tmp.path(), inst_tmp.path(), &[rel]);
        assert!(result.is_err(), "must return Err when source is missing");

        let msg = result.unwrap_err();
        assert!(
            msg.contains("source not found"),
            "error message must mention 'source not found': {msg}"
        );
    }

    // -----------------------------------------------------------------------
    // Multiple rel_paths: all destinations created correctly.
    // -----------------------------------------------------------------------

    #[test]
    fn multiple_paths_all_materialized() {
        let cache_tmp = TempDir::new().unwrap();
        let inst_tmp = TempDir::new().unwrap();

        let cache_root = cache_tmp.path();
        let instance_dir = inst_tmp.path();

        let rels: Vec<PathBuf> = vec![
            PathBuf::from("libraries/a/a.jar"),
            PathBuf::from("libraries/b/b.jar"),
            PathBuf::from("versions/1.0/1.0.jar"),
        ];

        for rel in &rels {
            let src = cache_root.join(rel);
            fs::create_dir_all(src.parent().unwrap()).unwrap();
            fs::write(&src, rel.to_string_lossy().as_bytes()).unwrap();
        }

        materialize(cache_root, instance_dir, &rels).unwrap();

        for rel in &rels {
            let dst = instance_dir.join(rel);
            assert!(dst.exists(), "destination must exist: {}", dst.display());
            let content = fs::read(&dst).unwrap();
            assert_eq!(content, rel.to_string_lossy().as_bytes());
        }
    }

    // -----------------------------------------------------------------------
    // Empty rel_paths: returns Ok immediately (no-op).
    // -----------------------------------------------------------------------

    #[test]
    fn empty_rel_paths_returns_ok() {
        let cache_tmp = TempDir::new().unwrap();
        let inst_tmp = TempDir::new().unwrap();
        let result = materialize(cache_tmp.path(), inst_tmp.path(), &[]);
        assert!(result.is_ok());
    }

    // Helper: return the platform EXDEV errno value.
    fn libc_exdev() -> i32 {
        // EXDEV = 18 on Linux, macOS, and Windows (ERROR_NOT_SAME_DEVICE).
        18
    }
}
