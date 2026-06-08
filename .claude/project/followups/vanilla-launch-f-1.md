---
id: vanilla-launch-f-1
title: Materialize assets_legacy virtual tree for pre-1.7 launch
created: "2026-06-08"
origin: |
    docs/spec/vanilla-launch.md, iter 1 (CP1)
kind: plan
severity: risk
review_by: "2026-08-07"
status: open
file: src-tauri/src/core/launch.rs
---

build_argv selects the legacy/virtual assets_root path when LaunchMeta.assets_legacy is true, but the resolver only sets the assets_legacy bool — it never materializes the assets/virtual/legacy tree (copying objects keyed by asset name). A pre-1.7 instance would launch pointing at a non-existent dir. Modern versions (the practical target; version list filters to releases) are unaffected. Fix before supporting pre-1.7 launch: materialize the virtual tree in the resolver or launch path. Origin: vanilla-launch slice D, CP1.
