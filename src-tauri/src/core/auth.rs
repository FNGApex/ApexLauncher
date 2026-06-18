//! Microsoft OAuth 2.0 device-code authentication for Minecraft.
//!
//! ## Scope (CP1 — device-code + poll + refresh)
//! - `request_device_code`: POST to MS device-code endpoint; returns device-code data.
//! - `poll_token`: polls MS token endpoint until resolved, expired, or declined.
//! - `refresh_ms_token`: exchanges a refresh token for a new MS access token.
//!
//! ## Scope (CP2 — Xbox chain + error mapping)
//! - `xbox_chain`: XBL → XSTS → MC token → profile; returns an `Account`.
//!
//! ## Testing convention
//! No live HTTP in any test. All HTTP calls go through the injectable `AuthHttpClient`
//! trait. Tests supply a mock client backed by a pre-loaded response queue.

// Azure AD application (client) ID for the device-code flow: the "modloader" app,
// registered 2026-06-09 (consumers tenant, personal accounts only, public client
// flows enabled). Override per-deploy with the MODLOADER_MS_CLIENT_ID env var.
//
// Do NOT swap in the legacy official-launcher id (00000000402b5328): it exists only
// in login.live.com for the redirect flow, and the AAD v2.0 device-code endpoint
// rejects it with AADSTS700016. See docs/design/auth-client-id-blocker.md.
pub const DEFAULT_MS_CLIENT_ID: &str = "82a79499-8c2e-49b8-9e42-1dd9d56252f2";

/// Env var that overrides the built-in client ID (for forks / CI / other deploys).
pub const MS_CLIENT_ID_ENV: &str = "MODLOADER_MS_CLIENT_ID";

/// Effective client ID: `MODLOADER_MS_CLIENT_ID` if set and non-blank, else the default.
pub fn ms_client_id() -> String {
    ms_client_id_from(std::env::var(MS_CLIENT_ID_ENV).ok())
}

/// Pure resolution logic, separated from env access so tests need no env mutation.
fn ms_client_id_from(override_val: Option<String>) -> String {
    match override_val {
        Some(v) if !v.trim().is_empty() => v,
        _ => DEFAULT_MS_CLIENT_ID.to_string(),
    }
}

pub const MS_DEVICE_CODE_URL: &str =
    "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
pub const MS_TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";

const XBL_AUTH_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_AUTH_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MC_LOGIN_URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MC_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";

// XSTS error codes (returned in the body of HTTP 401 responses).
const XERR_NO_XBOX_ACCOUNT: u64 = 2148916233;
const XERR_REGION_BLOCKED: u64 = 2148916235;
const XERR_CHILD_ACCOUNT: u64 = 2148916238;

// ── Data types ────────────────────────────────────────────────────────────────

/// Parsed response from the device-code endpoint.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    /// Seconds between poll attempts (minimum).
    pub interval: u64,
    /// Seconds until the code expires.
    pub expires_in: u64,
}

/// Successful token pair returned on a completed poll or refresh.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct MsTokens {
    pub access_token: String,
    pub refresh_token: String,
    /// Lifetime in seconds (from MS).
    pub expires_in: u64,
}

/// A resolved Minecraft account, produced by the full MS→Xbox→MC chain.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Account {
    /// Minecraft profile UUID (from `mc/profile`).
    pub id: String,
    /// Minecraft username / player name.
    pub username: String,
    /// Xbox user ID (from XSTS `DisplayClaims.xui[0].xid`).
    pub xuid: String,
    /// MC access token — short-lived, used at launch time.
    pub mc_access_token: String,
    /// Lifetime in seconds of the MC access token.
    pub mc_token_expires_in: u64,
}

/// Raw token-endpoint response — used internally to distinguish outcomes.
#[derive(Debug, serde::Deserialize)]
struct RawTokenResponse {
    // Success fields
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    // Error fields
    error: Option<String>,
}

// ── Xbox / MC internal types ──────────────────────────────────────────────────

/// XBL authenticate response (internal).
#[derive(Debug, serde::Deserialize)]
struct XblResponse {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    display_claims: XblDisplayClaims,
}

#[derive(Debug, serde::Deserialize)]
struct XblDisplayClaims {
    xui: Vec<XblXui>,
}

#[derive(Debug, serde::Deserialize)]
struct XblXui {
    uhs: String,
}

