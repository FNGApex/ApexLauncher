---
id: mod-install-f-6
title: enable/disable rename and manifest write are not atomic
created: "2026-06-15"
origin: |
    docs/spec/mod-install.md, iter 3 reviewer (CP-3)
kind: finding
severity: risk
review_by: "2026-08-14"
status: open
file: src-tauri/src/core/instances.rs:403
---

set_mod_enabled_on_disk renames the jar then writes the manifest non-atomically (instances.rs:~403). If the FS rename succeeds but write_manifest fails, ModEntry.enabled and the on-disk .disabled suffix diverge. Self-heals on next get() because scan_mods treats disk as truth for disabled-state, but the manifest flag can lag until then. No atomic FS primitive available; consider a reconcile pass or documenting disk-as-truth explicitly.
