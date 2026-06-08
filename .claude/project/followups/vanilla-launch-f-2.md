---
id: vanilla-launch-f-2
title: -Dminecraft.client.jar= gets asset_index_id not jar path (legacy)
created: "2026-06-08"
origin: |
    docs/spec/vanilla-launch.md, iter 1 (CP1)
kind: plan
severity: nit
review_by: "2026-08-07"
status: open
file: src-tauri/src/core/launch.rs:249
---

default_jvm_args (legacy-manifest path) passes asset_index_id (e.g. "17") as the value of -Dminecraft.client.jar=, but that property conventionally takes the client jar path. Non-fatal, legacy-only, but semantically wrong. Fix: pass the resolved client jar path. Coupled to vanilla-launch-f-1 (pre-1.7 support). Origin: vanilla-launch slice D, CP1 reviewer.
