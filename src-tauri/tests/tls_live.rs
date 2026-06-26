//! Live TLS smoke — proves the rustls/webpki-roots stack completes a real
//! HTTPS handshake against the public services the launcher actually uses.
//!
//! `#[ignore]`d so a plain `cargo test` never touches the network. These hit
//! **keyless** endpoints (Modrinth + Mojang piston-meta), so they need no
//! secrets — unlike `curseforge_live.rs`. Run explicitly:
//!
//! ```bash
//! cargo test --test tls_live -- --ignored --nocapture
//! # or: scripts/build.sh test --test tls_live -- --ignored
//! ```
//!
//! The unit suite mocks HTTP (injectable `ProviderHttpClient` seam) and never
//! opens a TLS socket; this is the one check that exercises the actual transport
//! after the native-tls → rustls switch (`docs/spec/rustls-tls-switch.md`, CP-2).
//! Both calls go through the production `ReqwestProviderClient`, not a parallel
//! client, so a green run proves the exact path the app takes.

use modloader_lib::core::providers::{ProviderHttpClient, ReqwestProviderClient};

#[tokio::test]
#[ignore = "live network; run with --ignored"]
async fn modrinth_https_handshake_over_rustls() {
    let client = ReqwestProviderClient(reqwest::Client::new());
    let (status, _body) = client
        .get("https://api.modrinth.com/v2/search?limit=1", &[])
        .await
        .expect("rustls handshake + GET to Modrinth should succeed");
    assert_eq!(status, 200, "Modrinth search over rustls returned non-200");
}

#[tokio::test]
#[ignore = "live network; run with --ignored"]
async fn mojang_piston_meta_https_handshake_over_rustls() {
    let client = ReqwestProviderClient(reqwest::Client::new());
    let (status, _body) = client
        .get(
            "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json",
            &[],
        )
        .await
        .expect("rustls handshake + GET to Mojang piston-meta should succeed");
    assert_eq!(status, 200, "Mojang piston-meta over rustls returned non-200");
}
