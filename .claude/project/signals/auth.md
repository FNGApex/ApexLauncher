# auth

## What it does

Implements Microsoft OAuth 2.0 device-code authentication for Minecraft. The full chain is: device-code request → poll until resolved → XBL authenticate → XSTS authorize → MC `login_with_xbox` → MC profile GET → `Account` struct. Multi-account metadata persists in `accounts.json`; the MS refresh token lives in the OS keyring (service name `"modloader"`). MC access tokens are never cached on disk — always re-derived via refresh at launch time.

## Artifacts

- `src/routes/Accounts.tsx` — full accounts UI: "Add account" triggers device-code flow, displays `userCode`+`verificationUri` panel, polls via `beginLogin()`, list/remove/set-active per account row; TanStack Query for list; `activeId` tracked locally (not returned by `listAccounts`)
- `src/lib/ipc.ts` — `AccountMeta`, `DeviceCodePayload`, `AuthCommandError` interfaces; `listenDeviceCode`, `beginLogin`, `cancelLogin`, `listAccounts`, `removeAccount`, `setActiveAccount` wrappers; `AUTH_DEVICE_CODE_EVENT = "auth://device-code"` constant

## CLI code

- `src-tauri/src/core/auth.rs` — all auth core logic: `request_device_code`, `poll_token_once`, `refresh_ms_token`, `xbox_chain` (XBL→XSTS→MC token→profile); `AccountStore` (multi-account persistence with keyring seam); `AccountMeta` (on-disk struct, no secrets); `AuthError` (12 named variants incl. `DeviceCodeExpired`, `AuthorizationDeclined`, `NoXboxAccount`, `XboxRegionBlocked`, `ChildAccount`, `NoMinecraftLicense`, `Keyring`, `Store`); `AuthHttpClient` trait (injectable, used in tests via `MockAuthClient`); `KeyringBackend` trait (injectable, `FakeKeyring` in tests, `SystemKeyringBackend` in prod); 39 unit+async tests, all mock HTTP (no live TCP), no real keyring
- `src-tauri/src/lib.rs` — 5 Tauri commands: `begin_login` (long-lived async, emits `auth://device-code`, polls, runs xbox chain, persists account), `cancel_login` (oneshot channel), `list_accounts`, `remove_account`, `set_active_account`; `AuthCommandError` serializable wrapper (preserves `kind` string for typed frontend handling); `SharedAccountStore = Arc<tokio::sync::Mutex<AccountStore>>` in Tauri managed state; `CancelToken = std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>`
- `src-tauri/src/core/store.rs` — `accounts_file()` path helper: `<data>/accounts.json`; ensures parent dir exists
- `src-tauri/Cargo.toml` — `keyring = { version = "3", features = ["apple-native", "windows-native", "sync-secret-service"] }`, `async-trait = "0.1"`, `thiserror = "1"`

## Docs

- `docs/spec/authentication.md` — implementation contract: 5 checkpoints, success criteria, approach table (A1/B1/C1 chosen), risks, open questions
- `docs/design/authentication.md` — problem statement, decisions table, auth chain sequence diagram (Mermaid), data model (ER diagram), error taxonomy table, approach analysis

## Coupling

- **launch domain:** `launch_instance` in `lib.rs` calls `launch::resolve_launch_identity` which takes `&mut AccountStore` + `&dyn AuthHttpClient`; reads the stored MS refresh token, runs `refresh_ms_token` → `xbox_chain` to obtain a fresh MC access token; updates the store. `settings.rs` `offline_mode: bool` field controls whether online or offline identity is used. Any change to `AccountStore` API or `AuthHttpClient` trait affects `launch.rs`.
- **instances domain:** `settings.rs` gained `offline_mode: bool` (default `false`); `Settings` IPC type in `ipc.ts` does not yet expose this field — it is read only on the Rust side at launch time.
- **frontend-shell / ipc.ts:** `AccountMeta`, `DeviceCodePayload`, `AuthCommandError` types and 6 IPC functions hand-mirrored in `ipc.ts`; no type generation. Any Rust struct rename requires manual `ipc.ts` update.

## Conventions worth knowing

- Azure `client_id` is `"00000000402b5328"` (official public Minecraft launcher id, constant in `auth.rs:18`; same used by PrismLauncher/MultiMC).
- XSTS `xuid` is from `DisplayClaims.xui[0].xid` (field name is `xid`, not `xuid` — verified by fixture tests).
- `remove_account` ordering: keyring delete first; if that fails, in-memory state and disk are left unchanged (F-10 fix, prevents desync).
- `new_empty` constructor starts with an empty store when `accounts.json` is corrupt — app boots without panicking; future logins write to the correct path.
- `poll_token_once` treats both HTTP 200 and 400 as parseable poll responses (MS uses 400 for `authorization_pending`, `expired_token`, etc.); any other status is `HttpStatus` error.
- WSL / Linux without secret-service daemon: keyring operations fail loud with `AuthError::Keyring` — no silent plaintext fallback.
- Test convention: `MockAuthClient` is a `VecDeque<MockResp>` that pops FIFO regardless of which method (`post_form`/`post_json`/`get_bearer`) is called; tests are ordered by expected call sequence.