/// XSTS authorize response (internal).
#[derive(Debug, serde::Deserialize)]
struct XstsResponse {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    display_claims: XstsDisplayClaims,
}

#[derive(Debug, serde::Deserialize)]
struct XstsDisplayClaims {
    xui: Vec<XstsXui>,
}

#[derive(Debug, serde::Deserialize)]
struct XstsXui {
    /// Xbox user ID. Field name is `xid` in XSTS claims (not `xuid`).
    /// Optional: some accounts return `xui[0]` with only `uhs` and no `xid`.
    /// The Minecraft identity token does not use xuid, so absence is tolerated.
    #[serde(default)]
    xid: Option<String>,
}

/// XSTS error body (returned inside a 401 response).
#[derive(Debug, serde::Deserialize)]
struct XstsErrorBody {
    #[serde(rename = "XErr")]
    x_err: u64,
}

/// MC login_with_xbox response (internal).
#[derive(Debug, serde::Deserialize)]
struct McTokenResponse {
    access_token: String,
    expires_in: u64,
}

/// MC profile response (internal).
#[derive(Debug, serde::Deserialize)]
struct McProfileResponse {
    id: String,
    name: String,
}

// ── Error taxonomy ────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// The device code has expired. User must restart the login flow.
    #[error("device code expired — please try again")]
    DeviceCodeExpired,

    /// The user explicitly cancelled / denied the sign-in prompt.
    #[error("sign-in cancelled by user")]
    AuthorizationDeclined,

    /// The Microsoft account has no linked Xbox Live account.
    #[error("Microsoft account has no Xbox Live account (XErr 2148916233)")]
    NoXboxAccount,

    /// Xbox Live is not available in the user's region.
    #[error("Xbox Live is not available in your region (XErr 2148916235)")]
    XboxRegionBlocked,

    /// Child / family-managed account without parental consent.
    #[error("child/family-managed account requires parental consent (XErr 2148916238)")]
    ChildAccount,

    /// The Microsoft account does not own Minecraft (profile 404).
    #[error("this account does not own Minecraft")]
    NoMinecraftLicense,

    /// Generic HTTP or network failure.
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// The server returned a JSON body we cannot parse.
    #[error("unexpected response body: {0}")]
    BadResponse(String),

    /// The server returned an unexpected HTTP status.
    #[error("unexpected HTTP status {status}: {body}")]
    HttpStatus { status: u16, body: String },

    /// The OS keyring backend failed (unavailable, locked, or permission denied).
    #[error("keyring error: {0}")]
    Keyring(String),

    /// Account store I/O or parse failure (reading/writing `account.json`).
    #[error("account store error: {0}")]
    Store(String),

    /// Unexpected internal condition (not a user-facing error).
    #[error("internal error: {0}")]
    Internal(String),
}

// ── Injectable HTTP client seam ───────────────────────────────────────────────

/// Minimal async HTTP abstraction so tests can inject a mock without live TCP.
///
/// `post_form` and `post_json` return `(status_code, body)` so callers can
/// inspect error bodies (e.g. XSTS 401 with `XErr`). `get_bearer` is used
/// for the MC profile GET. The real implementation uses `reqwest::Client`.
#[async_trait::async_trait]
pub trait AuthHttpClient: Send + Sync {
    /// POST `application/x-www-form-urlencoded`. Returns `(status, body)`.
    async fn post_form(
        &self,
        url: &str,
        params: &[(&str, &str)],
    ) -> Result<(u16, String), reqwest::Error>;

    /// POST a JSON body. Returns `(status, body)`.
    async fn post_json(
        &self,
        url: &str,
        body: serde_json::Value,
    ) -> Result<(u16, String), reqwest::Error>;

    /// GET with a Bearer token. Returns `(status, body)`.
    async fn get_bearer(
        &self,
        url: &str,
        token: &str,
    ) -> Result<(u16, String), reqwest::Error>;
}

/// Production implementation backed by a real `reqwest::Client`.
pub struct ReqwestAuthClient(pub reqwest::Client);

#[async_trait::async_trait]
impl AuthHttpClient for ReqwestAuthClient {
    async fn post_form(
        &self,
        url: &str,
        params: &[(&str, &str)],
    ) -> Result<(u16, String), reqwest::Error> {
        let resp = self.0.post(url).form(params).send().await?;
        // Capture status before consuming the body.
        let status = resp.status().as_u16();
        let body = resp.text().await?;
        Ok((status, body))
    }

