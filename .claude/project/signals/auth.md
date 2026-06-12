# auth

## What it does

Implements Microsoft OAuth 2.0 device-code flow → Xbox Live chain → Minecraft identity, with multi-account persistence (non-secret metadata in `accounts.json`, refresh tokens in the OS keyring). At launch time, the stored refresh token is used to re-derive a fresh MC access token via `refresh_ms_token` → `xbox_chain` without prompting the user.

## Artifacts

- `src/routes/Accounts.tsx` — fully implemented accounts page: device-code sign-in modal (subscribes to `auth://device-code` event before invoking `beginLogin`), account list with active indicator, set-active and remove buttons, cancel in-flight login, per-action loading/error states via TanStack Query mutations
- `src/lib/ipc.ts` (auth section, lines 339–419) — `AccountMeta`, `DeviceCodePayload`, `AuthCommandError` interfaces; `AUTH_DEVICE_CODE_EVENT` constant; `listenDeviceCode`, `beginLogin`, `cancelLogin`, `listAccounts`, `removeAccount`, `setActiveAccount` wrappers

## CLI code

- `src-tauri/src/core/auth.rs` (1793L) — full auth implementation:
  - `request_device_code`, `poll_token_once`, `refresh_ms_token` — MS OAuth2 device-code + poll + refresh (CP1)
  - `xbox_chain` — XBL authenticate → XSTS authorize → MC `login_with_xbox` → MC profile GET (CP2)
  - `AccountStore` — multi-account store: `load`, `new_empty`, `add_account`, `list_accounts`, `remove_account`, `set_active_account`, `get_active_account`, `get_refresh_token` (CP3)
  - `AccountMeta` — serializable non-secret metadata (`id`, `username`, `xuid`, `mc_token_expires`); `serde(rename_all = "camelCase")` for IPC, with `alias = "mc_token_expires"` so pre-rename accounts.json files still load
  - `KeyringBackend` trait + `SystemKeyringBackend` (production, backed by `keyring` crate) — injectable seam so tests never call the real OS keyring
  - `AuthHttpClient` trait + `ReqwestAuthClient` (production) — injectable HTTP seam so all tests use mock responses
  - `AuthError` enum — 11 named variants covering device-code expiry, authorization decline, XSTS XErr codes (2148916233 / 2148916235 / 2148916238), no Minecraft license (profile 404), keyring failure, store I/O failure
  - 44 unit tests (all mock HTTP, no live TCP; all keyring tests use `FakeKeyring` or `FailingKeyring` in-process)
- `src-tauri/src/lib.rs` (auth section, lines 16–228) — Tauri command layer:
  - `begin_login` — long-lived async command: requests device code, emits `auth://device-code` event, runs poll loop with cancel-token check, runs Xbox chain, persists account with MS refresh token in keyring, returns `AccountMeta`
  - `cancel_login` — fires a oneshot to abort the in-flight `begin_login` poll loop; no-op if not running
  - `list_accounts`, `remove_account`, `set_active_account` — thin wrappers over `AccountStore` under a `tokio::Mutex`
  - `AuthCommandError` — wraps `AuthError` as `{kind: String, message: String}` so non-`Serialize` error variants (e.g. `reqwest::Error`) cross the Tauri IPC boundary
  - `SharedAccountStore = Arc<tokio::sync::Mutex<AccountStore>>` — registered as Tauri managed state in `run()`; cancel token `CancelToken = std::sync::Mutex<Option<oneshot::Sender<()>>>` also managed state
- `src-tauri/src/core/store.rs` — `accounts_file()` path helper: resolves `<appdata>/accounts.json`, ensures parent dir exists (added alongside other path helpers in CP3)
- `src-tauri/src/core/launch.rs` — `resolve_launch_identity`: offline flag → offline; no active account → offline; active account → retrieve stored refresh token from keyring, run `refresh_ms_token` + `xbox_chain`, update store with refreshed metadata, return `LaunchIdentity`; `LaunchIdentity` struct (`player_name`, `uuid`, `access_token`, `xuid`, `user_type`)

