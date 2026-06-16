---
id: modpack-import-cf-manual-slug-link
title: CF manual link uses numeric project id, not file page
created: "2026-06-16"
origin: |
    docs/spec/modpack-import.md, slice B CP B3 (design-acknowledged risk)
kind: finding
severity: nit
review_by: "2026-08-15"
status: open
file: src-tauri/src/core/modpack.rs
---

Manual-download entries for distribution-disabled CF files link to `https://www.curseforge.com/projects/<projectID>` (a numeric-id redirect) rather than a direct file-download page. The CF file record from `get_file` does not carry the mod slug, so a precise `…/mc-mods/<slug>/files/<fileID>` link needs an extra mod lookup.

**Why:** the numeric-id link lands the user on the right project; the exact file page is a UX nicety, deferred for slice B.
**How to apply:** in `resolve_and_build_cf_plan`, for manual entries fetch the parent mod (`/v1/mods/<projectID>`) to get `links.websiteUrl`/slug, build `…/download/<fileID>`. Costs one extra request per manual file only.
