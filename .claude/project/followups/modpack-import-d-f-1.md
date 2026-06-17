---
id: modpack-import-d-f-1
title: d1_local_import_pack_source_is_none is a tautology test
created: "2026-06-17"
origin: |
    docs/spec/modpack-import.md, slice D iter 1 reviewer
kind: finding
severity: risk
review_by: "2026-08-16"
status: open
file: src-tauri/src/lib_tests.rs
---

Test `d1_local_import_pack_source_is_none` asserts `None.is_none()` and never calls import_mrpack_from_bytes/import_cf_zip_from_bytes — no regression guard if a local command is later changed to pass Some(source). AppHandle constraint blocks a real unit call. Add a command-level shape test once an AppHandle harness exists, or downgrade to an explicit doc-only marker.
