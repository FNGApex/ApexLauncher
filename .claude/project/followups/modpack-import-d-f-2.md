---
id: modpack-import-d-f-2
title: d1_source_built test duplicates production construction
created: "2026-06-17"
origin: |
    docs/spec/modpack-import.md, slice D iter 1 reviewer
kind: finding
severity: nit
review_by: "2026-08-16"
status: open
file: src-tauri/src/lib_tests.rs
---

Test `d1_source_built_from_resolved_pack_file_fields` rebuilds the Source itself and asserts on its own construction — would not catch install_modpack swapping version_id/pack_version. Useful as a typed contract; add a comment noting the call-site-coupling limitation.
