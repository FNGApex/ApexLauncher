---
id: download-cp4-concurrency-failure
title: cp4_concurrency_bound_not_exceeded fails deterministically (mock race)
created: "2026-06-12"
origin: |
    pre-existing; surfaced during storage-auth-reorg verification
kind: finding
severity: risk
review_by: "2026-08-11"
status: open
file: src-tauri/src/core/download.rs:1555
---

Pre-existing test failure, NOT introduced by the storage-auth-reorg branch — confirmed failing 3/3 at base df5c372. Earlier session commits labeled it a "flake" but it fails deterministically on this machine (5/5 in isolation).

Symptom: `core::download::tests::cp4_concurrency_bound_not_exceeded` panics at download.rs:1555 — `expected Ok, got Skipped for http://127.0.0.1:<port>/item0`. Likely a TcpListener mock port-binding / scheduling race in the concurrency-bound assertion: one item is Skipped instead of downloaded.

Fix direction: make the mock server bind deterministically / stabilize the concurrency assertion so it does not depend on host scheduling speed.
