---
id: storage-auth-reorg-c2
title: Wire materialization into launch (slice C2)
created: "2026-06-12"
origin: |
    docs/spec/storage-auth-reorg.md, slice C2 (deferred at /subagent-implementation pause)
kind: plan
review_by: "2026-08-11"
status: open
file: docs/spec/storage-auth-reorg.md
---

Slice C2 of the storage-auth-reorg spec, deferred at the /subagent-implementation pause after C1. Slices A (rebrand+cache layout), B (single-account), and C1 (materialize helper) shipped; C2 wires materialization into the live launch path.

Scope (see docs/spec/storage-auth-reorg.md checkpoint C2):
- Before JVM spawn, materialize the resolved plan's libraries + version jars from cache/ into instances/<slug>/ via core::materialize (hardlink + copy fallback).
- Rewrite launch_meta.classpath and ${library_directory} to point at the instance paths.
- Keep assets shared: --assetsDir stays cache/assets (Recommendation A in design doc).
- Loader installs into cache/ then materializes into the instance.

Folded dependent risks from C1 review:
- F-4: cache_assets_dir/cache_libraries_dir/cache_versions_dir/cache_installers_dir are dead-code until C2 wires them (store.rs). C2 should use them.
- F-6: materialize copy-fallback triggers on ANY io::Error, not just EXDEV (materialize.rs:76). Tighten when it runs in prod.
- F-7: materialize idempotency skips by exists() without content check (materialize.rs:61). Fine for content-stable versioned maven artifacts; revisit if copy-fallback truncation is possible.
