//! Microsoft OAuth 2.0 device-code authentication for Minecraft.
//!
//! ## Scope (CP1 — device-code + poll + refresh)
//! - `request_device_code`: POST to MS device-code endpoint; returns device-code data.
//! - `poll_token`: polls MS token endpoint until resolved, expired, or declined.
//! - `refresh_ms_token`: exchanges a refresh token for a new MS access token.
//!
//! ## CP2+ stubs (not implemented yet)
//! - `xbox_chain_stub`: placeholder seam for XBL → XSTS → MC → profile chain.
//!
//! ## Testing convention
//! No live HTTP in any test. All HTTP calls go through the injectable `AuthHttpClient`
//! trait. Tests supply a `TcpListener`-based mock server (same pattern as `download.rs`).

// Official public Minecraft launcher Azure client_id.
// Source: https://wiki.vg/Microsoft_Authentication_Scheme (also used by PrismLauncher,
// MultiMC, and other open-source MC launchers).
pub const MS_CLIENT_ID: &str = "00000000402b5328";

const MS_DEVICE_CODE_URL: &str =
    "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const MS_TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";

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

// ── Error taxonomy ────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// The device code has expired. User must restart the login flow.
    #[error("device code expired — please try again")]
    DeviceCodeExpired,

    /// The user explicitly cancelled / denied the sign-in prompt.
    #[error("sign-in cancelled by user")]
    AuthorizationDeclined,

    /// Generic HTTP or network failure.
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// The server returned a JSON body we cannot parse.
    #[error("unexpected response body: {0}")]
    BadResponse(String),

    /// CP2+ seam: Xbox chain not yet implemented (stub error).
    #[error("Xbox chain not yet implemented (CP2)")]
    XboxChainNotImplemented,
}

// ── Injectable HTTP client seam ───────────────────────────────────────────────

/// Minimal async HTTP abstraction so tests can inject a mock without live TCP.
///
/// The two methods mirror the two MS endpoints used in CP1. Implementations
/// POST `application/x-www-form-urlencoded` body and return the response body
/// bytes. The real implementation just uses `reqwest::Client`.
#[async_trait::async_trait]
pub trait AuthHttpClient: Send + Sync {
    async fn post_form(
        &self,
        url: &str,
        params: &[(&str, &str)],
    ) -> Result<String, reqwest::Error>;
}

/// Production implementation backed by a real `reqwest::Client`.
pub struct ReqwestAuthClient(pub reqwest::Client);

