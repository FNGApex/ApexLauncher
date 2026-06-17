# CurseForge API key — baked default + Advanced settings override

## Goal

Ship the launcher with a working CurseForge API key baked in at build time (sourced from a gitignored file, never committed), so CF browse/install works out of the box. Keep a manual override field under a new **Advanced → API Keys → CurseForge API** settings section, blank by default.

## Context / rationale (so this spec stands alone)

- Repo `FNGApex/ApexLauncher` is **PUBLIC**. A plaintext key committed to git is scraped and auto-revoked within minutes. **Hard rule: the key never lands in git.**
- A key embedded in any distributed client binary is extractable (binary dump or traffic sniff) — that is unavoidable for client apps and is *accepted*. CF's key program exists for exactly this (launchers); the "don't redistribute" clause targets handing your key to *other devs' apps*, not shipping it in *your own* launcher. This is the Prism pattern (already referenced in `docs/ROADMAP.md` Phase 5).
- Today the key resolves `env var → Settings field → None` (`cf_api_key_from` in `src-tauri/src/core/providers.rs:29`, called at ~5 sites in `src-tauri/src/lib.rs`). This adds a third, lowest-priority **baked** tier.

## Locked decisions

- **Supply mechanism:** `build.rs` sources the key from the existing gitignored `src-tauri/.env` (the same file already holding `MODLOADER_CF_API_KEY` for the `curseforge_live` test) and bakes it into the binary via a compile-time env (`cargo:rustc-env`), read in code with `option_env!`. No new committed file. Clean source builds (no `.env`) compile fine and bake nothing.
- **Resolution priority (highest → lowest):**
  1. Runtime `std::env::var("MODLOADER_CF_API_KEY")` — dev/CI override (unchanged).
  2. `settings.curseforge_api_key` — user-entered in Advanced → API Keys (override).
  3. Baked `option_env!("MODLOADER_CF_API_KEY")` — shipped default (NEW, lowest).
  4. `None` — CF key-missing state (Modrinth still works).
- **Settings UI:** new **Advanced** tab → **API Keys** group → **CurseForge API** text field. Blank by default. A non-blank value persists to `settings.curseforge_api_key` and overrides the baked key. Existing settings behavior unchanged.
- **Keep** the manual override (do not remove the field) — covers source builds, rate-limit cases, and users' own keys.

## Non-goals

- Runtime proxy server / key-injection service (explicitly rejected — needs infra; overkill pre-alpha).
- Hiding/obfuscating the baked key in the binary (impossible for client apps; not attempted).
- Any change to CF request logic, provider trait, or the key-missing UX beyond what resolution requires.
- Committing the key, a sample key, or the real key in any test/fixture.

## Success criteria

- [ ] `build.rs` bakes `MODLOADER_CF_API_KEY` from gitignored `src-tauri/.env` into the binary at compile time; emits `cargo:rerun-if-changed` (and/or `rerun-if-env-changed`) so changing the key forces a rebuild (no stale baked value).
- [ ] Missing `.env` → build still succeeds, bakes nothing (`option_env!` → `None`).
- [ ] `cf_api_key_from` resolves env → settings → baked → `None`; pure + unit-tested for every precedence case (env wins over settings wins over baked; blank values skipped; all-absent → `None`).
- [ ] All CF command call sites in `lib.rs` pass the baked tier (`option_env!("MODLOADER_CF_API_KEY")`).
- [ ] `git diff` of this change contains no literal key; `.env` stays gitignored; no key in any source or test.
- [ ] Settings has an **Advanced → API Keys → CurseForge API** field, blank by default, persisting to `settings.curseforge_api_key`, overriding the baked key when non-blank.
- [ ] With a key available (baked, settings, or env), CF search works with no manual entry. With none, CF shows the existing key-missing state and Modrinth is unaffected.
- [ ] `cargo test` green (new `cf_api_key_from` precedence tests); `npm run build` green.

