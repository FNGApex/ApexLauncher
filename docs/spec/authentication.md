# Authentication (Phase 3)

## Goal

Replace the hardcoded offline identity in `core/launch.rs` with a real Microsoft
OAuth 2.0 device-code flow that produces a genuine Minecraft access token + profile
UUID, persists refresh tokens in the OS keychain, and threads the active account's
identity into argv at launch time. Offline mode is retained behind a setting.

## Non-goals

- Embedded webview / authorization-code-with-PKCE flow. Device code only.
- Skin/cape rendering or account avatars beyond the MC profile response.
- Per-instance account binding. One global active account only.
- Mojang legacy (pre-Microsoft) account migration.
- Pre-1.7 `assets_legacy` materialization (deferred — `vanilla-launch-f-1/-f-2`).
- Enforcement of online mode per server / per instance.

## Success criteria

- [ ] `begin_login` Tauri command emits an `auth://device-code` event carrying `user_code`
      and `verification_uri` before the first token-poll attempt (mock HTTP, no live network).
- [ ] Polling resolves with a full `Account` struct (id, username, xuid,
      mc_token_expires) when the mock MS token endpoint returns `access_token`.
- [ ] Polling surfaces distinct error variants for `expired_token`,
      `authorization_declined`/`access_denied`, and XSTS error codes 2148916233,
      2148916235, 2148916238.
- [ ] `cancel_login` cancels an in-flight `begin_login` without panic or deadlock.
- [ ] Xbox chain (XBL → XSTS → MC token → profile) is exercised by fixture tests
      that never open a real TCP connection.
- [ ] `accounts.json` round-trips: add → list → remove → list produces the correct
      entries; `active_account_id` tracks `set_active_account` calls.
- [ ] The refresh token is stored/retrieved via an injectable keyring seam, not in
      `accounts.json`. Tests supply a fake backend; the real keyring is never called
      in any test.
- [ ] At launch when the active account is online (offline setting false), the assembled
      argv contains the active account's `username`, `uuid`, and `access_token` string —
      not the hardcoded constants at `core/launch.rs:150-152`. Verified by asserting the
      argv output of `build_argv`, not merely the inputs passed to it.
- [ ] At launch when the offline setting is true, `build_argv` continues to use
      `OFFLINE_PLAYER_NAME` + `offline_uuid()` + token `"0"`.
- [ ] MC token refresh path: given a stored refresh token and an expired MC token, the
      launch path re-derives a fresh MC token via `grant_type=refresh_token` without
      prompting the user (fixture/mock test, no live HTTP).
- [ ] `keyring` backend unavailable (injected fake that always errors) surfaces a named
      error variant — not a fallback to offline identity, not a panic.
- [ ] `npm run build` (tsc) passes with `Accounts.tsx` wired (no TS errors, no `TODO`
      stubs that reference non-existent IPC symbols).
- [ ] `cargo test` passes green with no new `#[ignore]` on auth tests.

## Approaches

### A. Token-storage strategy

| # | Approach | Pros | Cons |
|---|----------|------|------|
| A1 | `keyring` for refresh token + `accounts.json` for metadata *(chosen)* | Matches ARCHITECTURE §7; metadata listable without keychain unlock; secret never on disk | `keyring` Linux/WSL needs secret-service daemon; tests must inject a fake backend |
| A2 | Everything in an encrypted file (age/ring) | No OS keychain dependency; works headless | We own key management; reinvents what the OS provides; ARCHITECTURE says keyring |
| A3 | Plaintext JSON incl. refresh token | Trivial | Refresh token = long-lived credential in plaintext. Rejected. |

### B. Device-code UX threading

| # | Approach | Pros | Cons |
|---|----------|------|------|
| B1 | Event-driven single command *(chosen)* | Mirrors `download://progress` + `launch://log`; one round-trip; backend owns the poll loop | Command is long-lived; need a cancel path |
| B2 | Two-phase (`start_login` returns code, `poll_login` called by UI) | Stateless commands | UI drives polling — more IPC chatter; duplicates interval logic in TS |

### C. Auth client

| # | Approach | Pros | Cons |
|---|----------|------|------|
| C1 | Hand-rolled chain over `reqwest` *(chosen)* | Consistent with every other `core/` net module; full control over error mapping; testable via existing `TcpListener`/fixture conventions | More code than a crate would be |
| C2 | Third-party MS-MC auth crate | Less code | None maintained to the standard we'd trust; opaque error mapping; new dep surface |

### Recommendation

**A1 + B1 + C1.** Hand-rolled `reqwest` chain in a new `core/auth.rs`, refresh token in
`keyring`, metadata in `accounts.json`, device-code surfaced via an `auth://device-code`
event from a single `begin_login` command. Every choice matches an existing convention:
net-in-Rust, event streaming, fixture/mock tests, ARCHITECTURE §7 keyring commitment.

Hard constraint: no live HTTP in any test — follow `download.rs` (`TcpListener` mock) and
`java.rs` (fixtures + injected closure). Keyring access and HTTP chain must be behind
injectable seams.

## Checkpoints

