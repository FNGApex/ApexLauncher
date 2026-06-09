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

// Official public Minecraft launcher Azure client_id.
// Source: https://wiki.vg/Microsoft_Authentication_Scheme (also used by PrismLauncher,
// MultiMC, and other open-source MC launchers).
pub const MS_CLIENT_ID: &str = "00000000402b5328";

const MS_DEVICE_CODE_URL: &str =
    "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const MS_TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";

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
    xid: String,
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
    let (status, body) = client
        .post_form(
            device_code_url,
            &[
                ("client_id", MS_CLIENT_ID),
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
    let (status, body) = client
        .post_form(
            token_url,
            &[
                ("client_id", MS_CLIENT_ID),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("device_code", device_code),
            ],
        )
        .await?;

    // MS token endpoint uses 400 for pending/error states — parse the body for
    // the error field regardless of status.
    let _ = status; // body contains discriminating info; status alone doesn't distinguish error subtypes
    parse_poll_response(&body)
}

/// Exchanges a stored refresh token for a fresh MS access+refresh token pair.
pub async fn refresh_ms_token(
    client: &dyn AuthHttpClient,
    token_url: &str,
    refresh_token: &str,
) -> Result<MsTokens, AuthError> {
    let (status, body) = client
        .post_form(
            token_url,
            &[
                ("client_id", MS_CLIENT_ID),
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
        .map(|x| x.xid)
        .ok_or_else(|| {
            AuthError::BadResponse("XSTS response missing xui[0].xid".to_owned())
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    //! All HTTP calls go through an injectable mock client. No live network
    //! connections are opened in any test. Each test pre-loads response bodies
    //! (and status codes) into the mock; calls consume them in FIFO order.

    use super::*;
    use std::collections::VecDeque;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    // ── Mock HTTP client ──────────────────────────────────────────────────────

    /// Canned response: HTTP status code + body string.
    struct MockResp(u16, String);

    impl MockResp {
        fn ok(body: impl Into<String>) -> Self {
            MockResp(200, body.into())
        }
        fn status(code: u16, body: impl Into<String>) -> Self {
            MockResp(code, body.into())
        }
    }

    /// Mock client backed by a VecDeque of pre-loaded `(status, body)` pairs.
    /// Each HTTP call — `post_form`, `post_json`, or `get_bearer` — pops the
    /// next entry regardless of which method is called (ordering is per-test).
    struct MockAuthClient {
        responses: Arc<Mutex<VecDeque<MockResp>>>,
    }

    impl MockAuthClient {
        fn new(responses: Vec<MockResp>) -> Self {
            Self {
                responses: Arc::new(Mutex::new(responses.into_iter().collect())),
            }
        }

        async fn pop(&self) -> (u16, String) {
            let mut q = self.responses.lock().await;
            let MockResp(s, b) = q
                .pop_front()
                .expect("MockAuthClient: no more canned responses");
            (s, b)
        }
    }

    #[async_trait::async_trait]
    impl AuthHttpClient for MockAuthClient {
        async fn post_form(
            &self,
            _url: &str,
            _params: &[(&str, &str)],
        ) -> Result<(u16, String), reqwest::Error> {
            Ok(self.pop().await)
        }

        async fn post_json(
            &self,
            _url: &str,
            _body: serde_json::Value,
        ) -> Result<(u16, String), reqwest::Error> {
            Ok(self.pop().await)
        }

        async fn get_bearer(
            &self,
            _url: &str,
            _token: &str,
        ) -> Result<(u16, String), reqwest::Error> {
            Ok(self.pop().await)
        }
    }

    // ── CP1 fixture helpers ───────────────────────────────────────────────────

    fn device_code_json() -> String {
        r#"{
            "device_code": "dc_abc123",
            "user_code": "ABCD-1234",
            "verification_uri": "https://microsoft.com/devicelogin",
            "interval": 5,
            "expires_in": 900
        }"#
        .to_owned()
    }

    fn success_token_json() -> String {
        r#"{
            "access_token": "ms_access_xyz",
            "refresh_token": "ms_refresh_xyz",
            "expires_in": 3600
        }"#
        .to_owned()
    }

    fn pending_json() -> String {
        r#"{"error":"authorization_pending","error_description":"still waiting"}"#.to_owned()
    }

    fn expired_json() -> String {
        r#"{"error":"expired_token","error_description":"code expired"}"#.to_owned()
    }

    fn declined_json() -> String {
        r#"{"error":"authorization_declined","error_description":"user declined"}"#.to_owned()
    }

    fn access_denied_json() -> String {
        r#"{"error":"access_denied","error_description":"access denied"}"#.to_owned()
    }

    // ── CP2 fixture helpers ───────────────────────────────────────────────────

    /// Real-shape XBL authenticate response.
    fn xbl_response_json() -> String {
        r#"{
            "IssueInstant": "2023-01-01T00:00:00.0000000Z",
            "NotAfter":     "2023-01-02T00:00:00.0000000Z",
            "Token":        "xbl_token_abc",
            "DisplayClaims": {
                "xui": [{ "uhs": "userhash_abc" }]
            }
        }"#
        .to_owned()
    }

    /// Real-shape XSTS authorize response. Field is `xid` (not `xuid`).
    fn xsts_response_json() -> String {
        r#"{
            "IssueInstant": "2023-01-01T00:00:00.0000000Z",
            "NotAfter":     "2023-01-02T00:00:00.0000000Z",
            "Token":        "xsts_token_abc",
            "DisplayClaims": {
                "xui": [{ "xid": "xbox_user_id_1234" }]
            }
        }"#
        .to_owned()
    }

    /// XSTS 401 body for "no Xbox account" (XErr 2148916233).
    fn xsts_err_no_xbox_json() -> String {
        r#"{"Identity":"0","XErr":2148916233,"Message":"","Redirect":"https://start.ui.com/upsell"}"#.to_owned()
    }

    /// XSTS 401 body for "region blocked" (XErr 2148916235).
    fn xsts_err_region_json() -> String {
        r#"{"Identity":"0","XErr":2148916235,"Message":"","Redirect":""}"#.to_owned()
    }

    /// XSTS 401 body for "child account" (XErr 2148916238).
    fn xsts_err_child_json() -> String {
        r#"{"Identity":"0","XErr":2148916238,"Message":"","Redirect":"https://start.ui.com/family"}"#.to_owned()
    }

    /// MC login_with_xbox success response.
    fn mc_token_json() -> String {
        r#"{
            "username":    "some_uuid_ignored",
            "access_token":"mc_token_xyz",
            "token_type":  "Bearer",
            "expires_in":  86400
        }"#
        .to_owned()
    }

    /// MC profile success response. `id` is the Minecraft UUID.
    fn mc_profile_json() -> String {
        r#"{
            "id":   "aaaabbbbccccdddd",
            "name": "Steve",
            "skins": [],
            "capes": []
        }"#
        .to_owned()
    }

    fn ms_tokens() -> MsTokens {
        MsTokens {
            access_token: "ms_access_xyz".to_owned(),
            refresh_token: "ms_refresh_xyz".to_owned(),
            expires_in: 3600,
        }
    }

    // ── CP1 tests: request_device_code ───────────────────────────────────────

    #[tokio::test]
    async fn cp1_device_code_parses_all_fields() {
        let client = MockAuthClient::new(vec![MockResp::ok(device_code_json())]);
        let resp = request_device_code(&client, "http://unused")
            .await
            .expect("device code request should succeed");

        assert_eq!(resp.device_code, "dc_abc123");
        assert_eq!(resp.user_code, "ABCD-1234");
        assert_eq!(resp.verification_uri, "https://microsoft.com/devicelogin");
        assert_eq!(resp.interval, 5);
        assert_eq!(resp.expires_in, 900);
    }

    // ── CP1 tests: poll_token_once ────────────────────────────────────────────

    #[tokio::test]
    async fn cp1_poll_pending_returns_none() {
        let client = MockAuthClient::new(vec![MockResp::ok(pending_json())]);
        let result = poll_token_once(&client, "http://unused", "dc_abc123")
            .await
            .expect("pending should not be Err");
        assert!(result.is_none(), "authorization_pending must yield None");
    }

    #[tokio::test]
    async fn cp1_poll_success_returns_tokens() {
        let client = MockAuthClient::new(vec![MockResp::ok(success_token_json())]);
        let tokens = poll_token_once(&client, "http://unused", "dc_abc123")
            .await
            .expect("success poll should not be Err")
            .expect("success poll should yield Some(tokens)");

        assert_eq!(tokens.access_token, "ms_access_xyz");
        assert_eq!(tokens.refresh_token, "ms_refresh_xyz");
        assert_eq!(tokens.expires_in, 3600);
    }

    #[tokio::test]
    async fn cp1_poll_expired_token_is_distinct_error() {
        let client = MockAuthClient::new(vec![MockResp::ok(expired_json())]);
        let err = poll_token_once(&client, "http://unused", "dc_abc123")
            .await
            .expect_err("expired_token must be Err");
        assert!(
            matches!(err, AuthError::DeviceCodeExpired),
            "expected DeviceCodeExpired, got: {err}"
        );
    }

    #[tokio::test]
    async fn cp1_poll_authorization_declined_is_distinct_error() {
        let client = MockAuthClient::new(vec![MockResp::ok(declined_json())]);
        let err = poll_token_once(&client, "http://unused", "dc_abc123")
            .await
            .expect_err("authorization_declined must be Err");
        assert!(
            matches!(err, AuthError::AuthorizationDeclined),
            "expected AuthorizationDeclined, got: {err}"
        );
    }

    #[tokio::test]
    async fn cp1_poll_access_denied_is_authorization_declined() {
        let client = MockAuthClient::new(vec![MockResp::ok(access_denied_json())]);
        let err = poll_token_once(&client, "http://unused", "dc_abc123")
            .await
            .expect_err("access_denied must be Err");
        assert!(
            matches!(err, AuthError::AuthorizationDeclined),
            "expected AuthorizationDeclined for access_denied, got: {err}"
        );
    }

    // ── CP1 tests: refresh_ms_token ───────────────────────────────────────────

    #[tokio::test]
    async fn cp1_refresh_exchanges_token_successfully() {
        let client = MockAuthClient::new(vec![MockResp::ok(success_token_json())]);
        let tokens = refresh_ms_token(&client, "http://unused", "old_refresh_token")
            .await
            .expect("refresh should succeed");

        assert_eq!(tokens.access_token, "ms_access_xyz");
        assert_eq!(tokens.refresh_token, "ms_refresh_xyz");
        assert_eq!(tokens.expires_in, 3600);
    }

    #[tokio::test]
    async fn cp1_refresh_expired_token_is_expired_error() {
        // If the stored refresh token itself is expired, MS returns expired_token.
        let client = MockAuthClient::new(vec![MockResp::ok(expired_json())]);
        let err = refresh_ms_token(&client, "http://unused", "stale_refresh")
            .await
            .expect_err("expired refresh must be Err");
        assert!(
            matches!(err, AuthError::DeviceCodeExpired),
            "expected DeviceCodeExpired, got: {err}"
        );
    }

    // ── CP1 tests: sequential poll simulation ─────────────────────────────────

    /// Simulates a realistic poll sequence: two pending responses, then success.
    #[tokio::test]
    async fn cp1_poll_loop_pending_then_success() {
        let client = MockAuthClient::new(vec![
            MockResp::ok(pending_json()),
            MockResp::ok(pending_json()),
            MockResp::ok(success_token_json()),
        ]);

        // First two polls → None (still pending).
        let r1 = poll_token_once(&client, "http://unused", "dc")
            .await
            .unwrap();
        assert!(r1.is_none());

        let r2 = poll_token_once(&client, "http://unused", "dc")
            .await
            .unwrap();
        assert!(r2.is_none());

        // Third poll → success.
        let r3 = poll_token_once(&client, "http://unused", "dc")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(r3.access_token, "ms_access_xyz");
    }

    // ── CP2 tests: XBL authenticate ───────────────────────────────────────────

    #[tokio::test]
    async fn cp2_xbl_parses_token_and_uhs() {
        let client = MockAuthClient::new(vec![MockResp::ok(xbl_response_json())]);
        let (token, uhs) = xbl_authenticate(&client, "ms_access_xyz")
            .await
            .expect("XBL authenticate should succeed");

        assert_eq!(token, "xbl_token_abc");
        assert_eq!(uhs, "userhash_abc");
    }

    #[tokio::test]
    async fn cp2_xbl_non_200_maps_to_http_status_error() {
        let client = MockAuthClient::new(vec![MockResp::status(400, r#"{"error":"bad request"}"#)]);
        let err = xbl_authenticate(&client, "bad_token")
            .await
            .expect_err("XBL non-200 must be Err");
        assert!(
            matches!(err, AuthError::HttpStatus { status: 400, .. }),
            "expected HttpStatus(400), got: {err}"
        );
    }

    // ── CP2 tests: XSTS authorize ─────────────────────────────────────────────

    #[tokio::test]
    async fn cp2_xsts_parses_token_and_xuid_from_xid_field() {
        // Verifies the correct field name: `xid` not `xuid`.
        let client = MockAuthClient::new(vec![MockResp::ok(xsts_response_json())]);
        let (token, xuid) = xsts_authorize(&client, "xbl_token_abc")
            .await
            .expect("XSTS authorize should succeed");

        assert_eq!(token, "xsts_token_abc");
        // This assertion verifies the `xid` field is parsed correctly.
        assert_eq!(xuid, "xbox_user_id_1234");
    }

    #[tokio::test]
    async fn cp2_xsts_401_no_xbox_account_maps_to_named_variant() {
        let client =
            MockAuthClient::new(vec![MockResp::status(401, xsts_err_no_xbox_json())]);
        let err = xsts_authorize(&client, "xbl_token_abc")
            .await
            .expect_err("XSTS 401 XErr=2148916233 must be Err");
        assert!(
            matches!(err, AuthError::NoXboxAccount),
            "expected NoXboxAccount, got: {err}"
        );
    }

    #[tokio::test]
    async fn cp2_xsts_401_region_blocked_maps_to_named_variant() {
        let client =
            MockAuthClient::new(vec![MockResp::status(401, xsts_err_region_json())]);
        let err = xsts_authorize(&client, "xbl_token_abc")
            .await
            .expect_err("XSTS 401 XErr=2148916235 must be Err");
        assert!(
            matches!(err, AuthError::XboxRegionBlocked),
            "expected XboxRegionBlocked, got: {err}"
        );
    }

    #[tokio::test]
    async fn cp2_xsts_401_child_account_maps_to_named_variant() {
        let client =
            MockAuthClient::new(vec![MockResp::status(401, xsts_err_child_json())]);
        let err = xsts_authorize(&client, "xbl_token_abc")
            .await
            .expect_err("XSTS 401 XErr=2148916238 must be Err");
        assert!(
            matches!(err, AuthError::ChildAccount),
            "expected ChildAccount, got: {err}"
        );
    }

    // ── CP2 tests: MC token ───────────────────────────────────────────────────

    #[tokio::test]
    async fn cp2_mc_login_parses_token_and_expiry() {
        let identity_token = "XBL3.0 x=userhash_abc;xsts_token_abc";
        let client = MockAuthClient::new(vec![MockResp::ok(mc_token_json())]);
        let resp = mc_login(&client, identity_token)
            .await
            .expect("MC login should succeed");

        assert_eq!(resp.access_token, "mc_token_xyz");
        assert_eq!(resp.expires_in, 86400);
    }

    #[tokio::test]
    async fn cp2_mc_login_non_200_maps_to_http_status_error() {
        let client = MockAuthClient::new(vec![MockResp::status(401, r#"{"error":"Unauthorized"}"#)]);
        let err = mc_login(&client, "bad_identity")
            .await
            .expect_err("MC login non-200 must be Err");
        assert!(
            matches!(err, AuthError::HttpStatus { status: 401, .. }),
            "expected HttpStatus(401), got: {err}"
        );
    }

    // ── CP2 tests: MC profile ─────────────────────────────────────────────────

    #[tokio::test]
    async fn cp2_mc_profile_parses_id_and_name() {
        let client = MockAuthClient::new(vec![MockResp::ok(mc_profile_json())]);
        let profile = mc_get_profile(&client, "mc_token_xyz")
            .await
            .expect("MC profile should succeed");

        assert_eq!(profile.id, "aaaabbbbccccdddd");
        assert_eq!(profile.name, "Steve");
    }

    #[tokio::test]
    async fn cp2_mc_profile_404_maps_to_no_minecraft_license() {
        let client =
            MockAuthClient::new(vec![MockResp::status(404, r#"{"path":"/minecraft/profile"}"#)]);
        let err = mc_get_profile(&client, "mc_token_xyz")
            .await
            .expect_err("MC profile 404 must be Err");
        assert!(
            matches!(err, AuthError::NoMinecraftLicense),
            "expected NoMinecraftLicense, got: {err}"
        );
    }

    // ── CP2 tests: full xbox_chain happy path ─────────────────────────────────

    #[tokio::test]
    async fn cp2_xbox_chain_happy_path_produces_account() {
        // Four responses in order: XBL, XSTS, MC token, MC profile.
        let client = MockAuthClient::new(vec![
            MockResp::ok(xbl_response_json()),
            MockResp::ok(xsts_response_json()),
            MockResp::ok(mc_token_json()),
            MockResp::ok(mc_profile_json()),
        ]);

        let account = xbox_chain(&client, ms_tokens())
            .await
            .expect("xbox_chain happy path must succeed");

        assert_eq!(account.id, "aaaabbbbccccdddd");
        assert_eq!(account.username, "Steve");
        assert_eq!(account.xuid, "xbox_user_id_1234");
        assert_eq!(account.mc_access_token, "mc_token_xyz");
        assert_eq!(account.mc_token_expires_in, 86400);
    }

    #[tokio::test]
    async fn cp2_xbox_chain_xsts_error_propagates() {
        // XBL succeeds, XSTS returns 401 child-account.
        let client = MockAuthClient::new(vec![
            MockResp::ok(xbl_response_json()),
            MockResp::status(401, xsts_err_child_json()),
        ]);

        let err = xbox_chain(&client, ms_tokens())
            .await
            .expect_err("child account XSTS error must propagate");
        assert!(
            matches!(err, AuthError::ChildAccount),
            "expected ChildAccount, got: {err}"
        );
    }

    #[tokio::test]
    async fn cp2_xbox_chain_no_license_propagates() {
        // XBL + XSTS + MC token succeed; profile returns 404.
        let client = MockAuthClient::new(vec![
            MockResp::ok(xbl_response_json()),
            MockResp::ok(xsts_response_json()),
            MockResp::ok(mc_token_json()),
            MockResp::status(404, r#"{"path":"/minecraft/profile"}"#),
        ]);

        let err = xbox_chain(&client, ms_tokens())
            .await
            .expect_err("no license must propagate from profile 404");
        assert!(
            matches!(err, AuthError::NoMinecraftLicense),
            "expected NoMinecraftLicense, got: {err}"
        );
    }
}
