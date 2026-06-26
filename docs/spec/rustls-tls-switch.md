# Spec: reqwest native-tls → rustls-tls switch

> Workstream 1 of the Phase-7 ship slices (precedes dark/light themes + instance icons).
> Roadmap line: "Switch reqwest `native-tls` → `rustls-tls` before CI to drop the OpenSSL
> build dependency." (`docs/ROADMAP.md:121`)
> Build/test ONLY via `scripts/build.sh` (`check`, `test [filter]`, `dev`). No DTO/command/
> event change here, so **no `bindings.ts` regen** at any checkpoint.

Contained, dependency-only change: the Rust code references **no** TLS types, so the entire
switch is one `Cargo.toml` line plus verification. Each checkpoint ends **runnable**
(`scripts/build.sh check` + full `scripts/build.sh test` green).

---

## Decision & rationale (the only design surface)

**Switch reqwest's TLS backend from `default-tls` (native-tls → OpenSSL on Linux) to
`rustls-tls`, rooted on the bundled Mozilla `webpki-roots` set.**

Why webpki-roots (the default that `rustls-tls` selects) and **not** `rustls-tls-native-roots`:

- The launcher only ever hits **public** TLS endpoints with certificates chaining to public
  CAs — Mojang/piston-meta, Modrinth, CurseForge, Adoptium (evidence trail below). It never
  talks to a private/intranet host or a corporate-MITM proxy, so it has no need to consult the
  OS trust store.
- `webpki-roots` compiles the Mozilla CA bundle **into the binary** — zero runtime dependency
  on an OS cert store. That makes builds and runs **deterministic across CI runners** (no
  `ca-certificates` package, no `rustls-native-certs` platform shims), which is the whole point
  of the switch.
- `rustls-tls-native-roots` would reintroduce a platform dependency (`rustls-native-certs`
  reading the OS store) — the opposite of the CI-friendliness we want, for a capability we
  don't use.

Tradeoff recorded: with `webpki-roots` the trusted-root set is frozen at build time and only
updates when we bump the `webpki-roots` crate. For a launcher hitting a fixed handful of major
public services this is fine; a stale bundle is refreshed by a routine `cargo update`. If we
ever need to trust a user's corporate proxy CA, revisit `rustls-tls-native-roots` then.

**Crypto provider:** reqwest 0.12.28 + rustls 0.23 resolve to the **`ring`** provider
(`aws-lc-rs` is absent from `Cargo.lock`; `rustls-webpki` → `ring`). `ring` builds with a plain
C compiler (`cc`) — no `cmake`/`nasm`/`perl`/OpenSSL-dev, all of which every GitHub Actions
runner already has. This is the concrete build-prereq win over OpenSSL.

---

## Evidence trail (file:line / source)

