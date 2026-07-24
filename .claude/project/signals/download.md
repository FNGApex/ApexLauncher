# download

## Overview

Concurrent hash-verified download engine. Executes a `DownloadPlan` (list of `DownloadItem { url, dest, expected_hash, size }`), verifies SHA-1/SHA-256/SHA-512/MD5 on completion, skips files already present with matching hash, resumes `.part` files via HTTP `Range` with a TOCTOU guard, streams per-chunk progress through a `ProgressSink` trait. `CancelToken` (`Arc<AtomicBool>`) checked at per-item boundaries. No Minecraft-specific logic.

## CLI code

- `src-tauri/src/core/download.rs` — `ExpectedHash` (Sha1/Sha256/Sha512/Md5), `DownloadItem`, `DownloadPlan`, `DownloadError`, `ProgressUpdate`, `ProgressSink`, `NoOpSink`, `IncrementalHasher` (four variants via sha1/sha2/md-5 crates), `verify`, `needs_download`, `download_item`, `seed_hasher_from_file`, `execute_plan`, `execute_plan_cancellable`, `build_client`; `CancelToken` (`cancel()`, `is_cancelled()`, `Default` = uncancelled); `CapturingSink` (test-only); ends with `#[cfg(test)] #[path = "download_tests.rs"] mod tests;`
- `src-tauri/src/core/download_tests.rs` — 44 unit tests using hand-rolled `tokio::net::TcpListener` mock
- `src-tauri/src/lib.rs` — `TauriEventSink` (emits `download://progress`); `execute_download_plan` async Tauri command (takes `DownloadPlan` + optional `concurrency: usize`, clamps 1–32, default 8)

## Artifacts

- `src/lib/ipc.ts` — `DownloadItem` (expectedHash union: sha1/sha256/sha512/md5), `DownloadPlan`, `ItemStatus`, `ItemOutcome`, `PlanResult`, `DownloadProgressPayload`; `executeDownloadPlan` wrapper

## Docs

- `docs/spec/download-engine.md` — Phase 2 slice A spec
- `docs/spec/download-runner-rework/cp-1-download-cancel-seam.md` — `CancelToken` + `execute_plan_cancellable`

## Coupling

- `CancelToken` imported by `task_manager.rs`; `execute_plan_cancellable` called by pack/mod job impls in `lib.rs`.
- `resolver.rs` produces `DownloadPlan` inputs; changes to `DownloadItem`/`ExpectedHash` require updates there.
- `java.rs` uses `DownloadItem`, `ExpectedHash::Sha256`, `execute_plan`, `NoOpSink` directly.
- ATL install (`ImportAtlJob`) uses `ExpectedHash::Md5`; requires `md-5` crate dep.

## Conventions

- `.part` resume: reads `dest.part` size → `Range: bytes={n}-`; on `206` opens in append mode, re-reads actual size — TOCTOU guard: if actual ≠ expected, truncates `.part` and issues fresh GET. `200` always restarts.
- `seed_hasher_from_file` feeds first `resume_offset` bytes into the hasher before streaming remainder.
- Hash mismatch: `.part` deleted; error is per-item, does not abort the plan.
- `execute_plan` uses `tokio::sync::Semaphore` bounding `FuturesUnordered`; failed items produce `ItemStatus::Failed`.
- `execute_plan_cancellable` checks `cancel.is_cancelled()` before each item; returns partial results when tripped.
- `CapturingSink` is `#[cfg(test)]` only.
- Hash variants: SHA-1 (Mojang assets), SHA-256 (Adoptium Temurin JREs), SHA-512 (Modrinth), MD5 (ATLauncher mod jars).
