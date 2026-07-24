# auth

## Overview

Microsoft OAuth 2.0 device-code flow → Xbox Live chain → Minecraft identity. Non-secret metadata in `account.json`; MS refresh token in OS keyring. At launch time, the stored refresh token re-derives a fresh MC access token via `refresh_ms_token` → `xbox_chain` without prompting the user. Single-account store; multi-account was removed.

## CLI code

- `src-tauri/src/core/auth.rs` — `request_device_code`, `poll_token_once`, `refresh_ms_token` (MS OAuth2 device-code + poll + refresh); `xbox_chain` (XBL → XSTS → MC `login_with_xbox` → MC profile GET); `AccountStore` (`load`, `new_empty`, `set_account`, `get_account`, `logout`, `get_refresh_token`); `AccountMeta { id, username, xuid, mc_token_expires }` (`serde(rename_all = "camelCase")`); `KeyringBackend` trait + `SystemKeyringBackend` (injectable seam); `AuthHttpClient` trait + `ReqwestAuthClient` (injectable HTTP seam); `AuthError` (11 variants: device-code expiry, authorization decline, XSTS XErr codes 2148916233/2148916235/2148916238, no Minecraft license, keyring failure, store I/O failure); 40 tests in `auth_tests.rs` (all mock HTTP via `MockAuthClient` VecDeque; keyring via `FakeKeyring`/`FailingKeyring`); wired via `#[cfg(test)] #[path = "auth_tests.rs"] mod tests;`; module-scope test scaffolding stays in `auth.rs`
- `src-tauri/src/lib.rs` — `begin_login` (requests device code, emits `auth://device-code`, polls with cancel-token check, runs Xbox chain, persists); `cancel_login` (fires oneshot to abort poll loop); `get_account` (returns `Option<AccountMeta>`); `logout` (clears `account.json` + keyring); `AuthCommandError { kind, message }`; `SharedAccountStore = Arc<tokio::sync::Mutex<AccountStore>>`; `CancelToken = Mutex<Option<oneshot::Sender<()>>>` — both managed state
- `src-tauri/src/core/store.rs` — `account_file()`: `<data>/account.json`, ensures parent dir exists
- `src-tauri/src/core/launch.rs` — `resolve_launch_identity`: offline flag → offline; no account → offline; account present → retrieve refresh token, run `refresh_ms_token` + `xbox_chain`, update store

## Artifacts

- `src/components/Sidebar.tsx` — inline login/logout control: queries `getAccount`, shows Login button (device-code flow, displays code + URI), Logout button; subscribes to `auth://device-code` via `events.authDeviceCode.listen`
- `src/lib/ipc.ts` — `AccountMeta`, `DeviceCodePayload`, `AuthCommandError`; `AUTH_DEVICE_CODE_EVENT`; `listenDeviceCode`, `beginLogin`, `cancelLogin`, `getAccount`, `logout`

## Docs

- `docs/spec/authentication.md` — 12 success criteria, checkpoints CP1–CP5, risks
- `docs/design/auth-client-id-blocker.md` — Azure app registration; Mojang `login_with_xbox` approval (approved 2026-06-11)
- `docs/design/storage-auth-reorg.md` — single-account simplification, path consolidation

## Coupling

- `launch` domain — `resolve_launch_identity` in `launch.rs` depends on `AccountStore`, `AuthHttpClient`, `refresh_ms_token`, `xbox_chain`.
- `instances/settings` domain — `Settings.offline_mode` governs whether `resolve_launch_identity` returns offline identity even when an account is present.

## Conventions

- Azure `client_id`: `82a79499-8c2e-49b8-9e42-1dd9d56252f2` (`DEFAULT_MS_CLIENT_ID`). Overridable via `MODLOADER_MS_CLIENT_ID` env var.
- Refresh token stored in keyring under key = account UUID, service name `"modloader"`. MC access token + MS refresh token never written to `account.json`.
- `poll_token_once`: MS token endpoint uses HTTP 400 for `authorization_pending`/`expired_token`/`access_denied` — parsed as poll responses; any other non-200/400 is `HttpStatus` error.
- `begin_login` cancel: oneshot sender placed in `CancelToken` before poll loop; `cancel_login` `take()`s it. If receiver already gone (login completed), send is silently dropped.
- XSTS `DisplayClaims.xui[0]` Xbox user ID field is named `xid` (not `xuid`).
