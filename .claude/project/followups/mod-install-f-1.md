---
id: mod-install-f-1
title: Thread provider client/server side into ModEntry (currently hardcoded 'unknown')
created: "2026-06-15"
origin: |
    docs/spec/mod-install.md, iter 1 reviewer (CP-1)
kind: finding
severity: risk
review_by: "2026-08-14"
status: open
file: src-tauri/src/core/mod_install.rs:200
---

ProjectVersion exposes no per-version client/server side, so resolve_install hardcodes PlannedMod.side = "unknown" (mod_install.rs:200). If ModEntry.side ever drives install/skip behavior (e.g. server-only mod filtering), this becomes a silent logic bug. Gap is upstream: the ModProvider trait / normalized types do not surface client_side/server_side from the provider response. Thread side through providers before any consumer reads ModEntry.side.