| Fact | Source |
|------|--------|
| Current dep: `reqwest = { version = "0.12", features = ["json", "stream"] }` (keeps reqwest's **default** features → `default-tls`/native-tls) | `src-tauri/Cargo.toml:30` |
| reqwest default features = `charset, default-tls, http2, macos-system-configuration` | docs.rs/crate/reqwest/0.12.9/features (verified) |
| `rustls-tls` feature pulls in `rustls-tls-webpki-roots` → `webpki-roots` (Mozilla bundle) | docs.rs reqwest features (verified) |
| Resolved reqwest = 0.12.28; rustls = 0.23.40; **`ring`** present, **`aws-lc-rs` absent** | `src-tauri/Cargo.lock` (`name = "reqwest"` 0.12.28; `name = "rustls"` 0.23.40; `ring` present; no `aws-lc-rs`) |
| OpenSSL chain present today: `openssl-sys`, `native-tls`, `hyper-tls`, `tokio-native-tls` — all rooted only at reqwest 0.12's `default-tls` | `src-tauri/Cargo.lock` (`native-tls` reverse-deps = hyper-tls / reqwest 0.12 / tokio-native-tls) |
| Second reqwest in tree (`0.13.4`, transitive) lists **no** TLS deps → not an OpenSSL source | `src-tauri/Cargo.lock` (reqwest 0.13.4 dep block) |
| `hyper-rustls` + `rustls-pki-types` already in reqwest 0.12.28's resolved deps (rustls infra already partly built) | `src-tauri/Cargo.lock` (reqwest 0.12.28 dep block) |
| **No** TLS config anywhere in the code: no `use_native_tls`/`use_rustls_tls`/`add_root_certificate`/`danger_accept_invalid_certs`/`Identity`/`Certificate` | `src-tauri/src/**` (full-tree grep — zero `native-tls`/`native_tls`/`openssl`/`rustls`/`webpki` strings) |
| 4 client builders set only `.user_agent()` then `.build()` | `core/meta.rs:15-22`, `core/download.rs:294-297`, `core/java.rs:447-454`, `core/forge_installer.rs:315-322` |
| Many ad-hoc `reqwest::Client::new()` (no builder, no TLS opts) | `src-tauri/src/lib.rs` (begin_login 126, launch refresh 1145, providers 1360/1393/1430/1519/1669/2004/2265/2961/3194/3206/3396/3572/3584, archive dl 3237/3601) |
| Only reqwest types referenced: `reqwest::Client`, `reqwest::Error`, `reqwest::StatusCode::PARTIAL_CONTENT` — all backend-agnostic, unchanged under rustls | `core/download.rs:383` (PARTIAL_CONTENT), `core/auth.rs:198/235/242/249`, `core/providers.rs:230/242` |
| No response decompression in use (`gzip`/`brotli`/`deflate` reqwest features never enabled) → no encoding regression risk | `src-tauri/Cargo.toml:30` (features list); full-tree grep (no `.gzip(`/`.brotli(`) |
| `keyring` uses platform-native backends (apple-native / windows-native / secret-service) — **not** OpenSSL; unaffected by this switch | `src-tauri/Cargo.toml:39` |
| Public TLS endpoints only (no private/MITM hosts) | piston-meta/Mojang (`core/versions.rs`, `core/meta.rs`), Modrinth/CurseForge (`core/modrinth.rs`, `core/curseforge.rs`), Adoptium (`core/java.rs`) |
| Live-TLS integration test exists but is `#[ignore]` (network) | `src-tauri/tests/curseforge_live.rs` |
| Unit tests mock HTTP (injectable `ProviderHttpClient`/`AuthHttpClient` seams) — they do **not** exercise a real TLS handshake | `core/providers.rs:246` (`ReqwestProviderClient`), `core/auth.rs:253` (`ReqwestAuthClient`) |

---

## Checkpoint table

| CP | Goal | Files touched | Tests / verification | bindings regen? | Runnable gate |
|----|------|---------------|----------------------|-----------------|---------------|
| **CP-1** | Swap the TLS backend to rustls/webpki-roots | `src-tauri/Cargo.toml:30` (the one reqwest line); `src-tauri/Cargo.lock` (regenerated by the build — commit it) | No new unit tests (no code change; behavior covered by the existing mock-HTTP suite). Dep-graph assertions in the gate below. | **No** | `scripts/build.sh check` green; **full** `scripts/build.sh test` green (679 lib tests; the known `cp4_concurrency_bound_not_exceeded` flake aside); `cargo tree -i openssl-sys` reports **package not found**; `cargo tree -i webpki-roots` now **resolves**; `cargo tree -i ring` resolves and `aws-lc-rs` stays absent |
| **CP-2** | Prove a real rustls handshake against a live host | none (verification-only) | Run the ignored live test: `scripts/build.sh test --test curseforge_live -- --ignored` (or, if the harness can't forward `--test`, a documented manual `scripts/build.sh dev` smoke: Browse loads a CF + a Modrinth feed, install a small pack, log in via MS device-code — each is a live HTTPS round-trip through the new stack) | **No** | The live CurseForge fetch succeeds over rustls (HTTP 200, parsed JSON); a manual `dev` smoke confirms Modrinth browse, a pack install download, and MS auth all complete — no `tls`/`certificate`/handshake errors in the log |

---

## Per-checkpoint detail

### CP-1 — the dependency change
Replace `src-tauri/Cargo.toml:30`:

```toml
# before
reqwest = { version = "0.12", features = ["json", "stream"] }
# after
reqwest = { version = "0.12", default-features = false, features = [
  "json", "stream", "rustls-tls", "charset", "http2", "macos-system-configuration",
] }
```

Rationale for each retained feature (all were ON via the old default set — drop none to avoid
a silent regression):
- `rustls-tls` — the new backend (selects `rustls-tls-webpki-roots`, the Mozilla bundle).
- `charset` — `encoding_rs`; preserves `Response::text()` charset handling that `default-features`
  gave us.
- `http2` — keeps h2 negotiation against Cloudflare-fronted CF/Modrinth/Adoptium (was a default;
  dropping it would silently fall back to HTTP/1.1).
- `macos-system-configuration` — macOS proxy autodetection (was a default; the dep is
  macOS-target-gated, so enabling it unconditionally is harmless on Win/Linux).
- `json`, `stream` — unchanged (the engine streams bodies; providers parse JSON).

`default-features = false` removes exactly one thing: `default-tls`. That is what drops
`hyper-tls` → `native-tls` → `tokio-native-tls` → `openssl-sys`/`openssl` from the graph (their
only root is reqwest 0.12's `default-tls`; the transitive reqwest 0.13.4 carries no TLS deps).

No `.rs` edits: nothing in the code names a TLS type, picks a backend, adds a root cert, or
toggles cert verification (evidence trail). `reqwest::StatusCode::PARTIAL_CONTENT` and
`reqwest::Error` are backend-agnostic and compile unchanged.

**Verification gate (the substance of this CP)** — run on the native Windows toolchain via
`scripts/build.sh`; the `cargo tree` probes can also run under WSL-native cargo for IDE
purposes:
- `cargo tree -i openssl-sys` → **error: package ID not found** (OpenSSL is gone). If it still
  resolves, an unexpected consumer pulls it — list the reverse-dep path and resolve before
  declaring CP-1 done (do **not** paper over with `default-tls` re-enabled).
- `cargo tree -i webpki-roots` → now resolves (the Mozilla bundle is compiled in).
- `cargo tree -i ring` → resolves; `cargo tree -i aws-lc-rs` → not found (confirms the
  no-extra-build-tooling `ring` provider, not aws-lc-rs).
- `scripts/build.sh check` then full `scripts/build.sh test` stay green.

### CP-2 — live TLS smoke (unit tests can't cover this)
The unit suite mocks HTTP through the `ProviderHttpClient`/`AuthHttpClient` seams, so a green
`test` run proves the code compiles and the logic holds but **never opens a real TLS socket**.
CP-2 exercises the actual rustls handshake exactly once:
- Preferred: the existing ignored live test — `scripts/build.sh test --test curseforge_live --
  --ignored` (needs a CF API key in env/settings, per that test's own preconditions).
- If the build harness cannot forward the `--test`/`--ignored` flags, fall back to a documented
  manual `scripts/build.sh dev` smoke and tick all three transport paths: a provider **browse**
  (CF + Modrinth GET), a **pack install** (download-engine stream over rustls), and **MS
  device-code login** (auth chain over rustls). Any of these failing with a TLS/certificate
  error means the root strategy is wrong — but with public CAs + webpki-roots it should not.

---

## Risks & non-issues
- **keyring / OS secrets — unaffected.** keyring uses apple-native / windows-native /
  secret-service, never OpenSSL (`Cargo.toml:39`). The refresh-token store keeps working.
- **Tauri's own networking — out of scope.** Tauri/webview TLS is the platform WebView2/WebKit
  stack, independent of reqwest. This change touches only our Rust HTTP clients.
- **`http2` kept deliberately.** Removing it is the one easy way to introduce a behavior change
  (forced HTTP/1.1); the feature list above keeps it.
- **No `.part`/resume regression.** Range-resume relies on `StatusCode::PARTIAL_CONTENT`
  (`download.rs:383`), which is transport-agnostic.

---

## Change log
- 2026-06-26 — Initial spec authored. Not implemented. Decision: `rustls-tls` with bundled
  `webpki-roots` (not native-roots), `ring` crypto provider (no aws-lc-rs / no OpenSSL-dev).
  Verified against `Cargo.lock` (reqwest 0.12.28 / rustls 0.23.40 / ring present, aws-lc-rs
  absent) and reqwest 0.12 feature docs. Two CPs: (1) the `Cargo.toml` feature swap +
  dep-graph gates, (2) a live-TLS smoke since unit tests mock HTTP. No `bindings.ts` regen.
