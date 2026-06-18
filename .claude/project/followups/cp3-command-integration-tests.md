---
id: cp3-command-integration-tests
title: CP-3 pack jobs lack command-level integration tests
created: "2026-06-18"
origin: |
    CP-3 inline review
kind: finding
severity: risk
review_by: "2026-08-17"
status: open
---

ImportMrpackJob/ImportCfZipJob/UpdateModpackJob run() bodies need a tauri::AppHandle (instances::create/load/save, store paths) so are not unit-tested. Stage seams (remap_to_staging/promote_staging), result carrier, and TaskKind serialization ARE tested. Add a Tauri-harness or extracted-core integration test covering: command returns id without awaiting; labeled child queue after PLAN; cancel mid-download leaves instance tree clean.
