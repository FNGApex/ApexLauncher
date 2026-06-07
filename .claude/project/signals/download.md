# download

## What it does

Concurrent, hash-verified download engine that executes a `DownloadPlan` (list of `(url, dest, expected_hash, size)` items), verifies each file's SHA-1 or SHA-512 hash on completion, skips files already present with a matching hash, resumes `.part` files via HTTP `Range` requests, and streams per-chunk progress through a `ProgressSink` trait. No Minecraft-specific logic; the engine is resolver-agnostic.

## CLI code

- `src-tauri/src/core/download.rs` — entire engine: `ExpectedHash` (Sha1/Sha512 variants), `DownloadItem`, `DownloadPlan`, `DownloadError` (Network/HashMismatch/Io), `ProgressUpdate`, `ProgressSink` trait, `NoOpSink`, `IncrementalHasher`, `verify`, `needs_download`, `download_item`, `execute_plan`, `build_client`; 31 unit tests via hand-rolled `tokio::net::TcpListener` mock (no `httpmock` dep); `CapturingSink` (test-only) collects progress updates
- `src-tauri/src/lib.rs` — `TauriEventSink` (emits `download://progress` Tauri event); `execute_download_plan` async Tauri command (takes `DownloadPlan` + optional `concurrency: usize`, clamps 1–32, default 8)

## Artifacts

- `src/lib/ipc.ts` — `DownloadItem`, `DownloadPlan`, `ItemStatus`, `ItemOutcome`, `PlanResult`, `DownloadProgressPayload` interfaces; `executeDownloadPlan` wrapper

## Docs

- `docs/spec/download-engine.md` — Phase 2 slice A spec: success criteria, checkpoints, accepted risk items (F-10, F-11); implementation log with commit hashes
- `docs/design/vanilla-launch.md` — design doc: approach table (A1 tokio Semaphore + buffer_unordered selected), future slices (B resolver, C Java mgr, D launch)

## Coupling

- `src/lib/ipc.ts` hand-mirrors Rust structs with camelCase rename; `DownloadItem.dest` is typed `string` (not `PathBuf`) — any Rust field rename requires manual `ipc.ts` update (no specta/ts-rs yet).
- `meta.rs` (metadata domain) builds a new `reqwest::Client` per `cached_text` call; `download.rs` uses `build_client()` for a separate shared client — two separate clients coexist.
- Slice B (vanilla resolver, `core/resolver.rs`) produces `DownloadPlan` inputs for this engine; `assemble()` constructs `DownloadItem` values using `download::DownloadItem` and `download::ExpectedHash` directly. Changes to those types require updates in `resolver.rs`.
- `execute_download_plan` command registered in `lib.rs` alongside all other domain commands — adding new IPC commands requires editing `lib.rs` across domains.

## Conventions worth knowing

- `.part` resume: `download_item` reads `dest.part` size as `resume_offset`; sends `Range: bytes={resume_offset}-`; if server returns `206` it opens `.part` in append mode and verifies actual file size against `resume_offset` (TOCTOU guard — mismatch triggers clean restart); `200` always restarts.
- Hash mismatch after download: `.part` file is deleted; error is per-item, does not abort the plan.
- `execute_plan` uses `tokio::sync::Semaphore` bounding `futures::stream::iter(plan).buffer_unordered(n)`; failed items produce `ItemStatus::Failed`, not panics.
- `CapturingSink` is `#[cfg(test)]` only — not available in production builds.
- Deps added for this module: `sha1 = "0.10"`, `sha2 = "0.10"`, `hex = "0.4"`, `futures-util = "0.3"`, `tokio = { features = ["sync"] }`; dev: `tokio = { features = ["rt", "macros", "net", "io-util"] }`.
- CurseForge fingerprint hashing is out of scope until Phase 5; only SHA-1 (Mojang assets) and SHA-512 (Modrinth) are supported now.
