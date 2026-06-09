---
id: auth-client-id-aadsts700016
title: 'MS sign-in blocked: bundled client_id invalid for device-code flow (AADSTS700016)'
created: "2026-06-09"
origin: |
    manual auth test, 2026-06-09
kind: finding
severity: risk
review_by: "2026-08-08"
status: open
file: docs/design/auth-client-id-blocker.md
---

Microsoft sign-in (Phase 3 device-code flow) fails at runtime with AADSTS700016: the bundled `MS_CLIENT_ID=00000000402b5328` (auth.rs:18) is a legacy login.live.com client (redirect/auth-code flow only) and is not registered in the AAD v2.0 consumers tenant that the code's device-code endpoint targets — the two are incompatible. Fix: register an own Azure AD app (Personal Microsoft accounts only; Allow public client flows = Yes), use its client-ID GUID; endpoints + scope already correct. Full diagnosis, registration steps, rejected alternative, and resume checklist in docs/design/auth-client-id-blocker.md. Realizes the risk flagged at spec authentication.md:105.