## Checkpoints

| # | Checkpoint | Files/areas | Agent | Est. files | Verifies |
|---|------------|-------------|-------|------------|----------|
| 1 | Baked key tier: build.rs sources `src-tauri/.env` → `cargo:rustc-env`; `cf_api_key_from` gains a baked arg (`option_env!`); update all CF call sites; precedence tests | `src-tauri/build.rs`, `src-tauri/src/core/providers.rs`, `src-tauri/src/lib.rs`, sibling `providers_tests.rs` | atomic-builder | ~4 | `cargo test` — precedence (env>settings>baked>none, blanks skipped); build succeeds with and without `.env`; no key in diff |
| 2 | Settings Advanced → API Keys → CurseForge API field (blank default, override, persists) | `src/routes/Settings.tsx`, `src/lib/ipc.ts` if needed | atomic-builder | ~2 | `npm run build` green; field renders under Advanced, saves, overrides baked key |

## Risks

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| `option_env!` bakes a stale key (Cargo caches compilation) | med | build.rs emits `rerun-if-changed=.env` (+ `rerun-if-env-changed`) so key changes retrigger compile |
| Implementer hardcodes the real/sample key in a test or fixture (public-repo leak) | med | tests use a fake key string only; assert resolution logic, never a real value; reviewer greps the diff for any key-shaped literal |
| `.env` parsing in build.rs is brittle (quotes, comments, CRLF) | low | parse only `MODLOADER_CF_API_KEY=`; trim; ignore blanks/comments; absent or unparseable → bake nothing |
| Settings field collides with existing key handling / double-trim | low | reuse existing `settings.curseforge_api_key` load/save; blank ⇒ `None`; resolution already trims |

## Change log

<!-- Populated on first amendment after approval. -->

## Implementation log

### shipped — 2026-06-16

Built across 3 iterations of /subagent-implementation on branch `curseforge-api-key` (worktree). Commits (chronological):

- `ac81514` — CP-1 baked key tier: `build.rs` bakes `MODLOADER_CF_API_KEY` from gitignored `.env` via `cargo:rustc-env` (+ `rerun-if-changed`/`rerun-if-env-changed`); `cf_api_key_from` gains a third baked arg (env > settings > baked > None, blanks skipped); 5 `lib.rs` call sites + `curseforge_live.rs` updated; 5 new precedence tests (fake keys only).
- `8f1de51` — CP-2 Settings: relocated the existing CurseForge API key field under an **Advanced → API Keys** grouping; reworded as an optional override (key ships by default); no duplicate field; `ipc.ts` unchanged.
- `440d267` — polish: `build.rs` strips one leading + one trailing quote independently (mismatched-quote fix, F-1); dropped invisible `space-y-1` on the single-row API Keys group (F-3).

**Out-of-scope work performed during this build:**
- `src-tauri/tests/curseforge_live.rs:58` call-site arity updated to the 3-arg `cf_api_key_from` (necessary to keep the test crate compiling).

**Unforeseens — surprises that emerged during implementation:**
- CP-2 was a relocate/regroup, not a net-new field — the CF key input already existed in the flat Settings list. Implemented Advanced/API-Keys as section headings (`h2`/`h3`); no tab framework exists in the app, so introducing one was out of scope.
- `build.rs` parse logic is not reachable by `cargo test` (build script, not lib-compiled); the F-1 fix was verified by orchestrator reasoning over the strip logic rather than a unit test.
- IDE diagnostics during CP-1 showed a stale mid-edit arity-mismatch snapshot; a fresh `cargo test --no-run` confirmed a clean compile.

**Deferred items still open:**
- F-2 (🔵 `curseforge_live.rs` no longer mirrors full 3-tier precedence — passes `None` for the baked slot) — **dropped** per user: the live test is `#[ignore]`d and the `.env` read still covers the key; cosmetic mirror only.
