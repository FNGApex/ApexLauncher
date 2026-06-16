//! Unit tests for `auth`. Extracted from the source module; wired back
//! via `#[cfg(test)] #[path = "auth_tests.rs"] mod tests;` so private items
//! remain accessible through `super::*`.

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

    async fn get_bearer(&self, _url: &str, _token: &str) -> Result<(u16, String), reqwest::Error> {
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
    r#"{"Identity":"0","XErr":2148916233,"Message":"","Redirect":"https://start.ui.com/upsell"}"#
        .to_owned()
}

/// XSTS 401 body for "region blocked" (XErr 2148916235).
fn xsts_err_region_json() -> String {
    r#"{"Identity":"0","XErr":2148916235,"Message":"","Redirect":""}"#.to_owned()
}

/// XSTS 401 body for "child account" (XErr 2148916238).
fn xsts_err_child_json() -> String {
    r#"{"Identity":"0","XErr":2148916238,"Message":"","Redirect":"https://start.ui.com/family"}"#
        .to_owned()
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

// ── F-7: poll_token_once non-200/400 guard ────────────────────────────────

/// Any status that is not 200 or 400 must return HttpStatus error without
/// attempting to parse the body as a poll response.
#[tokio::test]
async fn f7_poll_non_200_non_400_returns_http_status_error() {
    let client = MockAuthClient::new(vec![MockResp::status(503, "Service Unavailable")]);
    let err = poll_token_once(&client, "http://unused", "dc_abc123")
        .await
        .expect_err("503 must be Err");
    assert!(
        matches!(err, AuthError::HttpStatus { status: 503, .. }),
        "expected HttpStatus(503), got: {err}"
    );
}

/// Status 400 still goes through parse_poll_response (MS uses 400 for
/// error states like expired_token / authorization_declined).
#[tokio::test]
async fn f7_poll_status_400_still_parsed_as_poll_response() {
    // MS returns 400 with authorization_pending — should yield None (pending).
    let client = MockAuthClient::new(vec![MockResp::status(400, pending_json())]);
    let result = poll_token_once(&client, "http://unused", "dc_abc123")
        .await
        .expect("400 with pending body must not be Err");
    assert!(
        result.is_none(),
        "400 authorization_pending must yield None"
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
async fn cp2_xsts_without_xid_succeeds_with_empty_xuid() {
    // Some Microsoft accounts return an XSTS DisplayClaims `xui[0]` with
    // only `uhs` and no `xid`. The Minecraft identity token does not use
    // xuid, so the flow must tolerate its absence instead of failing.
    let body = r#"{
            "IssueInstant": "2026-06-12T06:40:19.0787317Z",
            "NotAfter":     "2026-06-12T22:40:19.0787317Z",
            "Token":        "xsts_token_no_xid",
            "DisplayClaims": {
                "xui": [{ "uhs": "1162757871890020810" }]
            }
        }"#;
    let client = MockAuthClient::new(vec![MockResp::ok(body.to_owned())]);
    let (token, xuid) = xsts_authorize(&client, "xbl_token_abc")
        .await
        .expect("XSTS authorize should tolerate a missing xid");

    assert_eq!(token, "xsts_token_no_xid");
    assert_eq!(xuid, "");
}

#[tokio::test]
async fn cp2_xsts_401_no_xbox_account_maps_to_named_variant() {
    let client = MockAuthClient::new(vec![MockResp::status(401, xsts_err_no_xbox_json())]);
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
    let client = MockAuthClient::new(vec![MockResp::status(401, xsts_err_region_json())]);
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
    let client = MockAuthClient::new(vec![MockResp::status(401, xsts_err_child_json())]);
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
    let client = MockAuthClient::new(vec![MockResp::status(
        404,
        r#"{"path":"/minecraft/profile"}"#,
    )]);
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

// ── CP3: keyring fake + account store tests ───────────────────────────────
//
// No real keyring is used in any test below. All tests inject a `FakeKeyring`
// (in-memory HashMap) or a `FailingKeyring` (always errors) instead.

use std::collections::HashMap;
use std::sync::Mutex as StdMutex;
use tempfile::TempDir;

/// In-memory keyring fake backed by a `HashMap`.
struct FakeKeyring {
    store: StdMutex<HashMap<String, String>>,
}

impl FakeKeyring {
    fn new() -> Self {
        FakeKeyring {
            store: StdMutex::new(HashMap::new()),
        }
    }

    /// Read a key directly — used by tests to inspect keyring state without
    /// going through `AccountStore`, which guards on `account.is_some()`.
    fn get(&self, key: &str) -> Option<String> {
        self.store.lock().unwrap().get(key).cloned()
    }
}

impl KeyringBackend for FakeKeyring {
    fn store_secret(&self, account_id: &str, secret: &str) -> Result<(), AuthError> {
        self.store
            .lock()
            .unwrap()
            .insert(account_id.to_owned(), secret.to_owned());
        Ok(())
    }

    fn load_secret(&self, account_id: &str) -> Result<String, AuthError> {
        self.store
            .lock()
            .unwrap()
            .get(account_id)
            .cloned()
            .ok_or_else(|| AuthError::Keyring(format!("no secret for {account_id}")))
    }

    fn delete_secret(&self, account_id: &str) -> Result<(), AuthError> {
        self.store.lock().unwrap().remove(account_id);
        Ok(())
    }
}

/// Arc-wrapped `FakeKeyring` so tests retain a handle after passing ownership
/// to `AccountStore` via `Box<dyn KeyringBackend>`.
struct SharedFakeKeyring(Arc<FakeKeyring>);

impl SharedFakeKeyring {
    fn new() -> (Self, Arc<FakeKeyring>) {
        let inner = Arc::new(FakeKeyring::new());
        (SharedFakeKeyring(Arc::clone(&inner)), inner)
    }
}

impl KeyringBackend for SharedFakeKeyring {
    fn store_secret(&self, account_id: &str, secret: &str) -> Result<(), AuthError> {
        self.0.store_secret(account_id, secret)
    }

    fn load_secret(&self, account_id: &str) -> Result<String, AuthError> {
        self.0.load_secret(account_id)
    }

    fn delete_secret(&self, account_id: &str) -> Result<(), AuthError> {
        self.0.delete_secret(account_id)
    }
}

/// Keyring that always errors — used to verify the named `Keyring` error variant surfaces.
struct FailingKeyring;

impl KeyringBackend for FailingKeyring {
    fn store_secret(&self, _id: &str, _secret: &str) -> Result<(), AuthError> {
        Err(AuthError::Keyring("backend unavailable".to_owned()))
    }

    fn load_secret(&self, _id: &str) -> Result<String, AuthError> {
        Err(AuthError::Keyring("backend unavailable".to_owned()))
    }

    fn delete_secret(&self, _id: &str) -> Result<(), AuthError> {
        Err(AuthError::Keyring("backend unavailable".to_owned()))
    }
}

fn make_meta(id: &str, username: &str) -> AccountMeta {
    AccountMeta {
        id: id.to_owned(),
        username: username.to_owned(),
        xuid: format!("xuid_{id}"),
        mc_token_expires: Some(9999999),
    }
}

// ── Test: AccountMeta IPC shape is camelCase; legacy disk format still loads ──

#[test]
fn cp5_account_meta_serializes_camel_case_for_ipc() {
    // ipc.ts mirrors this struct with camelCase fields (project-wide
    // `serde(rename_all = "camelCase")` convention); a snake_case key here
    // means the frontend reads `undefined`.
    let json = serde_json::to_value(make_meta("acc-1", "Steve")).expect("serialize");
    assert_eq!(json["mcTokenExpires"], serde_json::json!(9999999));
    assert!(
        json.get("mc_token_expires").is_none(),
        "snake_case key must not leak over IPC; got: {json}"
    );
}

#[test]
fn cp5_account_meta_reads_legacy_snake_case_disk_format() {
    // account.json files written before the camelCase rename used
    // `mc_token_expires`; the alias keeps them loadable.
    let legacy =
        r#"{"id":"acc-1","username":"Steve","xuid":"xuid_acc-1","mc_token_expires":12345}"#;
    let meta: AccountMeta = serde_json::from_str(legacy).expect("legacy parse");
    assert_eq!(meta.mc_token_expires, Some(12345));
}

// ── Test: failing keyring surfaces named Keyring error variant ────────────

#[test]
fn cp3_failing_keyring_surfaces_named_keyring_error() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("account.json");
    let mut store = AccountStore::load(path, Box::new(FailingKeyring)).expect("load");

    let err = store
        .set_account(make_meta("acc-1", "Steve"), "refresh")
        .expect_err("set_account with failing keyring must error");

    // Must be the named Keyring variant — not a panic, not a silent fallback.
    assert!(
        matches!(err, AuthError::Keyring(_)),
        "expected AuthError::Keyring, got: {err}"
    );
}

// ── Test: logout keyring failure leaves state intact ──────────────────────
//
// A keyring backend that stores successfully but fails on delete.

struct StoreOkDeleteFailKeyring {
    store: StdMutex<HashMap<String, String>>,
}

impl StoreOkDeleteFailKeyring {
    fn new() -> Self {
        StoreOkDeleteFailKeyring {
            store: StdMutex::new(HashMap::new()),
        }
    }
}

impl KeyringBackend for StoreOkDeleteFailKeyring {
    fn store_secret(&self, account_id: &str, secret: &str) -> Result<(), AuthError> {
        self.store
            .lock()
            .unwrap()
            .insert(account_id.to_owned(), secret.to_owned());
        Ok(())
    }

    fn load_secret(&self, account_id: &str) -> Result<String, AuthError> {
        self.store
            .lock()
            .unwrap()
            .get(account_id)
            .cloned()
            .ok_or_else(|| AuthError::Keyring(format!("no secret for {account_id}")))
    }

    fn delete_secret(&self, _id: &str) -> Result<(), AuthError> {
        Err(AuthError::Keyring("keyring delete failed".to_owned()))
    }
}

#[test]
fn cp3_logout_keyring_failure_leaves_state_unchanged() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("account.json");
    let mut store =
        AccountStore::load(path.clone(), Box::new(StoreOkDeleteFailKeyring::new())).expect("load");

    store
        .set_account(make_meta("acc-1", "Steve"), "refresh_abc")
        .expect("set_account must succeed with StoreOk backend");

    // Attempt logout — keyring delete fails.
    let err = store
        .logout()
        .expect_err("logout must fail when keyring delete fails");

    assert!(
        matches!(err, AuthError::Keyring(_)),
        "expected Keyring error, got: {err}"
    );

    // State must be unchanged — account still present in memory.
    assert!(
        store.get_account().is_some(),
        "account must still be in-memory after failed logout"
    );

    // Disk must also be unchanged — reload and verify.
    let store2 =
        AccountStore::load(path, Box::new(StoreOkDeleteFailKeyring::new())).expect("reload");
    assert!(
        store2.get_account().is_some(),
        "account must still be on disk after failed logout"
    );
}

// ── CP4 tests: refresh-at-launch (mock HTTP, no live TCP) ─────────────────
//
// Simulates the launch-time path: stored refresh token + expired MC token
// → refresh_ms_token → xbox_chain → Account with fresh MC token.
// No live HTTP. No real keyring.

#[tokio::test]
async fn cp4_refresh_at_launch_derives_fresh_mc_token() {
    // Refresh token exchange: MS returns new access+refresh tokens.
    // Then the full xbox_chain: XBL, XSTS, MC token, MC profile.
    let client = MockAuthClient::new(vec![
        // 1. refresh_ms_token call → new MS tokens
        MockResp::ok(success_token_json()),
        // 2. xbox_chain: XBL
        MockResp::ok(xbl_response_json()),
        // 3. xbox_chain: XSTS
        MockResp::ok(xsts_response_json()),
        // 4. xbox_chain: MC token
        MockResp::ok(mc_token_json()),
        // 5. xbox_chain: MC profile
        MockResp::ok(mc_profile_json()),
    ]);

    // Step 1: refresh MS token.
    let ms_tokens = refresh_ms_token(&client, "http://unused", "stored_refresh_token")
        .await
        .expect("refresh must succeed");

    assert_eq!(ms_tokens.access_token, "ms_access_xyz");

    // Step 2: run xbox_chain with the fresh MS tokens.
    let account = xbox_chain(&client, ms_tokens)
        .await
        .expect("xbox_chain must succeed after refresh");

    // The resulting account has a fresh MC access token — not the expired one.
    assert_eq!(account.mc_access_token, "mc_token_xyz");
    assert_eq!(account.username, "Steve");
    assert_eq!(account.id, "aaaabbbbccccdddd");
    assert_eq!(account.xuid, "xbox_user_id_1234");
    // Confirm this path requires no device-code prompt (no device-code response in queue).
}

// ── Client ID resolution ──────────────────────────────────────────────────

#[test]
fn client_id_defaults_to_registered_modloader_app() {
    // The default must be the modloader Azure app GUID (consumers tenant,
    // public client). The legacy official-launcher id (00000000402b5328) is
    // rejected by the AAD v2.0 device-code endpoint with AADSTS700016 —
    // see docs/design/auth-client-id-blocker.md.
    assert_eq!(DEFAULT_MS_CLIENT_ID, "82a79499-8c2e-49b8-9e42-1dd9d56252f2");
    assert_eq!(ms_client_id_from(None), DEFAULT_MS_CLIENT_ID);
}

#[test]
fn client_id_env_override_wins_but_blank_is_ignored() {
    assert_eq!(
        ms_client_id_from(Some("11111111-2222-3333-4444-555555555555".into())),
        "11111111-2222-3333-4444-555555555555"
    );
    // A set-but-empty override must not produce an empty client_id.
    assert_eq!(ms_client_id_from(Some("   ".into())), DEFAULT_MS_CLIENT_ID);
}

// ── B1 TDD anchor: single-account store set→get round-trip ───────────────
// This test is written BEFORE the single-account store exists; it fails until
// AccountStore is replaced with the single-account API.

fn make_single_store(dir: &TempDir) -> AccountStore {
    let path = dir.path().join("account.json");
    AccountStore::load(path, Box::new(FakeKeyring::new()))
        .expect("AccountStore::load should succeed on a fresh dir")
}

#[test]
fn b1_set_then_get_round_trip() {
    let dir = TempDir::new().unwrap();
    let mut store = make_single_store(&dir);

    assert!(store.get_account().is_none(), "fresh store must be empty");

    let meta = make_meta("acc-1", "Steve");
    store
        .set_account(meta.clone(), "refresh_abc")
        .expect("set_account should succeed");

    let got = store
        .get_account()
        .expect("get_account must return Some after set");
    assert_eq!(*got, meta);
}

#[test]
fn b1_logout_clears_json_and_keyring() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("account.json");
    let (shared, keyring_handle) = SharedFakeKeyring::new();
    let mut store = AccountStore::load(path.clone(), Box::new(shared)).expect("load");

    store
        .set_account(make_meta("acc-1", "Steve"), "refresh_abc")
        .expect("set");

    // Confirm file exists before logout.
    assert!(path.exists(), "account.json must exist after set");
    // Confirm keyring secret was stored.
    assert_eq!(
        keyring_handle.get(KEYRING_ACCOUNT_KEY),
        Some("refresh_abc".to_owned()),
        "keyring must hold secret after set"
    );

    store.logout().expect("logout should succeed");

    // File must be gone.
    assert!(!path.exists(), "account.json must be deleted after logout");

    // Account must be gone from memory.
    assert!(
        store.get_account().is_none(),
        "get_account must return None after logout"
    );

    // Keyring entry must be deleted — inspect the FakeKeyring directly so the
    // assert is not short-circuited by `get_refresh_token`'s account guard.
    assert_eq!(
        keyring_handle.get(KEYRING_ACCOUNT_KEY),
        None,
        "keyring entry must be deleted after logout"
    );
}

