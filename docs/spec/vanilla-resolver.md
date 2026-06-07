# Vanilla resolver (Phase 2, slice B)

## Goal

Turn a Minecraft version id into a `DownloadPlan` (executed by the slice-A engine) plus a
`LaunchMeta` (consumed by slice D), by fetching and parsing the piston-meta version manifest
and asset index. No downloads, no JVM — pure metadata → plan + launch contract.

## Non-goals

- Executing the plan — slice A owns the engine; this slice only produces the plan.
- Spawning the JVM, building the final argv, extracting natives — slice D.
- Java provisioning — slice C. Resolver only surfaces the required Java major.
- Mod loaders (Fabric/Forge/Quilt/NeoForge) — Phase 4. Vanilla manifest only.
- Online auth identity — Phase 3. `LaunchMeta` carries arg *templates*; placeholder
  substitution is slice D's job.
- `rustls` migration — Phase 7. Stay on `native-tls`.
- Generated TS types — cross-cutting. `ipc.ts` stays hand-mirrored.

## Success criteria

- [ ] Given a version id, resolver fetches that version's manifest via the
      `version_manifest_v2` entry's `url` (with its `sha1`), parsed into typed structs.
- [ ] Per-version manifest JSON is disk-cached via `meta::cached_text`, keyed by version id,
      with a long TTL (manifest is immutable per id).
- [ ] Library `rules` are evaluated for the current OS; disallowed libraries are excluded
      from both classpath and plan.
- [ ] Native libraries are identified per current OS (classifier from `natives` map +
      `downloads.classifiers`) and surfaced in `LaunchMeta` for slice-D extraction.
- [ ] Asset index is fetched (cached by index id) and every object becomes a `DownloadItem`:
      `url = <resources base>/<2hex>/<sha1>`, `dest = assets/objects/<2hex>/<sha1>`,
      `expected_hash = Sha1`, `size` set.
- [ ] Dest paths are content-addressed/conventional under the app data dir: libraries at
      their Maven `path`, client jar at `versions/<id>/<id>.jar`, asset index at
      `assets/indexes/<id>.json`, asset objects under `assets/objects/`.
- [ ] Every `DownloadItem` with a known hash uses `ExpectedHash::Sha1` (vanilla is sha1).
- [ ] `LaunchMeta` carries: `main_class`, jvm + game argument templates (modern structured
      `arguments` and legacy `minecraftArguments` string both handled), `asset_index_id`,
      `assets_legacy` flag, `java_major`, classpath entries (libs + client jar), natives list,
      `version_id`.
- [ ] A `resolve_vanilla` Tauri command exposes the resolver; `ipc.ts` mirrors the returned
      types (camelCase via serde `rename_all`).
- [ ] Unit tests parse recorded fixtures (≥1 modern manifest, ≥1 legacy manifest with OS
      library rules, ≥1 asset index) and assert the produced plan + `LaunchMeta`. No live HTTP
      in tests.
- [ ] `cargo test` green from `src-tauri/`; `npm run build` (tsc) green.

## Approaches

Copied from `docs/design/vanilla-launch.md` §B; the plan/execute seam is already fixed by
slice A.

| # | Approach | Sketch | Cost | Risk |
|---|----------|--------|------|------|
| A | Reuse `meta::cached_text`, parse into typed structs, emit `DownloadPlan` + `LaunchMeta` | resolver owns dest-path layout; engine stays generic | low | manifest schema variance across MC versions (legacy vs modern args, os rules) |
| B | Parse `serde_json::Value` ad hoc (like `versions.rs`) | less struct boilerplate | low | fragile field access; no compile-time schema; harder to test |
| C | Pull a third-party MC-meta crate | less parsing code | med | opaque layout decisions; may not match our content-addressed dest rules |

## Recommendation

**A.** Typed structs make the schema explicit and fixture-testable, which is the whole point
of the plan/execute seam (`docs/design/vanilla-launch.md:56`). `versions.rs:29` uses ad-hoc
`Value` access because it reads one field; the resolver reads dozens across nested
rule/native/classifier structures, where typed `#[derive(Deserialize)]` pays off. Dest-path
layout lives in the resolver, not the engine, so the engine stays Minecraft-agnostic
(`download.rs:5`). Asset objects are emitted as one flat plan — slice A executes whatever
list it is handed (`docs/design/vanilla-launch.md:124`); no chunking in B.

## Checkpoints

