---
id: mod-install-root-slug
title: add_mod passes project_id as root_slug; dep page URLs id-based not slug-based
created: "2026-06-15"
origin: |
    refresh-signals scan 2026-06-15
kind: finding
severity: nit
review_by: "2026-08-14"
status: open
file: src-tauri/src/lib.rs:863
---

add_mod passes the mod's project_id as both root_project_id and root_slug to resolve_install, so even the root mod's page URL is id-based rather than the human-readable slug. Design intent: thread ProjectSummary.slug from Browse.