#[test]
fn b1_logout_idempotent_when_already_logged_out() {
    let dir = TempDir::new().unwrap();
    let mut store = make_single_store(&dir);

    // No account set; logout should succeed without error.
    store
        .logout()
        .expect("logout on empty store must not error");
    store.logout().expect("second logout must also not error");
}

#[test]
fn b1_login_replaces_prior_account() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("account.json");
    let mut store = AccountStore::load(path.clone(), Box::new(FakeKeyring::new())).expect("load");

    store
        .set_account(make_meta("acc-1", "Steve"), "refresh_1")
        .expect("set first");

    store
        .set_account(make_meta("acc-2", "Alex"), "refresh_2")
        .expect("set second replaces first");

    let got = store.get_account().expect("must have account");
    assert_eq!(got.id, "acc-2", "second account must replace first");
    assert_eq!(got.username, "Alex");

    // Keyring must hold the second token, not the first.
    let token = store
        .get_refresh_token()
        .expect("get_refresh_token must succeed");
    assert_eq!(
        token, "refresh_2",
        "keyring must contain the replacement refresh token"
    );
}

#[test]
fn b1_set_persists_to_disk_and_reloads() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("account.json");

    {
        let mut store =
            AccountStore::load(path.clone(), Box::new(FakeKeyring::new())).expect("load");
        store
            .set_account(make_meta("acc-1", "Steve"), "refresh_abc")
            .expect("set");
    }

    // Fresh load from disk.
    let store2 = AccountStore::load(path, Box::new(FakeKeyring::new())).expect("reload");
    let got = store2.get_account().expect("must reload account from disk");
    assert_eq!(got.id, "acc-1");
    assert_eq!(got.username, "Steve");
}

