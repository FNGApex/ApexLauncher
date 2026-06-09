---
id: fabric-quilt-launch-f-1
title: Verify loader-library hashes via .sha1 sibling files
created: "2026-06-09"
origin: |
    docs/spec/fabric-quilt-launch.md, CP2 (resolver::merge_loader_profile)
kind: plan
severity: risk
review_by: "2026-08-08"
status: open
file: src-tauri/src/core/resolver.rs
---

Fabric and Quilt loader profiles carry no sha1/size for their libraries, so loader jars currently download with `expected_hash: None` (HTTPS transport integrity only). Fabric publishes `.sha1` sibling files on its maven (e.g. `<jar-url>.sha1`); fetch them to populate `ExpectedHash::Sha1` on the loader `DownloadItem`s in `merge_loader_profile`. One extra request per loader lib. Adds end-to-end hash verification for loader artifacts, matching the vanilla path. Origin: docs/spec/fabric-quilt-launch.md non-goal + design Open questions; merge built in resolver.rs::merge_loader_profile.