    async fn post_json(
        &self,
        url: &str,
        body: serde_json::Value,
    ) -> Result<(u16, String), reqwest::Error> {
        let resp = self.0.post(url).json(&body).send().await?;
        let status = resp.status().as_u16();
        let text = resp.text().await?;
        Ok((status, text))
    }

    async fn get_bearer(
        &self,
        url: &str,
        token: &str,
    ) -> Result<(u16, String), reqwest::Error> {
        let resp = self
            .0
            .get(url)
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await?;
        let status = resp.status().as_u16();
        let body = resp.text().await?;
        Ok((status, body))
    }
}

// ── Core functions ────────────────────────────────────────────────────────────

/// POST to the MS device-code endpoint; returns device-code data.
///
/// The caller should emit an `auth://device-code` event carrying
/// `user_code` + `verification_uri` before starting the poll loop (CP4).
pub async fn request_device_code(
    client: &dyn AuthHttpClient,
    device_code_url: &str,
) -> Result<DeviceCodeResponse, AuthError> {
    let client_id = ms_client_id();
    let (status, body) = client
        .post_form(
            device_code_url,
            &[
                ("client_id", &client_id),
                ("scope", "XboxLive.signin offline_access"),
            ],
        )
        .await?;

    if status != 200 {
        return Err(AuthError::HttpStatus { status, body });
    }

    serde_json::from_str::<DeviceCodeResponse>(&body)
        .map_err(|e| AuthError::BadResponse(format!("{e}: {body}")))
}

/// Polls the MS token endpoint once.
///
/// Returns:
/// - `Ok(Some(tokens))` — success, poll loop should stop.
/// - `Ok(None)`         — still pending, poll again after `interval`.
/// - `Err(AuthError::DeviceCodeExpired)` — code expired; restart flow.
/// - `Err(AuthError::AuthorizationDeclined)` — user cancelled.
/// - Other `Err` variants for network / parse failures.
pub async fn poll_token_once(
    client: &dyn AuthHttpClient,
    token_url: &str,
    device_code: &str,
) -> Result<Option<MsTokens>, AuthError> {
    let client_id = ms_client_id();
    let (status, body) = client
        .post_form(
            token_url,
            &[
                ("client_id", &client_id),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("device_code", device_code),
            ],
        )
        .await?;

    // MS token endpoint uses 400 for pending/error states (authorization_pending,
    // expired_token, access_denied, etc.) — parse the body for the error field.
    // Any other non-200 status is unexpected and returned as HttpStatus.
    if status != 200 && status != 400 {
        return Err(AuthError::HttpStatus { status, body });
    }
    parse_poll_response(&body)
}

/// Exchanges a stored refresh token for a fresh MS access+refresh token pair.
pub async fn refresh_ms_token(
    client: &dyn AuthHttpClient,
    token_url: &str,
    refresh_token: &str,
) -> Result<MsTokens, AuthError> {
    let client_id = ms_client_id();
    let (status, body) = client
        .post_form(
            token_url,
            &[
                ("client_id", &client_id),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("scope", "XboxLive.signin offline_access"),
            ],
        )
        .await?;

    // 400 errors carry a parseable JSON error body; non-400 non-200 are unexpected.
    if status != 200 && status != 400 {
        return Err(AuthError::HttpStatus { status, body });
    }

    let raw: RawTokenResponse = serde_json::from_str(&body)
        .map_err(|e| AuthError::BadResponse(format!("{e}: {body}")))?;

    if let (Some(access_token), Some(refresh_token), Some(expires_in)) =
        (raw.access_token, raw.refresh_token, raw.expires_in)
    {
        Ok(MsTokens {
            access_token,
            refresh_token,
            expires_in,
        })
    } else if let Some(err) = raw.error {
        map_oauth_error(&err)
    } else {
        Err(AuthError::BadResponse(body))
    }
}