| # | Checkpoint | Files / areas | Agent | Est. files | Verifies |
|---|------------|---------------|-------|------------|---------|
| 1 | MS device-code + poll + refresh | `src-tauri/src/core/auth.rs` (new); `src-tauri/src/core/mod.rs:14-23` (`pub mod auth`); `src-tauri/Cargo.toml` (`keyring` dep) | atomic-builder | 3 | `cargo test` — all poll states from the design error taxonomy + token refresh, all via mock HTTP; no live connections |
| 2 | Xbox chain + error mapping | `src-tauri/src/core/auth.rs` (extend) | atomic-builder | 1 | `cargo test` — XBL → XSTS → MC token → profile fixture tests; XSTS error codes 2148916233 / 2148916235 / 2148916238 map to named variants; profile 404 maps to "no Minecraft license" |
| 3 | Account store (persistence + keyring seam) | `src-tauri/src/core/auth.rs` (extend); `src-tauri/src/core/store.rs:14-35` (`accounts_file()` path helper) | atomic-builder | 2 | `cargo test` — `accounts.json` round-trip (add/list/remove/active-selection) using a TempDir; keyring store/load via injectable fake; no real keyring in any test |
| 4 | Tauri commands + argv wiring | `src-tauri/src/lib.rs:363-379` (register 5 new commands); `src-tauri/src/core/launch.rs:116-155` (`build_argv` substitution at lines 150-152 reads active account; offline guard) | atomic-builder | 2 | `cargo test` — existing `build_argv` tests green; new online/offline identity-routing tests assert the assembled argv contains the correct username, uuid, and access-token string |
| 5 | Frontend: Accounts UI + IPC types | `src/routes/Accounts.tsx:1-22` (replace stub); `src/lib/ipc.ts:226-336` (add `Account`, `AuthEvent` types + `beginLogin`/`cancelLogin`/`listAccounts`/`removeAccount`/`setActiveAccount` wrappers) | atomic-builder | 2 | `npm run build` — tsc clean; device-code modal renders, account list/add/remove/switch wired to commands |

Fixture JSON files are counted in Est. files; CP2 adds XBL/XSTS/MC/profile fixtures, reused by CP3.

## Risks

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| `keyring` backend unavailable on Linux / WSL (no secret-service daemon) | High in dev environment | Injectable seam ensures tests never call the real keyring; at runtime, surface a clear error and fail loud — no silent plaintext fallback. Document the limitation for WSL users. |
| Live HTTP accidentally used in a test | Medium (new contributor path) | Enforce via `TcpListener` mock or fixture injection for every HTTP call in `auth.rs`. If a test opens a real connection the CI integration-test policy blocks it; add a comment at the top of the test module. |
| Token refresh races at launch (two concurrent launches refresh simultaneously) | Low | Account store update after refresh must be atomic (file write + in-memory); a simple per-account mutex or a serialized refresh future is sufficient. Spec does not prescribe the primitive. |
| Mojang approval of our client_id pending | Certain until form clears (submitted 2026-06-09) | `login_with_xbox` returns 403 for unapproved client IDs. Treat 403 there as "approval pending", not a code bug. Default client ID is the registered `modloader` Azure app (`DEFAULT_MS_CLIENT_ID`, env-overridable via `MODLOADER_MS_CLIENT_ID`); see docs/design/auth-client-id-blocker.md. |
| XSTS `xuid` field name inconsistency (documented as `xid` in some references) | Medium | Verified by fixture tests covering the actual JSON shape from a real response snapshot; builder must confirm field name against a captured response fixture before parsing. |

## Open questions

1. **keyring on dev / WSL:** the dev environment is WSL, which often lacks a
   secret-service daemon. Does `begin_login` need to succeed in dev, or is failing-loud
   acceptable (real use is desktop Win / macOS / Linux with a keychain)? Leaning: fail
   loud in dev, document the limitation; no plaintext fallback.

2. **MC token caching:** persist the short-lived MC access token across launches (skips
   a refresh round-trip) or always refresh on expiry? Leaning: do not persist — always
   refresh if expired; simpler, and the refresh round-trip is fast.

3. **xuid source:** XSTS `DisplayClaims.xui[0].xid` vs. decoding the MC token JWT.
   Leaning: XSTS claims (already parsed at that stage, no JWT decode dependency).

## Change log

### 2026-06-09 — Own Azure app client ID; env override

**What changed**

- `MS_CLIENT_ID` const replaced by `DEFAULT_MS_CLIENT_ID` (`82a79499-8c2e-49b8-9e42-1dd9d56252f2`,
  the registered `modloader` Azure app: consumers tenant, personal accounts only, public client
  flows enabled) + `ms_client_id()` resolver honoring a `MODLOADER_MS_CLIENT_ID` env override.
- Risk row rewritten: the "client_id validity" risk realized as AADSTS700016 — the legacy
  official-launcher id exists only in login.live.com (redirect flow) and is rejected by the
  AAD v2.0 device-code endpoint. New residual risk: Mojang approval of the new app id pending
  (form aka.ms/mce-reviewappid); `login_with_xbox` 403s until it clears.

**Why**

Real-account testing of the Phase 3 flow failed at the device-code request. Full diagnosis in
docs/design/auth-client-id-blocker.md.

**Superseded:** "ship the official public launcher client_id as a constant; revocation is a
one-line swap" — that id never worked for the device-code flow.

### 2026-06-09 — CP5 review fixes: active-account-id exposure + cast removal

**What changed**

- Added a `get_active_account_id` Tauri command (`-> Option<String>`) backed by a new
  `AccountStore::active_account_id()` accessor. `Accounts.tsx` now seeds the active-account
  indicator from this command on mount instead of tracking it in local React state.
- `Accounts.tsx` `extractMessage` no longer casts the error to `Record<string, unknown>`;
  it narrows via the `in` operator (project "Never cast" rule).
- Renamed test `cp4_f10_remove_account_keyring_failure_leaves_state_unchanged` →
  `cp3_…`; it exercises CP3 store behavior, not CP4.

**Why**

The active-account indicator was wrong on mount: `list_accounts` returns no active id, so
the UI showed the wrong row as active until the user clicked. CP4 registered 5 commands;
this adds a 6th for the active id. Found by the signals scan (`accounts-active-id-mount`
risk, `accounts-extractmessage-cast` + `auth-test-cp4-misnamed` nits).
