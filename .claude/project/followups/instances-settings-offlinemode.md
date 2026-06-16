---
id: instances-settings-offlinemode
title: Settings ipc.ts missing offlineMode field present in Rust Settings struct
created: "2026-06-15"
origin: |
    refresh-signals scan 2026-06-15
kind: finding
severity: nit
review_by: "2026-08-14"
status: open
file: src/lib/ipc.ts
---

Rust Settings struct has offline_mode: bool (default false); the ipc.ts Settings interface omits offlineMode. If a Settings page renders/saves it, IPC silently drops the field.
