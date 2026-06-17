---
id: modpack-import-c-empty-pageurl
title: 'Manual variant: empty pageUrl + misleading toast (F-2+F-7)'
created: "2026-06-17"
origin: |
    docs/spec/modpack-import.md, slice C iter 3-4 reviewer
kind: finding
severity: risk
review_by: "2026-08-16"
status: open
file: src-tauri/src/lib.rs:1519
---

A CF non-distributable pack with no page_url yields ModpackInstallResult::Manual{pageUrl:""} (lib.rs page_url.unwrap_or_else(|| String::new())), and the Browse manual toast (Browse.tsx:378) still says the project page was opened even though the empty-string guard skips openUrl. C4's Browse card always supplies page_url from ProjectSummary.page_url, so the integrated path is correct; this is a defensive edge for other callers. Fix: make empty page_url an explicit backend error (or Option), and branch the toast copy. Deferred to slice D.