## Docs

- `docs/spec/authentication.md` — implementation contract: goal, non-goals, success criteria (12 criteria), checkpoints CP1–CP5 with file ranges and verification conditions, risks table, open questions, change log
- `docs/design/authentication.md` — design doc: problem statement, goals/non-goals, token-chain diagram, approach comparisons (token storage A1–A3, UX threading B1–B2, HTTP client C1–C2), chosen approach rationale, checkpoint table, risk table
- `docs/design/auth-client-id-blocker.md` — ongoing blocker log: AADSTS700016 root cause, Azure app registration steps, Mojang `login_with_xbox` approval gate (form at `aka.ms/mce-reviewappid`, submitted 2026-06-09, approved 2026-06-11), test plan for post-approval verification

## Coupling

- **launch domain:** `resolve_launch_identity` in `launch.rs` depends directly on `AccountStore`, `AuthHttpClient`, `refresh_ms_token`, `xbox_chain` from `auth.rs`; `LaunchIdentity` is defined in `launch.rs` but populated by the auth chain — adding fields to `AccountMeta` (e.g. new token fields) requires coordinating both modules. `launch_instance` command in `lib.rs` holds the `SharedAccountStore` lock across the `resolve_launch_identity` async call.
- **frontend-shell / ipc.ts:** `AccountMeta`, `DeviceCodePayload`, `AuthCommandError` interfaces and all five auth IPC wrappers are hand-mirrored in `ipc.ts`; no type generation — any rename in `auth.rs` or `lib.rs` requires manual `ipc.ts` update.
- **instances / settings domain:** `Settings.offline_mode` (added in CP4 to `settings.rs`) governs whether `resolve_launch_identity` returns offline identity even when an active account is present.
- **store.rs:** `accounts_file()` was added to `core/store.rs` as part of CP3; the auth domain owns this path helper's usage, but `store.rs` is shared infrastructure.

## Conventions worth knowing

- Azure `client_id` defaults to `82a79499-8c2e-49b8-9e42-1dd9d56252f2` — registered modloader Azure app GUID (constant `DEFAULT_MS_CLIENT_ID` in `auth.rs:22`). Can be overridden at runtime via `MODLOADER_MS_CLIENT_ID` env var (resolved by `ms_client_id()` in `auth.rs:28`). Mojang approved this client ID 2026-06-11 — `login_with_xbox` 403 gate cleared; only end-to-end re-test remains (see `docs/design/auth-client-id-blocker.md`).
- Refresh token stored in keyring under key `account_id` (Minecraft UUID), service name `"modloader"` (constant `KEYRING_SERVICE`). MC access token and MS refresh token are never written to `accounts.json`.
- `AccountStore` is not thread-safe internally; callers serialize access via `tokio::sync::Mutex<AccountStore>` in Tauri managed state.
- `remove_account` ordering (F-10 fix): keyring delete happens first; if it fails, in-memory state and disk are left unchanged — no desync.
- `poll_token_once`: MS token endpoint uses HTTP 400 for error states (`authorization_pending`, `expired_token`, `access_denied`) — these are parsed as poll responses. Any other non-200/400 status is returned as `HttpStatus` error without parsing.
- `begin_login` cancel: oneshot sender placed in `CancelToken` managed state before the poll loop; `cancel_login` `take()`s the sender. If the receiver is already gone (login completed), the send is silently dropped.
- All auth tests: 44 unit tests in `auth.rs`, split across CP1 (device-code/poll/refresh), CP2 (Xbox chain + XSTS XErr mapping), CP3 (AccountStore round-trips with `FakeKeyring`/`FailingKeyring`/`StoreOkDeleteFailKeyring`), CP4 (refresh-at-launch mock sequence), CP5 (`ms_client_id_from` default, env-override, blank-override cases). No real TCP connections in any test.
- XSTS `DisplayClaims.xui[0]` field for Xbox user ID is named `xid` (not `xuid`) — verified by fixture test `cp2_xsts_parses_token_and_xuid_from_xid_field`.
