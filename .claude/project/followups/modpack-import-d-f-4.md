---
id: modpack-import-d-f-4
title: update_modpack archive GET has no timeout
created: "2026-06-17"
origin: |
    docs/spec/modpack-import.md, slice D iter 3 reviewer
kind: finding
severity: risk
review_by: "2026-08-16"
status: open
file: src-tauri/src/lib.rs
---

update_modpack stages the archive via bare reqwest::Client::new().get(url) with no timeout — same shape as modpack-import-c-archive-timeout (install_modpack). Fold both into one timeout fix.
