---
id: modpack-import-d-f-5
title: set_mod_enabled/remove_mod load the manifest twice
created: "2026-06-17"
origin: |
    docs/spec/modpack-import.md, slice D iter 4 reviewer
kind: finding
severity: risk
review_by: "2026-08-16"
status: open
file: src-tauri/src/lib.rs
---

Pack Lock guard loads the manifest, then instances::set_mod_enabled/remove_mod loads it again. add_mod/update_mod load once and hold the instance. Inconsistent + a stale-guard window if a write lands between loads (negligible on single-user desktop). Fix: thread the loaded instance into the inner ops, or document the double-read as intentional.
