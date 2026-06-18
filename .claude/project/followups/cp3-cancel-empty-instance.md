---
id: cp3-cancel-empty-instance
title: CP-3 cancel leaves created-but-empty instance dir
created: "2026-06-18"
origin: |
    CP-3 inline review
kind: finding
severity: nit
review_by: "2026-08-17"
status: open
---

On cancel mid-download, the job discards staging + finish_cancelled, but the instance created in PLAN remains (empty mc/mods). Staging discard keeps mods clean (criterion met) but the orphan instance is UX cruft. Consider deleting the instance on cancel for fresh installs (not updates).