#[async_trait::async_trait]
impl AuthHttpClient for ReqwestAuthClient {
    async fn post_form(
        &self,
        url: &str,
        params: &[(&str, &str)],
    ) -> Result<String, reqwest::Error> {
        let resp = self.0.post(url).form(params).send().await?;
        resp.text().await
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
    let body = client
        .post_form(
            device_code_url,
            &[
                ("client_id", MS_CLIENT_ID),
                ("scope", "XboxLive.signin offline_access"),
            ],
        )
        .await?;

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
    let body = client
        .post_form(
            token_url,
            &[
                ("client_id", MS_CLIENT_ID),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("device_code", device_code),
            ],
        )
        .await?;

    parse_poll_response(&body)
}

/// Exchanges a stored refresh token for a fresh MS access+refresh token pair.
pub async fn refresh_ms_token(
    client: &dyn AuthHttpClient,
    token_url: &str,
    refresh_token: &str,
) -> Result<MsTokens, AuthError> {
    let body = client
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

/// CP2+ seam stub. Will be replaced when the Xbox chain is implemented.
/// Returns `Err(AuthError::XboxChainNotImplemented)` always.
pub async fn xbox_chain_stub(_ms_tokens: MsTokens) -> Result<(), AuthError> {
    Err(AuthError::XboxChainNotImplemented)
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
    //! All HTTP calls go through a `TcpListener`-based mock server (same pattern
    //! as `core/download.rs`). No live network connections are opened in any test.

    use super::*;
    use std::collections::VecDeque;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    // ── TcpListener mock server ───────────────────────────────────────────────

    /// Serves a fixed sequence of HTTP responses (one per accepted connection).
    /// Each call to `post_form` in `MockAuthClient` consumes the next response.
    struct MockAuthClient {
        /// Pre-loaded response bodies, in order.
        responses: Arc<Mutex<VecDeque<String>>>,
    }

    impl MockAuthClient {
        fn new(responses: Vec<String>) -> Self {
            Self {
                responses: Arc::new(Mutex::new(responses.into_iter().collect())),
            }
        }
    }

    #[async_trait::async_trait]
    impl AuthHttpClient for MockAuthClient {
        async fn post_form(
            &self,
            _url: &str,
            _params: &[(&str, &str)],
        ) -> Result<String, reqwest::Error> {
            let mut q = self.responses.lock().await;
            // Return the next canned response; panic loudly on underrun so tests fail clearly.
            Ok(q.pop_front().expect("MockAuthClient: no more canned responses"))
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

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

    // ── request_device_code ───────────────────────────────────────────────────

    #[tokio::test]
    async fn cp1_device_code_parses_all_fields() {
        let client = MockAuthClient::new(vec![device_code_json()]);
        let resp = request_device_code(&client, "http://unused")
            .await
            .expect("device code request should succeed");

        assert_eq!(resp.device_code, "dc_abc123");
        assert_eq!(resp.user_code, "ABCD-1234");
        assert_eq!(resp.verification_uri, "https://microsoft.com/devicelogin");
        assert_eq!(resp.interval, 5);
        assert_eq!(resp.expires_in, 900);
    }

    // ── poll_token_once ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn cp1_poll_pending_returns_none() {
        let client = MockAuthClient::new(vec![pending_json()]);
        let result = poll_token_once(&client, "http://unused", "dc_abc123")
            .await
            .expect("pending should not be Err");
        assert!(result.is_none(), "authorization_pending must yield None");
    }

    #[tokio::test]
    async fn cp1_poll_success_returns_tokens() {
        let client = MockAuthClient::new(vec![success_token_json()]);
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
        let client = MockAuthClient::new(vec![expired_json()]);
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
        let client = MockAuthClient::new(vec![declined_json()]);
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
        let client = MockAuthClient::new(vec![access_denied_json()]);
        let err = poll_token_once(&client, "http://unused", "dc_abc123")
            .await
            .expect_err("access_denied must be Err");
        assert!(
            matches!(err, AuthError::AuthorizationDeclined),
            "expected AuthorizationDeclined for access_denied, got: {err}"
        );
    }

    // ── refresh_ms_token ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn cp1_refresh_exchanges_token_successfully() {
        let client = MockAuthClient::new(vec![success_token_json()]);
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
        let client = MockAuthClient::new(vec![expired_json()]);
        let err = refresh_ms_token(&client, "http://unused", "stale_refresh")
            .await
            .expect_err("expired refresh must be Err");
        assert!(
            matches!(err, AuthError::DeviceCodeExpired),
            "expected DeviceCodeExpired, got: {err}"
        );
    }

    // ── sequential poll simulation ────────────────────────────────────────────

    /// Simulates a realistic poll sequence: two pending responses, then success.
    #[tokio::test]
    async fn cp1_poll_loop_pending_then_success() {
        let client = MockAuthClient::new(vec![
            pending_json(),
            pending_json(),
            success_token_json(),
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

    // ── CP2 stub ──────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn cp1_xbox_chain_stub_returns_not_implemented() {
        let fake_tokens = MsTokens {
            access_token: "x".to_owned(),
            refresh_token: "y".to_owned(),
            expires_in: 3600,
        };
        let err = xbox_chain_stub(fake_tokens)
            .await
            .expect_err("stub must return Err");
        assert!(
            matches!(err, AuthError::XboxChainNotImplemented),
            "expected XboxChainNotImplemented, got: {err}"
        );
    }
}
