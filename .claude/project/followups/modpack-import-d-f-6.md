---
id: modpack-import-d-f-6
title: d4_update_modpack_unguarded_contract does not verify the contract
created: "2026-06-17"
origin: |
    docs/spec/modpack-import.md, slice D iter 4 reviewer
kind: finding
severity: nit
review_by: "2026-08-16"
status: open
file: src-tauri/src/lib_tests.rs
---

Test d4_update_modpack_unguarded_contract only re-asserts ensure_not_locked rejects a locked instance (dupes d4_ensure_not_locked_err_when_locked); it does not guard against a future accidental ensure_not_locked call being added to update_modpack. Rename to reflect what it asserts, or encode the intent.
