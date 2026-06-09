# Auth blocker: Microsoft sign-in fails with AADSTS700016

**Status:** open blocker. Discovered 2026-06-09 testing the Phase 3 auth flow against a real
Microsoft account. Phase 3 code is otherwise complete; this is a configuration/credential gap,
not a logic bug.

## Symptom

"Add a Microsoft account" → device-code request returns HTTP 400:

```
AADSTS700016: Application with identifier '00000000402b5328' was not found in the
directory 'Microsoft Accounts'. ... error: unauthorized_client
```

## Root cause

`MS_CLIENT_ID = "00000000402b5328"` (`src-tauri/src/core/auth.rs:18`) is the **legacy Minecraft
launcher client ID**, registered in the `login.live.com` MSA system for the **redirect-based
auth-code flow** (redirect URI `https://login.live.com/oauth20_desktop.srf`, scope
`service::user.auth.xboxlive.com::MBI_SSL`).

The code points that ID at the **Azure AD v2.0 device-code endpoint**
(`login.microsoftonline.com/consumers/oauth2/v2.0/devicecode`, `auth.rs:21-22`). The consumers
AAD tenant has no record of that app, so it rejects it with 700016.

Device-code flow is an AAD v2.0 feature. The legacy client ID only exists in `login.live.com`
and only supports the redirect flow. **The two halves are incompatible** — not an endpoint typo.
This is why PrismLauncher / MultiMC each register and ship their *own* Azure app ID.

```
code:     client_id=00000000402b5328  →  AAD v2.0 /consumers/devicecode
reality:  that ID lives in login.live.com (redirect flow only), not AAD  →  700016
```

The comment at `auth.rs:15` ("Official public Minecraft launcher client_id ... also used by
PrismLauncher") is **wrong** for this flow and should be corrected. This realizes the risk the
spec already flagged (`docs/spec/authentication.md:105`) and disproves the design assumption
(`docs/design/authentication.md:39` — "no per-user Azure registration").

## Fix (chosen approach)

Register an own Azure AD application and use its client ID. The endpoints + scope in `auth.rs`
are already correct for device code; only the client ID is wrong.

1. Azure Portal → **App registrations** → **New registration**.
2. Name `modloader`. **Supported account types: Personal Microsoft accounts only**.
3. Skip redirect URI. Create.
4. **Authentication** → Advanced settings → **Allow public client flows → Yes** (enables device
   code). Save.
5. Copy **Application (client) ID** (a GUID).
6. Set it as `MS_CLIENT_ID` (`auth.rs:18`). Keep `/consumers/...devicecode`, `/token`, and scope
   `XboxLive.signin offline_access` unchanged.

No client secret (public client). Personal MS accounts sign in via the consumers tenant — what
the code already targets.

### Rejected alternative

Switch to the legacy `login.live.com` auth-code flow to keep `00000000402b5328`. Rejected: needs
a loopback/webview redirect handler, abandons the device-code UX, and is a much larger rewrite of
a working flow. The client ID is the cheap thing to change, not the flow.

## Resume checklist (next session)

- [ ] Register the Azure app (steps above); obtain the client-ID GUID.
- [ ] Wire `MS_CLIENT_ID`: read from env / build config with a constant fallback, so the per-deploy
      ID isn't a hardcoded literal and other devs/CI can override. Paste the GUID into the local
      config.
- [ ] Fix the misleading comment at `auth.rs:15`.
- [ ] Correct `docs/spec/authentication.md` risk row + `docs/design/authentication.md:39`
      assumption (per the spec change-log rule).
- [ ] Re-test: add a real Microsoft account end to end; then launch online.

## Sources

- wiki.vg — Microsoft Authentication Scheme: https://wiki.vg/Microsoft_Authentication_Scheme
- minecraft-launcher-lib — Microsoft Login (consumers tenant, `XboxLive.signin`):
  https://minecraft-launcher-lib.readthedocs.io/en/stable/tutorial/microsoft_login.html
- Microsoft Entra error codes (AADSTS700016):
  https://learn.microsoft.com/en-us/entra/identity-platform/reference-error-codes
