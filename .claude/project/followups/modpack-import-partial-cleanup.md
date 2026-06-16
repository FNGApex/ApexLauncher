---
id: modpack-import-partial-cleanup
title: mrpack import leaves half-created instance on plan/download failure (no rollback)
created: "2026-06-15"
origin: |
    CP4 review 2026-06-15
kind: finding
severity: risk
review_by: "2026-08-14"
status: open
file: src-tauri/src/lib.rs (import_mrpack)
---

If build_pack_plan errors after instances::create (e.g. disallowed host), or execute_plan completes with failures, the instance is left on disk partially populated. Slice A has no rollback/cleanup. Decide: rollback on hard failure, or surface partial state in MrpackImportResult and let the user delete. Acceptable for slice A; revisit in slice D (pack update/re-resolve).