/// Full Xbox → MC authentication chain.
///
/// Takes an `MsTokens` (from `poll_token_once` / `refresh_ms_token`) and runs:
/// XBL authenticate → XSTS authorize → MC login_with_xbox → MC profile GET.
///
/// Returns a fully populated `Account` on success.
pub async fn xbox_chain(
    client: &dyn AuthHttpClient,
    ms_tokens: MsTokens,
) -> Result<Account, AuthError> {
    // 1. XBL — exchange MS access token for an Xbox Live token + uhs.
    let (xbl_token, uhs) = xbl_authenticate(client, &ms_tokens.access_token).await?;

    // 2. XSTS — exchange XBL token for XSTS token + xuid.
    let (xsts_token, xuid) = xsts_authorize(client, &xbl_token).await?;

    // 3. MC token — exchange XSTS for a Minecraft access token.
    let identity_token = format!("XBL3.0 x={uhs};{xsts_token}");
    let mc_token = mc_login(client, &identity_token).await?;

    // 4. MC profile — fetch UUID + username.
    let profile = mc_get_profile(client, &mc_token.access_token).await?;

    Ok(Account {
        id: profile.id,
        username: profile.name,
        xuid,
        mc_access_token: mc_token.access_token,
        mc_token_expires_in: mc_token.expires_in,
    })
}

// ── Xbox chain helpers ────────────────────────────────────────────────────────

async fn xbl_authenticate(
    client: &dyn AuthHttpClient,
    ms_access_token: &str,
) -> Result<(String, String), AuthError> {
    let body = serde_json::json!({
        "Properties": {
            "AuthMethod": "RPS",
            "SiteName":   "user.auth.xboxlive.com",
            "RpsTicket":  format!("d={ms_access_token}")
        },
        "RelyingParty": "http://auth.xboxlive.com",
        "TokenType":    "JWT"
    });

    let (status, resp_body) = client.post_json(XBL_AUTH_URL, body).await?;

    if status != 200 {
        return Err(AuthError::HttpStatus {
            status,
            body: resp_body,
        });
    }

    let xbl: XblResponse = serde_json::from_str(&resp_body)
        .map_err(|e| AuthError::BadResponse(format!("{e}: {resp_body}")))?;

    let uhs = xbl
        .display_claims
        .xui
        .into_iter()
        .next()
        .map(|x| x.uhs)
        .ok_or_else(|| AuthError::BadResponse("XBL response missing xui[0].uhs".to_owned()))?;

    Ok((xbl.token, uhs))
}

async fn xsts_authorize(
    client: &dyn AuthHttpClient,
    xbl_token: &str,
) -> Result<(String, String), AuthError> {
    let body = serde_json::json!({
        "Properties": {
            "SandboxId":  "RETAIL",
            "UserTokens": [xbl_token]
        },
        "RelyingParty": "rp://api.minecraftservices.com/",
        "TokenType":    "JWT"
    });

    let (status, resp_body) = client.post_json(XSTS_AUTH_URL, body).await?;

    if status == 401 {
        // XSTS error — body contains XErr code.
        return Err(map_xsts_error(&resp_body));
    }

    if status != 200 {
        return Err(AuthError::HttpStatus {
            status,
            body: resp_body,
        });
    }

    let xsts: XstsResponse = serde_json::from_str(&resp_body)
        .map_err(|e| AuthError::BadResponse(format!("{e}: {resp_body}")))?;

    let xuid = xsts
        .display_claims
        .xui
        .into_iter()
        .next()
        .map(|x| x.xid.unwrap_or_default())
        .ok_or_else(|| {
            AuthError::BadResponse("XSTS response missing xui[0]".to_owned())
        })?;

    Ok((xsts.token, xuid))
}

async fn mc_login(
    client: &dyn AuthHttpClient,
    identity_token: &str,
) -> Result<McTokenResponse, AuthError> {
    let body = serde_json::json!({
        "identityToken": identity_token
    });

    let (status, resp_body) = client.post_json(MC_LOGIN_URL, body).await?;

    if status != 200 {
        return Err(AuthError::HttpStatus {
            status,
            body: resp_body,
        });
    }

    serde_json::from_str::<McTokenResponse>(&resp_body)
        .map_err(|e| AuthError::BadResponse(format!("{e}: {resp_body}")))
}

async fn mc_get_profile(
    client: &dyn AuthHttpClient,
    mc_access_token: &str,
) -> Result<McProfileResponse, AuthError> {
    let (status, resp_body) = client.get_bearer(MC_PROFILE_URL, mc_access_token).await?;

    if status == 404 {
        return Err(AuthError::NoMinecraftLicense);
    }

    if status != 200 {
        return Err(AuthError::HttpStatus {
            status,
            body: resp_body,
        });
    }

    serde_json::from_str::<McProfileResponse>(&resp_body)
        .map_err(|e| AuthError::BadResponse(format!("{e}: {resp_body}")))
}