#[test]
fn b1_get_refresh_token_round_trips_via_keyring() {
    let dir = TempDir::new().unwrap();
    let mut store = make_single_store(&dir);

    store
        .set_account(make_meta("acc-1", "Steve"), "my_refresh_token")
        .expect("set");

    let token = store
        .get_refresh_token()
        .expect("get_refresh_token should succeed");
    assert_eq!(token, "my_refresh_token");
}

#[test]
fn b1_failing_keyring_surfaces_keyring_error() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("account.json");
    let mut store = AccountStore::load(path, Box::new(FailingKeyring)).expect("load");

    let err = store
        .set_account(make_meta("acc-1", "Steve"), "refresh")
        .expect_err("set_account with failing keyring must error");

    assert!(
        matches!(err, AuthError::Keyring(_)),
        "expected AuthError::Keyring, got: {err}"
    );
}

#[test]
fn b1_refresh_token_not_in_account_json() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("account.json");
    let mut store = AccountStore::load(path.clone(), Box::new(FakeKeyring::new())).expect("load");

    store
        .set_account(make_meta("acc-1", "Steve"), "super_secret_refresh")
        .expect("set");

    let raw = std::fs::read_to_string(&path).expect("read account.json");
    assert!(
        !raw.contains("super_secret_refresh"),
        "refresh token must not appear in account.json; got: {raw}"
    );
}
