---
id: auth-client-id-aadsts700016
title: 'MS sign-in blocked: bundled client_id invalid for device-code flow (AADSTS700016)'
created: "2026-06-09"
origin: |
    manual auth test, 2026-06-09
kind: finding
severity: risk
review_by: "2026-08-08"
status: resolved
resolved: "2026-06-11"
file: docs/design/auth-client-id-blocker.md
---

Microsoft sign-in (Phase 3 device-code flow) failed at runtime with AADSTS700016: the bundled `MS_CLIENT_ID=00000000402b5328` was a legacy login.live.com client (redirect/auth-code flow only), not registered in the AAD v2.0 consumers tenant the device-code endpoint targets. **Fix landed 2026-06-09:** own Azure app registered (`modloader`), `DEFAULT_MS_CLIENT_ID=82a79499-8c2e-49b8-9e42-1dd9d56252f2` + `MODLOADER_MS_CLIENT_ID` env override wired in auth.rs; spec/design docs corrected. **Resolved 2026-06-11:** Mojang approved the registered app (form aka.ms/mce-reviewappid); the `login_with_xbox` 403 gate cleared and online sign-in works. Full trail in docs/design/auth-client-id-blocker.md.