/// Maps a XSTS 401 body to the appropriate `AuthError` variant.
fn map_xsts_error(body: &str) -> AuthError {
    if let Ok(err_body) = serde_json::from_str::<XstsErrorBody>(body) {
        match err_body.x_err {
            XERR_NO_XBOX_ACCOUNT => AuthError::NoXboxAccount,
            XERR_REGION_BLOCKED => AuthError::XboxRegionBlocked,
            XERR_CHILD_ACCOUNT => AuthError::ChildAccount,
            other => AuthError::HttpStatus {
                status: 401,
                body: format!("XSTS XErr={other}"),
            },
        }
    } else {
        AuthError::HttpStatus {
            status: 401,
            body: body.to_owned(),
        }
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn parse_poll_response(body: &str) -> Result<Option<MsTokens>, AuthError> {
    let raw: RawTokenResponse = serde_json::from_str(body)
        .map_err(|e| AuthError::BadResponse(format!("{e}: {body}")))?;

    if let (Some(access_token), Some(refresh_token), Some(expires_in)) =
        (raw.access_token, raw.refresh_token, raw.expires_in)
    {
        return Ok(Some(MsTokens {
            access_token,
            refresh_token,
            expires_in,
        }));
    }

    if let Some(err) = raw.error {
        match err.as_str() {
            "authorization_pending" => return Ok(None),
            other => return map_oauth_error(other),
        }
    }

    Err(AuthError::BadResponse(body.to_owned()))
}

/// Maps an OAuth error string to an `AuthError`. Returns the error inside `Err`.
fn map_oauth_error<T>(err: &str) -> Result<T, AuthError> {
    match err {
        "expired_token" => Err(AuthError::DeviceCodeExpired),
        "authorization_declined" | "access_denied" => Err(AuthError::AuthorizationDeclined),
        other => Err(AuthError::BadResponse(format!("oauth error: {other}"))),
    }
}

// ── CP3: Account store (persistence + keyring seam) ──────────────────────────

/// Non-secret account metadata written to `account.json`.
///
/// The refresh token and MC access token are NOT stored here: the refresh token
/// lives in the OS keyring (via [`KeyringBackend`]); the MC access token is
/// re-derived at launch time.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountMeta {
    /// Minecraft profile UUID (from `mc/profile`).
    pub id: String,
    /// Minecraft username.
    pub username: String,
    /// Xbox user ID.
    pub xuid: String,
    /// Unix timestamp (seconds) at which the MC token expires; `None` if unknown.
    /// Alias keeps account.json files written before the camelCase rename loadable.
    #[serde(alias = "mc_token_expires")]
    pub mc_token_expires: Option<u64>,
}

impl AccountMeta {
    /// Build an `AccountMeta` from a fully resolved `Account`.
    /// `login_time_secs` is the wall-clock time at which the account was obtained
    /// (used to compute `mc_token_expires`).
    pub fn from_account(account: &Account, login_time_secs: u64) -> Self {
        AccountMeta {
            id: account.id.clone(),
            username: account.username.clone(),
            xuid: account.xuid.clone(),
            mc_token_expires: Some(login_time_secs + account.mc_token_expires_in),
        }
    }
}

/// Injectable seam for OS keyring access so tests never call the real keyring.
pub trait KeyringBackend: Send + Sync {
    /// Store `secret` for `account_id`. Overwrites any prior value.
    fn store_secret(&self, account_id: &str, secret: &str) -> Result<(), AuthError>;
    /// Retrieve the secret for `account_id`.
    fn load_secret(&self, account_id: &str) -> Result<String, AuthError>;
    /// Delete the secret for `account_id`. Not-found is not an error.
    fn delete_secret(&self, account_id: &str) -> Result<(), AuthError>;
}

/// Production keyring backend backed by the `keyring` crate.
pub struct SystemKeyringBackend;

const KEYRING_SERVICE: &str = "modloader";

/// Fixed keyring key used for the single account's refresh token.
/// Using a constant means the key does not depend on the account id, which
/// simplifies logout (no need to track the id just to delete the key).
const KEYRING_ACCOUNT_KEY: &str = "current_account";

impl KeyringBackend for SystemKeyringBackend {
    fn store_secret(&self, account_id: &str, secret: &str) -> Result<(), AuthError> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account_id)
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        entry
            .set_password(secret)
            .map_err(|e| AuthError::Keyring(e.to_string()))
    }

    fn load_secret(&self, account_id: &str) -> Result<String, AuthError> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account_id)
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        entry
            .get_password()
            .map_err(|e| AuthError::Keyring(e.to_string()))
    }

    fn delete_secret(&self, account_id: &str) -> Result<(), AuthError> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account_id)
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        match entry.delete_credential() {
            Ok(_) => Ok(()),
            // Not-found is not an error.
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AuthError::Keyring(e.to_string())),
        }
    }
}

