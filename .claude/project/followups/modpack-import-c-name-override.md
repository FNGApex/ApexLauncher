---
id: modpack-import-c-name-override
title: install_modpack name_override=None; pack version name unused
created: "2026-06-17"
origin: |
    docs/spec/modpack-import.md, slice C iter 3-4 reviewer
kind: finding
severity: risk
review_by: "2026-08-16"
status: open
file: src-tauri/src/lib.rs:1551
---

install_modpack passes name_override=None; the pack version-API name (ProjectVersion.name) is never used because resolve_pack_file returns only file_name. In practice the embedded pack manifest name takes over, so behavior is acceptable. Proper fix: widen C2 ResolvedPackFile/resolver to carry the version name, then pass it (else file_name stem) as name_override. Deferred to slice D.
