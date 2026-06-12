# Auth blocker: Microsoft sign-in fails with AADSTS700016

**Status:** resolved pending final verification. Discovered 2026-06-09 testing the Phase 3 auth
flow against a real Microsoft account. Mojang approved the registered client ID
`82a79499-8c2e-49b8-9e42-1dd9d56252f2` on 2026-06-11 — the `login_with_xbox` 403 gate is cleared.
Phase 3 code was already complete and wired; only the async approval remained. Last open item: an
end-to-end re-test (add a real account, launch online). This was a configuration/credential gap,
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

## Second gate: Mojang approval of the new client ID (found 2026-06-09)

Registering the Azure app fixes AADSTS700016, but it is not the last gate. Mojang restricts
which client IDs may call the Minecraft services API (`api.minecraftservices.com` —
`login_with_xbox`, profile). A newly created Azure app gets **HTTP 403 at the
`login_with_xbox` step** until Mojang approves it via the review form at
https://aka.ms/mce-reviewappid. The OAuth + Xbox chain up to XSTS works without approval,
so the device-code flow itself can be smoke-tested before the form clears.

Consequence: submit the form immediately after registering the app — approval is async
(community reports days to weeks) and the end-to-end test stays blocked until it lands.

## Resume checklist

- [x] Register the Azure app (steps above); obtain the client-ID GUID. *(done 2026-06-09:
      `82a79499-8c2e-49b8-9e42-1dd9d56252f2`)*
- [x] Submit the Mojang app-review form (https://aka.ms/mce-reviewappid) with that GUID;
      until approved, expect 403 at `login_with_xbox` and treat it as pending, not a bug.
      *(submitted 2026-06-09; **approved 2026-06-11** — 403 gate cleared)*
- [x] Wire the client ID: `DEFAULT_MS_CLIENT_ID` const + `ms_client_id()` with
      `MODLOADER_MS_CLIENT_ID` env override (`auth.rs`). *(done 2026-06-09)*
- [x] Fix the misleading comment at `auth.rs:15`. *(done 2026-06-09)*
- [x] Correct `docs/spec/authentication.md` risk row + `docs/design/authentication.md`
      decision row (per the spec change-log rule). *(done 2026-06-09)*
- [ ] Re-test: add a real Microsoft account end to end (device-code + Xbox chain should pass;
      `login_with_xbox` 403 = Mojang approval still pending); after approval, launch online.

## Sources

- wiki.vg — Microsoft Authentication Scheme: https://wiki.vg/Microsoft_Authentication_Scheme
- minecraft-launcher-lib — Microsoft Login (consumers tenant, `XboxLive.signin`):
  https://minecraft-launcher-lib.readthedocs.io/en/stable/tutorial/microsoft_login.html
- Microsoft Entra error codes (AADSTS700016):
  https://learn.microsoft.com/en-us/entra/identity-platform/reference-error-codes
- Entra app registration quickstart:
  https://learn.microsoft.com/en-us/entra/identity-platform/quickstart-register-app
- Minecraft Wiki — Microsoft authentication (approval form, consumers tenant):
  https://minecraft.wiki/w/Microsoft_authentication
- Mojang app-review form: https://aka.ms/mce-reviewappid