/// Single-account store: persists `AccountMeta` to `account.json`, refresh token to keyring.
///
/// One account at a time. Login replaces any prior account. Logout clears both the
/// JSON file and the keyring entry. The store is not thread-safe by itself; callers
/// must serialize access (e.g. a `Mutex<AccountStore>` in the Tauri state).
pub struct AccountStore {
    path: std::path::PathBuf,
    keyring: Box<dyn KeyringBackend>,
    account: Option<AccountMeta>,
}

impl AccountStore {
    /// Load from `path` if it exists; start empty otherwise.
    pub fn load(
        path: std::path::PathBuf,
        keyring: Box<dyn KeyringBackend>,
    ) -> Result<Self, AuthError> {
        let account = if path.exists() {
            let raw = std::fs::read_to_string(&path)
                .map_err(|e| AuthError::Store(e.to_string()))?;
            let meta = serde_json::from_str::<AccountMeta>(&raw)
                .map_err(|e| AuthError::Store(format!("account.json parse error: {e}")))?;
            Some(meta)
        } else {
            None
        };
        Ok(AccountStore { path, keyring, account })
    }

    /// Construct an in-memory-only empty store (no file I/O).
    ///
    /// Used when `load` fails (e.g. corrupt `account.json`) to start the app
    /// without panicking. The path is kept so future writes go to the correct location.
    pub fn new_empty(path: std::path::PathBuf, keyring: Box<dyn KeyringBackend>) -> Self {
        AccountStore { path, keyring, account: None }
    }

    /// Persist `account` to `account.json`.
    fn save(&self) -> Result<(), AuthError> {
        let meta = match &self.account {
            Some(m) => m,
            None => return Ok(()),
        };
        let json = serde_json::to_string_pretty(meta)
            .map_err(|e| AuthError::Store(e.to_string()))?;
        std::fs::write(&self.path, json).map_err(|e| AuthError::Store(e.to_string()))
    }

    /// Store the single account: write `AccountMeta` to `account.json` and the
    /// refresh token to the keyring under the fixed key. Overwrites any prior account.
    pub fn set_account(&mut self, meta: AccountMeta, refresh_token: &str) -> Result<(), AuthError> {
        // Store secret first — if keyring fails, don't persist stale metadata.
        self.keyring.store_secret(KEYRING_ACCOUNT_KEY, refresh_token)?;
        self.account = Some(meta);
        self.save()
    }

    /// The current account metadata, or `None` if not logged in.
    pub fn get_account(&self) -> Option<&AccountMeta> {
        self.account.as_ref()
    }

    /// Clear the account: delete `account.json` and the keyring entry. Idempotent.
    pub fn logout(&mut self) -> Result<(), AuthError> {
        // Delete keyring first. If it fails, leave the file intact (no desync).
        // Not-found is tolerated by the KeyringBackend contract.
        self.keyring.delete_secret(KEYRING_ACCOUNT_KEY)?;

        // Remove the file if it exists.
        if self.path.exists() {
            std::fs::remove_file(&self.path)
                .map_err(|e| AuthError::Store(format!("could not delete account.json: {e}")))?;
        }

        self.account = None;
        Ok(())
    }

    /// Retrieve the current account's refresh token from the keyring.
    pub fn get_refresh_token(&self) -> Result<String, AuthError> {
        if self.account.is_none() {
            return Err(AuthError::Store("no account logged in".to_owned()));
        }
        self.keyring.load_secret(KEYRING_ACCOUNT_KEY)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;
