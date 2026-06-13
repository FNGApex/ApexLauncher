# auth

## What it does

Implements Microsoft OAuth 2.0 device-code flow → Xbox Live chain → Minecraft identity, with single-account persistence (non-secret metadata in `account.json`, MS refresh token in the OS keyring). At launch time, the stored refresh token re-derives a fresh MC access token via `refresh_ms_token` → `xbox_chain` without prompting the user. Multi-account support was removed in the storage-auth-reorg branch.

## Artifacts

- `src/components/Sidebar.tsx` — inline login/logout control embedded in the sidebar: queries `getAccount`, shows Login button (triggers device-code flow, displays code + verification URI, cancel in-flight), and Logout button; subscribes to `auth://device-code` event before invoking `beginLogin`; account display name shown when logged in
- `src/lib/ipc.ts` (auth section) — `AccountMeta`, `DeviceCodePayload`, `AuthCommandError` interfaces; `AUTH_DEVICE_CODE_EVENT` constant; `listenDeviceCode`, `beginLogin`, `cancelLogin`, `getAccount`, `logout` wrappers

## CLI code

- `src-tauri/src/core/auth.rs` (1804L) — full auth implementation:
  - `request_device_code`, `poll_token_once`, `refresh_ms_token` — MS OAuth2 device-code + poll + refresh (CP1)
  - `xbox_chain` — XBL authenticate → XSTS authorize → MC `login_with_xbox` → MC profile GET (CP2)
  - `AccountStore` — single-account store: `load`, `new_empty`, `set_account`, `get_account`, `logout`, `get_refresh_token` (CP3)
  - `AccountMeta` — serializable non-secret metadata (`id`, `username`, `xuid`, `mc_token_expires`); `serde(rename_all = "camelCase")`
  - `KeyringBackend` trait + `SystemKeyringBackend` (production, backed by `keyring` crate) — injectable seam so tests never call the real OS keyring
  - `AuthHttpClient` trait + `ReqwestAuthClient` (production) — injectable HTTP seam; all tests use mock responses
  - `AuthError` enum — 11 named variants covering device-code expiry, authorization decline, XSTS XErr codes (2148916233 / 2148916235 / 2148916238), no Minecraft license (profile 404), keyring failure, store I/O failure
  - 40 unit tests (all mock HTTP via `MockAuthClient` VecDeque; keyring via `FakeKeyring`/`FailingKeyring` — no real TCP, no OS keyring)
- `src-tauri/src/lib.rs` (auth section, lines ~1–228) — Tauri command layer:
  - `begin_login` — long-lived async command: requests device code, emits `auth://device-code` event, runs poll loop with cancel-token check, runs Xbox chain, persists account with MS refresh token in keyring, returns `AccountMeta`
  - `cancel_login` — fires a oneshot to abort the in-flight `begin_login` poll loop; no-op if not running
  - `get_account` — returns `Option<AccountMeta>` from the shared store; `None` when not logged in
  - `logout` — clears `account.json` and the keyring entry
  - `AuthCommandError` — wraps `AuthError` as `{kind: String, message: String}` for Tauri IPC serialization
  - `SharedAccountStore = Arc<tokio::sync::Mutex<AccountStore>>` — registered as Tauri managed state; `CancelToken = std::sync::Mutex<Option<oneshot::Sender<()>>>` also managed state
- `src-tauri/src/core/store.rs` — `account_file()` path helper: resolves `<data>/account.json`, ensures parent dir exists; `cache_dir` + cache subdir helpers (`cache_assets_dir`, `cache_libraries_dir`, `cache_versions_dir`, `cache_java_dir`, `cache_meta_dir`, `cache_installers_dir`) added in storage-auth-reorg
- `src-tauri/src/core/launch.rs` — `resolve_launch_identity`: offline flag → offline; no account → offline; account present → retrieve stored refresh token from keyring, run `refresh_ms_token` + `xbox_chain`, update store with refreshed metadata, return `LaunchIdentity`

## Docs

- `docs/spec/authentication.md` — implementation contract: goal, non-goals, success criteria (12 criteria), checkpoints CP1–CP5 with file ranges and verification conditions, risks table, change log
- `docs/design/authentication.md` — design doc: problem statement, goals/non-goals, token-chain diagram, approach comparisons, chosen approach rationale, checkpoint table, risk table
- `docs/design/auth-client-id-blocker.md` — AADSTS700016 root cause, Azure app registration steps, Mojang `login_with_xbox` approval gate (approved 2026-06-11); test plan for post-approval verification
- `docs/design/storage-auth-reorg.md` — design doc for the storage/auth reorganization: single-account simplification, path consolidation under `ApexLauncher/`, cache layout

## Coupling

- **launch domain:** `resolve_launch_identity` in `launch.rs` depends on `AccountStore`, `AuthHttpClient`, `refresh_ms_token`, `xbox_chain` from `auth.rs`; adding fields to `AccountMeta` requires coordinating both modules. `launch_instance` command in `lib.rs` holds the `SharedAccountStore` lock across the `resolve_launch_identity` async call.
- **frontend-shell / ipc.ts:** `AccountMeta`, `DeviceCodePayload`, `AuthCommandError` interfaces and the four auth IPC wrappers are hand-mirrored in `ipc.ts`; no type generation — any rename in `auth.rs` or `lib.rs` requires manual `ipc.ts` update. Login/logout UI moved from `Accounts.tsx` into `Sidebar.tsx`.
- **instances / settings domain:** `Settings.offline_mode` in `settings.rs` governs whether `resolve_launch_identity` returns offline identity even when an account is present.

## Conventions worth knowing

- Azure `client_id` defaults to `82a79499-8c2e-49b8-9e42-1dd9d56252f2` (`DEFAULT_MS_CLIENT_ID` in `auth.rs:22`). Overridable via `MODLOADER_MS_CLIENT_ID` env var (`ms_client_id()` in `auth.rs:28`). Mojang approved this client ID 2026-06-11.
- Refresh token stored in keyring under key `account_id` (Minecraft UUID), service name `"modloader"` (constant `KEYRING_SERVICE`). MC access token and MS refresh token are never written to `account.json`.
- `AccountStore` is not thread-safe internally; callers serialize via `tokio::sync::Mutex<AccountStore>` in Tauri managed state.
- `poll_token_once`: MS token endpoint uses HTTP 400 for `authorization_pending`/`expired_token`/`access_denied` — parsed as poll responses; any other non-200/400 is `HttpStatus` error.
- `begin_login` cancel: oneshot sender placed in `CancelToken` before the poll loop; `cancel_login` `take()`s it. If receiver already gone (login completed), send is silently dropped.
- XSTS `DisplayClaims.xui[0]` field for Xbox user ID is named `xid` (not `xuid`) — verified by fixture test `cp2_xsts_parses_token_and_xuid_from_xid_field`.
