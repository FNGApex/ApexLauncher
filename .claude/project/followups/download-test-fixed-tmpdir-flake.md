---
id: download-test-fixed-tmpdir-flake
title: download.rs test_tmp_dir uses fixed path; cleanup-on-success-only wedges cp4 test
created: "2026-06-09"
origin: |
    discovered during fabric-quilt-launch finalize (2026-06-09)
kind: finding
severity: risk
review_by: "2026-08-08"
status: open
file: src-tauri/src/core/download.rs:1073
---

`core::download::tests::cp4_concurrency_bound_not_exceeded` (and any sibling using `test_tmp_dir`) writes to a FIXED temp path (`$TMPDIR/cp3_test_<name>`) and only cleans it up at the END of the test (download.rs:1573). All 8 items share identical content/hash. Once the test fails for any reason, cleanup is skipped, leaving `item0.bin` on disk; the next run sees dest-exists + hash-match -> `ItemStatus::Skipped` -> `expected Ok, got Skipped` -> permanent failure until the temp dir is manually cleared. Confirmed PRE-EXISTING (fails at base commit e6d53c4, before Phase 4 work). Fix: `test_tmp_dir` should return a unique dir (tempfile::TempDir, RAII cleanup) or clear the dir at test START, not only on success. Affects test reliability + any future CI (Phase 7). Workaround today: `rm -rf $TMPDIR/cp3_test_*`.
