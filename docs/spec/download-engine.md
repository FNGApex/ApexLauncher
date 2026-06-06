# Download engine (Phase 2, slice A)

## Goal

A concurrent, hash-verified, content-addressed, resumable download engine in
`core/download.rs` that executes a `DownloadPlan` (a list of `(url, dest, expected_hash,
size)` items) against any HTTP server, verifies each file's hash on completion, skips files
already present and valid, and streams progress to the UI via Tauri events. Testable in
isolation against a local mock HTTP server — no Minecraft knowledge, no resolver.

## Non-goals

- Producing a `DownloadPlan` from piston-meta — that's slice B (the resolver).
- The in-app log/download **console UI** — slice D / Phase 7 polish. Slice A emits the
  events; no React page consumes them yet.
- Persisting partial-download state across app restarts (`.part` + range on retry within a
  run is enough; cross-restart resume is out).
- CurseForge fingerprint hashing — `ExpectedHash` carries the variants the engine needs now
  (sha1, sha512); CF fingerprint lands when providers do (Phase 5).
- `rustls` migration — stay on `native-tls`.

## Success criteria

- [ ] `DownloadPlan` / `DownloadItem` / `ExpectedHash` types exist and round-trip serde.
- [ ] Executor downloads every item in a plan against a local mock server and writes each to
      its `dest` path.
- [ ] Each completed file's hash is verified against `expected_hash`; a mismatch is a
      per-item error, not a silent pass.
- [ ] An item whose `dest` already exists with a matching hash is **skipped** (no network
      request) — dedupe.
- [ ] A partial `dest` (`.part`) is resumed via an HTTP `Range` request when the server
      returns `206`; a `200` (no range support) restarts the file cleanly.
- [ ] Concurrency is bounded by a semaphore (configurable, default in 8–16 range); a plan
      larger than the bound completes without exceeding it.
- [ ] Progress is emitted through an abstraction (a sink/callback) so unit tests assert
      progress without a Tauri runtime; the Tauri-event implementation emits
      `download://progress`.
- [ ] A failed item does not abort the whole plan; the executor returns an aggregate result
      enumerating per-item success/failure.
- [ ] `cargo check` and the new `cargo test` module pass.

## Approaches

(From `docs/design/vanilla-launch.md` §A. Full table there.)

| # | Approach | Sketch | Cost | Risk |
|---|----------|--------|------|------|
| A1 | tokio `Semaphore` + `futures::buffer_unordered`, incremental hashing per chunk | async-native, bounded, streams progress | med | needs `stream` feature + `futures-util` |
| A2 | OS thread pool + blocking reqwest | simple | med | second runtime beside tokio; wastes async I/O |
| A3 | external downloader crate | less code | med | opaque verify/retry; can't match our hash/dedupe rules |

## Recommendation

**A1.** Tauri already runs a tokio runtime (`lib.rs` commands are `async fn`), so a
`tokio::sync::Semaphore` bounding `futures::stream::iter(plan).buffer_unordered(n)` is the
idiomatic fit. Hash is fed chunk-by-chunk into a `Sha1`/`Sha2` hasher as bytes stream from
`reqwest::Response::bytes_stream()`, so verification adds no extra file read. Dedupe and
content-addressing stay out of the engine: dest paths are chosen by the caller (the resolver
in B), and the engine's only dedupe rule is "dest exists + hash matches → skip."

Evidence:
- `core/meta.rs:14-23` — existing reqwest client builder + user-agent string to mirror.
- `core/store.rs:14-18` — `data_dir(app)` is the path root; the engine takes dest paths
  ready-made, it does not compute the app data dir itself.
- `lib.rs:81-89` — async command pattern (`async fn … -> Result<T, String>`) the Tauri
  command wrapper follows.
- `Cargo.toml:27` — `reqwest = { features = ["json"] }` today; `stream` must be added.

## Checkpoints

| # | Checkpoint | Files/areas | Agent | Est. files | Verifies |
|---|------------|-------------|-------|------------|----------|
| 1 | Deps + module skeleton: add `sha1`, `sha2`, `hex`, `futures-util`, `tokio` (sync), reqwest `stream` feature; create `core/download.rs` with `DownloadItem`, `DownloadPlan`, `ExpectedHash` (Sha1/Sha512 variants), `DownloadError`, and a `ProgressSink` trait; register module in `core/mod.rs` | `src-tauri/Cargo.toml`, `core/download.rs`, `core/mod.rs` | atomic-builder | ~3 | `cargo check`; unit test constructs a plan + serde round-trips the types |
| 2 | Hashing + dedupe: incremental hasher per `ExpectedHash` variant; `verify(path, expected) -> bool`; "dest exists and matches → skip" check | `core/download.rs` | atomic-builder | 1 | unit test: hash known bytes → expected hex (both variants); mismatch returns false; existing-valid-file path is detected as skip |
| 3 | Single-file download: stream body, hash incrementally, write to `dest.part`, atomic rename on verify; `Range` resume when `.part` exists (`206` resumes, `200` restarts); hash mismatch deletes `.part` and errors | `core/download.rs` | atomic-builder | 1 | unit test vs local mock server: full download verifies + lands at dest; seeded `.part` resumes via Range; mismatched hash errors and leaves no dest |
| 4 | Concurrent executor + progress + IPC seam: `Semaphore`-bounded `buffer_unordered` over the plan; per-item + aggregate progress via `ProgressSink`; Tauri-event sink emits `download://progress`; thin `#[tauri::command]` wrapper in `lib.rs` + typed `ipc.ts` entry + event payload type | `core/download.rs`, `src-tauri/src/lib.rs`, `src/lib/ipc.ts` | atomic-builder | ~3 | integration test vs mock server with a capturing sink: multi-item plan all-verified; one failing item doesn't abort others; aggregate result enumerates per-item outcome; in-flight count never exceeds the bound |

## Risks

| Risk | Likelihood | Mitigation |
|------|-----------|-----------|
| Mock HTTP server in Rust tests adds a heavy dep | med | Prefer a small dev-dep with Range support (e.g. `httpmock`/`wiremock`) or a hand-rolled `std::net::TcpListener` fixture; builder picks, keeps it `[dev-dependencies]` only |
| `native-tls` needs system OpenSSL on Linux/Windows | med | Out of scope here; Phase 7 CI migrates to `rustls`. If dev build breaks on Linux now, surface it — don't silently swap TLS backends mid-slice |
| Server ignores `Range`, returns `200` for a partial | med | Detect status: `206` resume, anything else restart from byte 0 (truncate `.part`) — covered by criterion + checkpoint 3 test |
| Progress events fire per chunk → event flood to webview | low | Throttle/coalesce in the Tauri sink (time- or byte-interval); the `ProgressSink` trait keeps the policy swappable and out of the core loop |
| Huge plans (thousands of asset objects) build one in-memory `Vec` | low | Deferred to slice B (resolver decides plan granularity); engine executes whatever list it's handed |

## Change log

<!-- Populated on first amendment after approval. -->
