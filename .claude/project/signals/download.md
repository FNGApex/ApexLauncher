# download

## What it does

Concurrent, hash-verified download engine that executes a `DownloadPlan` (list of `(url, dest, expected_hash, size)` items), verifies each file's SHA-1, SHA-256, or SHA-512 hash on completion, skips files already present with a matching hash, resumes `.part` files via HTTP `Range` requests with a TOCTOU guard (detects `.part` file growth between metadata read and append-open; triggers a clean full-GET restart), and streams per-chunk progress through a `ProgressSink` trait. No Minecraft-specific logic; the engine is resolver-agnostic.

## CLI code

- `src-tauri/src/core/download.rs` (618 lines) — entire engine: `ExpectedHash` (Sha1/Sha256/Sha512 variants), `DownloadItem`, `DownloadPlan`, `DownloadError` (Network/HashMismatch/Io), `ProgressUpdate`, `ProgressSink` trait, `NoOpSink`, `IncrementalHasher`, `verify`, `needs_download`, `download_item`, `seed_hasher_from_file` (seeds hasher from partial `.part` bytes on resume), `execute_plan`, `build_client`; `CapturingSink` (test-only mock, stays module-scope) collects progress updates; ends with a 3-line `#[cfg(test)] #[path = "download_tests.rs"] mod tests;` stub
- `src-tauri/src/core/download_tests.rs` (1262 lines) — 37 unit tests (`#[test]`/`#[tokio::test]`) via hand-rolled `tokio::net::TcpListener` mock (no `httpmock` dep), wired back into `download.rs` via the `#[path]` stub
- `src-tauri/src/lib.rs` — `TauriEventSink` (emits `download://progress` Tauri event); `execute_download_plan` async Tauri command (takes `DownloadPlan` + optional `concurrency: usize`, clamps 1–32, default 8)

## Artifacts

- `src/lib/ipc.ts` — `DownloadItem` (expectedHash union includes sha1/sha256/sha512), `DownloadPlan`, `ItemStatus`, `ItemOutcome`, `PlanResult`, `DownloadProgressPayload` interfaces; `executeDownloadPlan` wrapper

## Docs

- `docs/spec/download-engine.md` — Phase 2 slice A spec: success criteria, checkpoints, accepted risk items (F-10, F-11); implementation log with commit hashes
- `docs/design/vanilla-launch.md` — design doc: approach table (A1 tokio Semaphore + buffer_unordered selected), future slices (B resolver, C Java mgr, D launch)

## Coupling

- `src/lib/ipc.ts` hand-mirrors Rust structs with camelCase rename; `DownloadItem.dest` is typed `string` (not `PathBuf`) — any Rust field rename requires manual `ipc.ts` update (no specta/ts-rs yet).
- `meta.rs` (metadata domain) builds a new `reqwest::Client` per `cached_text` call; `download.rs` uses `build_client()` for a separate shared client — two separate clients coexist.
- Slice B (vanilla resolver, `core/resolver.rs`) produces `DownloadPlan` inputs for this engine; `assemble()` constructs `DownloadItem` values using `download::DownloadItem` and `download::ExpectedHash` directly. Changes to those types require updates in `resolver.rs`.
- Slice C (java domain, `core/java.rs`) uses `DownloadItem`, `ExpectedHash::Sha256`, `DownloadPlan`, `execute_plan`, and `NoOpSink` directly. Breaking changes to these types cascade to `java.rs`.
- `execute_download_plan` command registered in `lib.rs` alongside all other domain commands — adding new IPC commands requires editing `lib.rs` across domains.

## Conventions worth knowing

- `.part` resume: `download_item` reads `dest.part` size as `resume_offset`; sends `Range: bytes={resume_offset}-`; if server returns `206` it opens `.part` in append mode then re-reads actual size — if actual ≠ resume_offset (TOCTOU guard: another writer modified the file), it truncates `.part` and issues a fresh unconditional GET from byte 0. `200` always restarts.
- On 206 resume, `seed_hasher_from_file` reads the first `resume_offset` bytes from `.part` into the hasher before streaming the remainder.
- Hash mismatch after download: `.part` file is deleted; error is per-item, does not abort the plan.
- `execute_plan` uses `tokio::sync::Semaphore` bounding `futures::stream::FuturesUnordered`; failed items produce `ItemStatus::Failed`, not panics.
- `CapturingSink` is `#[cfg(test)]` only — not available in production builds.
- Deps added for this module: `sha1 = "0.10"`, `sha2 = "0.10"`, `hex = "0.4"`, `futures-util = "0.3"`, `tokio = { features = ["sync"] }`; dev: `tokio = { features = ["rt", "macros", "net", "io-util"] }`.
- CurseForge fingerprint hashing is out of scope until Phase 5; SHA-1 (Mojang assets), SHA-256 (Adoptium Temurin JREs), and SHA-512 (Modrinth) are the three hash variants now supported.
