---
id: providers-providerkind-casing
title: 'ProviderKind dual-casing: curseForge (response) vs curseforge (routing param)'
created: "2026-06-15"
origin: |
    refresh-signals scan 2026-06-15
kind: finding
severity: risk
review_by: "2026-08-14"
status: open
file: src/lib/ipc.ts
---

ProviderKind response value is camelCase 'curseForge' but the routing param string is lowercase 'curseforge'. Two distinct string shapes coexist; Browse.tsx else-branch papers over it. Adding a third provider will silently route incorrectly.
