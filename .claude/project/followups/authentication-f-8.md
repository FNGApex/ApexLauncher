---
id: authentication-f-8
title: Injectable auth chain URLs for Phase 7 integration coverage
created: "2026-06-11"
origin: |
    authentication loop iter-2 reviewer (F-8)
kind: plan
review_by: "2026-08-10"
status: open
file: src-tauri/src/core/auth.rs
---

xbl_authenticate/xsts_authorize/mc_login/mc_get_profile use hardcoded URL constants; the mock ignores _url, so tests verify behavior but not URL routing. Consistent with download.rs, but no integration test can redirect these endpoints. Widen the seam when Phase 7 integration tests land.