| # | Checkpoint | Files/areas | Agent | Est. files | Verifies |
|---|------------|-------------|-------|------------|----------|
| 1 | Version manifest fetch + parse: resolve per-version `url`+`sha1` from `version_manifest_v2`, fetch+cache version JSON by id, deserialize into typed structs (client download, `javaVersion`, `mainClass`, `arguments`/`minecraftArguments`, `assetIndex`, `libraries`) | `src-tauri/src/core/resolver.rs` (new), `core/mod.rs` | atomic-builder | ~2 | Fixture test: parse a recorded modern version JSON → assert client url/sha1/size, main class, java major, asset-index id |
| 2 | Library rule eval + classpath/natives selection: evaluate `rules` (os allow/disallow) for current platform; pick `downloads.artifact` + native classifier per OS; compute Maven dest paths | `src-tauri/src/core/resolver.rs` | atomic-builder | 1 | Fixture test (legacy manifest w/ os-ruled libs): assert platform-filtered classpath set + correct natives selected for each OS |
| 3 | Asset index resolution: fetch+cache asset index by id, map every object → `DownloadItem` (resources-base url, `objects/<2hex>/<sha1>` dest, sha1 hash, size); emit asset-index file item + `assets_legacy` flag | `src-tauri/src/core/resolver.rs` | atomic-builder | 1 | Fixture asset-index test: assert object count, sample item url/dest/hash, index-file item present |
| 4 | Assemble + wire: combine client jar + libs + natives + asset-index file + asset objects into one `DownloadPlan`; build `LaunchMeta`; expose `resolve_vanilla` command; mirror types in `ipc.ts` | `src-tauri/src/core/resolver.rs`, `src-tauri/src/lib.rs`, `src/lib/ipc.ts` | atomic-builder | ~3 | End-to-end fixture test: resolve full recorded version → assert total plan item count + `LaunchMeta` fields; `cargo check`; `npm run build` |

## Risks

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Manifest schema drift: modern `arguments` (object) vs legacy `minecraftArguments` (string), pre-1.13 | high | Both fixtures in CP1/CP2; `#[serde(default)]` + enum/Option for the divergent fields |
| OS rule evaluation wrong (wrong natives or excluded lib) → broken launch at slice D | med | CP2 fixture asserts per-OS selection explicitly; pick a version known to use os rules (e.g. LWJGL natives) |
| Asset index size: thousands of objects → large in-memory plan | low | Accepted; design defers chunking (`vanilla-launch.md:124`). One flat plan; revisit only if memory bites at slice D |
| IPC type drift: `ipc.ts` hand-mirror diverges from `LaunchMeta`/`DownloadPlan` | med | CP4 updates `ipc.ts` in the same slice; `npm run build` gate catches TS breakage |
| `javaVersion` absent on very old manifests | low | Default to major 8 when missing (pre-1.17 floor is Java 8); note in resolver |

## Open questions

- **Resources base URL:** asset objects come from `https://resources.download.minecraft.net/`.
  Confirm no per-version override exists in the manifest (none observed) — hardcode with a
  const, same pattern as `versions.rs:14`.
- **Logging config:** Mojang ships `logging.client` (a log4j2 xml + `${path}` jvm arg).
  Include as a plan item + `LaunchMeta` field now, or defer to slice D? Lean: include the
  download in CP4 (it's one small file the launch arg references), but treat the jvm arg
  substitution as slice D's concern.
- **Legacy assets (`assets_legacy` / `virtual`):** pre-1.7 "legacy" asset layout maps objects
  to real filenames under `assets/virtual/legacy/`. Floor is `1.7.10` (`versions.rs:16`), so
  modern `objects/` layout covers the supported range — flag carried but virtual-mapping is
  out of scope unless a supported version needs it.

## Change log

<!-- Populated on first amendment after approval. -->

## Implementation log

### shipped — 2026-06-06

Built across 4 checkpoints (+ 1 polish pass) of /subagent-implementation on branch `vanilla-resolver`. Commits (chronological):

- `b75ae84` — CP-1 parse piston-meta version manifest (typed structs, cache by version id, 7 tests)
- `6327dc4` — CP-2 library rule eval + classpath/natives (OS-parametrized, Maven dest paths, 15 tests)
- `a6a4106` — CP-3 asset index resolution (object→DownloadItem, index-file item, assets_legacy, 8 tests)
- `2688a69` — CP-4 assemble DownloadPlan + LaunchMeta + `resolve_vanilla` command + ipc.ts mirror (2 e2e tests)
- `3f282f8` — polish: F-1 asset-hash panic guard + F-2 redundant-binding cleanup (1 test)

Final: 64 Rust tests pass (31 download baseline + 33 resolver); `npm run build` clean.

**Out-of-scope work performed during this build:**
- CP-1 review flagged the artifact struct missing its Maven `path` field; folded into CP-2 (its consumer) rather than a separate fix.

**Unforeseens — surprises that emerged during implementation:**
- WSL-native `cargo`/`cargo test` fails here (Tauri Linux target needs GTK/WebKit libs, `libsoup-3.0`, absent). All Rust build/test ran via the **Windows** cargo toolchain over the WSL UNC path, sharing the main-tree `target/` dep cache (`CARGO_TARGET_DIR`). The CLAUDE.md `source $HOME/.cargo/env && cargo` instruction is wrong for this machine. (Recorded in project memory `windows-build-toolchain`.)

**Deferred items still open:**
- None. Both ledger findings (F-1 hash guard, F-2 binding) fixed in `3f282f8`.

**Merged into main as 0926f4f — 2026-06-06.**
