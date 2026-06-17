---
id: modpack-import-c-archive-timeout
title: install_modpack archive GET has no timeout
created: "2026-06-17"
origin: |
    docs/spec/modpack-import.md, slice C iter 3-4 reviewer
kind: finding
severity: risk
review_by: "2026-08-16"
status: open
file: src-tauri/src/lib.rs:1529
---

install_modpack downloads the pack archive via a bare reqwest::Client::new() GET with no timeout; a hung server blocks the command indefinitely, bypassing the download engine's retry/timeout logic. Acceptable for slice C (live progress is a non-goal). Fix in slice D: route the archive through the download engine or add an explicit timeout.
