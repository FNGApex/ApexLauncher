---
id: download-broken-runtime
title: Downloads broken at runtime (engine path fails; repro + diagnose with new logs)
created: "2026-06-17"
origin: |
    dev-logging GUI verification, 2026-06-17
kind: finding
severity: risk
review_by: "2026-08-16"
status: open
file: src-tauri/src/core/download.rs
---

Observed during dev-logging GUI verification (2026-06-17, macOS dev run). Login works end-to-end (begin_login INFO logged, signed in OK). Downloads reported broken at runtime — the download engine path (asset/library fetch on launch, mod add, and modpack install all ride core/download.rs execute_plan) fails. Error not yet captured: the GUI session only exercised login, so no download ERROR/WARN reached apex.log.

Next session: reproduce by triggering a logged download path (e.g. add a Modrinth mod — no API key needed — or launch an instance), capture the [WARN/ERROR core::download] lines now emitted by CP2 logging, and diagnose. The new structured logging should make the failing item + reason visible in ~/Library/Logs/com.apex.apexlauncher/apex.log.
