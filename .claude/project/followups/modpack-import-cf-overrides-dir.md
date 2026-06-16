---
id: modpack-import-cf-overrides-dir
title: CF import ignores non-default overrides dir name
created: "2026-06-16"
origin: |
    docs/spec/modpack-import.md, slice B CP B4 (design-acknowledged risk)
kind: finding
severity: risk
review_by: "2026-08-15"
status: open
file: src-tauri/src/core/modpack.rs
---

CF `manifest.json` may set a non-default `overrides` key (dir name other than `overrides/`). CP B4 reuses slice-A `extract_overrides` unchanged, which only applies the hardcoded `overrides/` prefix — a pack using a custom overrides dir would silently skip those files.

**Why:** low-likelihood (almost all CF packs use the default `overrides/`), so deferred for slice B per the design.
**How to apply:** thread `CfManifest.overrides` (already parsed) into the extraction so the CF path honors the declared dir name; add a fixture pack with a renamed overrides dir.
