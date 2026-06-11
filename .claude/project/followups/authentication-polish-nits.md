---
id: authentication-polish-nits
title: 'Auth polish nits: mock queue underrun (F-3), XSTS 401 fallback diagnostic (F-9), KeyringBackend not-found doc (F-11)'
created: "2026-06-11"
origin: |
    authentication loop reviewers (F-3/F-9/F-11)
kind: finding
severity: nit
review_by: "2026-08-10"
status: open
file: src-tauri/src/core/auth.rs
---

F-3: MockAuthClient panics on canned-response queue underrun inside async tests — may hang instead of failing cleanly; consider returning Err. F-9: map_xsts_error falls back to HttpStatus{401,body} on unparseable XErr body — no distinct diagnostic. F-11: KeyringBackend trait lacks a doc comment pinning not-found semantics (Keyring error on load, idempotent on delete) so impls stay symmetric.
