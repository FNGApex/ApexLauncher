//! Live CurseForge smoke tests — hit the real CurseForge API.
//!
//! These are `#[ignore]`d so a plain `cargo test` never touches the network.
//! Run explicitly once a real key is exported:
//!
//! ```bash
//! source .env   # exports MODLOADER_CF_API_KEY
//! cargo test --test curseforge_live -- --ignored --nocapture
//! ```
//!
//! The key is read from the `MODLOADER_CF_API_KEY` env var via the same
//! `cf_api_key_from` resolver the production command layer uses, so these tests
//! verify the exact path the app takes — not a parallel one. If the key is
//! absent, each test fails fast with a clear message rather than silently
//! passing, so an unconfigured run is never mistaken for a green run.

use modloader_lib::core::curseforge::CurseForgeProvider;
use modloader_lib::core::providers::{
    cf_api_key_from, ModProvider, ProjectType, ReqwestProviderClient, SearchParams,
};

/// Read `MODLOADER_CF_API_KEY` out of a gitignored `.env` file at the repo root.
///
/// Needed because this suite is built and run by the Windows cargo toolchain
/// (see the project's Windows-build memory note), and WSL-exported env vars do
/// not propagate into that Windows process. Reading the file keeps the secret
/// out of the command line and out of any test transcript. Looks one level up
/// from the manifest dir (`src-tauri/`) first, then the current dir.
fn key_from_dotenv() -> Option<String> {
    for path in ["../.env", ".env"] {
        let Ok(contents) = std::fs::read_to_string(path) else {
            continue;
        };
        for line in contents.lines() {
            let line = line.trim().strip_prefix("export ").unwrap_or(line.trim());
            let Some((name, value)) = line.split_once('=') else {
                continue;
            };
            if name.trim() == "MODLOADER_CF_API_KEY" {
                let value = value.trim().trim_matches(['"', '\'']).to_string();
                if !value.is_empty() {
                    return Some(value);
                }
            }
        }
    }
    None
}

/// Resolve the key the way production does (env first), falling back to the
/// gitignored `.env` file, and failing the test loudly when neither has it so
/// an unconfigured run is never mistaken for a green run.
fn resolved_key() -> String {
    // Mirror production precedence: env var wins, then the `.env`-sourced value
    // takes the settings slot. The caller pre-fetches both, exactly as the
    // command layer in `lib.rs` does.
    let env_val = std::env::var(modloader_lib::core::providers::CF_API_KEY_ENV).ok();
    cf_api_key_from(env_val, key_from_dotenv(), None).unwrap_or_else(|| {
        panic!(
            "MODLOADER_CF_API_KEY not found in env or .env — add it to the gitignored \
             .env file (MODLOADER_CF_API_KEY=...) before running the live CurseForge tests"
        )
    })
}

fn client() -> ReqwestProviderClient {
    ReqwestProviderClient(reqwest::Client::new())
}

/// Search returns real, non-empty results and the `x-api-key` path is accepted
/// by the live API (a bad key surfaces as an HTTP 403 here, not a panic).
#[tokio::test]
#[ignore = "live network — run with --ignored once MODLOADER_CF_API_KEY is set"]
async fn cf_live_search_returns_results() {
    let provider = CurseForgeProvider::new(Some(resolved_key()));
    let http = client();
    let params = SearchParams {
        query: "jei".to_string(),
        mc_version: None,
        loader: None,
        offset: 0,
        limit: 10,
        project_type: ProjectType::Mod,
    };

    let result = provider
        .search(&http, &params)
        .await
        .expect("live CF search failed (check the key — a 403 means it was rejected)");

    assert!(
        !result.hits.is_empty(),
        "CF search for 'jei' returned zero hits — unexpected for a popular query"
    );

    eprintln!("CF search 'jei' -> {} hits:", result.hits.len());
    for hit in result.hits.iter().take(3) {
        eprintln!("  [{}] {} ({})", hit.id, hit.name, hit.slug);
    }
}

/// Version listing for a known project (JEI, CF id 238222) returns files, and
/// we surface whether any file has distribution disabled (`url == None`) — the
/// CF-specific `allowModDistribution:false` case the install path must handle.
#[tokio::test]
#[ignore = "live network — run with --ignored once MODLOADER_CF_API_KEY is set"]
async fn cf_live_get_versions_for_jei() {
    const JEI_PROJECT_ID: &str = "238222";

    let provider = CurseForgeProvider::new(Some(resolved_key()));
    let http = client();

    let versions = provider
        .get_versions(&http, JEI_PROJECT_ID, None, None)
        .await
        .expect("live CF get_versions failed");

    assert!(
        !versions.is_empty(),
        "CF get_versions for JEI ({JEI_PROJECT_ID}) returned no versions"
    );

    let distribution_disabled = versions
        .iter()
        .flat_map(|v| &v.files)
        .filter(|f| f.url.is_none())
        .count();

    eprintln!(
        "CF JEI ({JEI_PROJECT_ID}) -> {} versions; {} file(s) with distribution disabled (url=None)",
        versions.len(),
        distribution_disabled
    );
    if let Some(v) = versions.first() {
        eprintln!(
            "  newest: {} ({}) — {} file(s), game_versions={:?}, loaders={:?}",
            v.name,
            v.version_number,
            v.files.len(),
            v.game_versions,
            v.loaders
        );
    }
}
