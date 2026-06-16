# java

## What it does

Detects or provisions a JRE matching a required Java major version. Probes system installs (`JAVA_HOME`, `PATH`, per-OS common dirs, `<data>/java/<major>/`) first; on miss, fetches Temurin metadata from Adoptium `/v3/assets/latest/<major>/hotspot`, executes the download through the shared download engine (SHA-256 verification), and extracts the archive in-process with traversal-safe tar.gz/zip unpacking. Returns a `JavaInstallation` (major + absolute path to `java`/`java.exe`) for slice D to spawn.

## CLI code

- `src-tauri/src/core/java.rs` (752 lines) — full implementation: `TargetOs` (Linux/MacOs/Windows, injectable), `JavaInstallation` (major + path + source), `JavaSource` (Detected/Downloaded enum), `parse_major_from_release` (handles modern `"17.0.8"` and legacy `"1.8.0_392"` schemes), `probe_installation`, `detect` (injectable candidates + cache_prefix for source labelling), `default_candidates` (env+filesystem reads; not under test), `ArchiveKind` (TarGz/Zip), `adoptium_query_url`, `adoptium_arch`, `parse_adoptium_response` (fixture-tested), `provision_java` (async; real HTTP; not unit-tested), `extract_archive` → `extract_tar_gz` / `extract_zip` (traversal guard via `normalize_path` + prefix-check on every entry), `locate_java_bin` (recursive walk for `bin/java[.exe]`), `ensure_java_core` (injectable detect-or-provision; unit-tested without network), `ensure_java` (thin `AppHandle` wrapper; real network); ends with a `#[cfg(test)] #[path = "java_tests.rs"] mod tests;` stub
- `src-tauri/src/core/java_tests.rs` (779 lines) — the 39 tests (36 `#[test]` + 3 `#[tokio::test]`) for the above, relocated out of `java.rs` via the `#[path]` stub; test count and content unchanged by the move
- `src-tauri/src/core/store.rs` — `java_dir` fn: returns `<data>/java/`, creates on demand; downloaded JREs land at `<data>/java/<major>/`
- `src-tauri/src/lib.rs` — `ensure_java` Tauri command (wraps `core::java::ensure_java`); registered in `invoke_handler`

## Artifacts

- `src/lib/ipc.ts` — `JavaSource` type, `JavaInstallation` interface, `ensureJava(major)` wrapper

## Docs

- `docs/spec/java-manager.md` — Phase 2 slice C spec: success criteria, approach table, checkpoint plan, risks, implementation log (shipped 2026-06-07, 39 tests)
- `docs/design/vanilla-launch.md` — design doc §C: detect-first + Temurin-download decision; slices B/C/D context

## Coupling

- `download` domain: `parse_adoptium_response` constructs `download::DownloadItem` with `ExpectedHash::Sha256` (added in slice C); `provision_java` calls `download::execute_plan` + `download::DownloadPlan`; `ensure_java` uses `download::NoOpSink`. Any breaking change to `DownloadItem`, `ExpectedHash`, or `execute_plan` signatures requires updates here.
- `instances` domain: `Instance.java` field (`JavaCfg.major`) is the source of the `major` value passed to `ensure_java`; wiring `JavaCfg.major` into the launch argv is slice D, not implemented yet.
- `ipc.ts` hand-mirrors `JavaInstallation` and `JavaSource`; no generated types. Rust field renames require manual `ipc.ts` update.
- `lib.rs` command dispatch: `ensure_java` is registered alongside all other domain commands; adding new commands requires editing `lib.rs` across domains.

## Conventions worth knowing

- `detect` is fully injectable (candidates list + OS + optional cache_prefix) — no env reads or filesystem side effects. Tests pass fixture directories directly to `detect` / `probe_installation`; `default_candidates` (env+filesystem) is not called in tests.
- Tests live in the sibling `java_tests.rs`, not inline in `java.rs` — wired back via `#[path = "java_tests.rs"] mod tests;` at the end of `java.rs`. This is a mechanical relocation applied repo-wide; no test logic changed.
- Source labelling: JRE homes whose path starts with the `cache_prefix` (`<data>/java/`) are labelled `JavaSource::Downloaded`; all others `JavaSource::Detected`.
- `probe_installation` requires a `release` file with a parseable `JAVA_VERSION=` line — no `java -version` shell-out. If `release` is absent, the candidate is skipped.
- Adoptium OS param: `"mac"` not `"osx"` (Adoptium convention; differs from Mojang). Guards against regression at `adoptium_os_macos_is_mac_not_osx` test.
- Archive extension detection uses `ends_with` (`.tar.gz` / `.zip`) not `Path::extension()` — `extension()` returns `"gz"` for `.tar.gz`, not `"tar.gz"`.
- Traversal guard: `normalize_path` resolves `..`/`.` lexically (no `canonicalize` — target path doesn't exist yet); every entry's resolved path is prefix-checked against the canonicalized dest before any write.
- Temurin archives nest under a versioned top dir (e.g. `jdk-17.0.8+7-jre/bin/java`); `locate_java_bin` walks the extracted tree recursively for the first `bin/java[.exe]` match — not a fixed depth.
- Extraction dest is explicitly `<cache_dir>/<major>/` — not derived from the archive's parent path. This is the F-4 invariant, tested in `extract_dest_is_major_scoped_dir`.
- New Cargo deps added for this module: `flate2`, `tar`, `zip`; dev: `tempfile`.
- `src-tauri/src/core/fixtures/adoptium_latest.json` — fixture file embedded via `include_str!` for `parse_adoptium_response` tests.
