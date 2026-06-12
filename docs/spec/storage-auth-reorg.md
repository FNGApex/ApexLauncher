# Storage, branding, and auth reorganization (ApexLauncher)

Design: `docs/design/storage-auth-reorg.md`

## Goal

Rebrand to "ApexLauncher" with a friendly OS-native data root; reduce auth to a single
persistent Microsoft account driven from a sidebar login/logout control; make each instance a
self-contained game tree materialized via hardlinks from a shared `cache/`.

## Non-goals

- Migrating existing `com.bear.modloader` data (pre-alpha; fresh start).
- Multi-account, account switching, offline accounts.
- Renaming the `modloader` Rust crate, repo directory, or npm package.
- Changing download/resolver/loader-install logic beyond where artifacts land and how they
  are materialized into an instance.
- Hardlinking the content-addressed asset store per instance (assets stay shared via
  `--assetsDir`; see design Recommendation A — pending confirmation in Open questions).

## Success criteria

- [ ] Data root resolves to `<OS-appdata-base>/ApexLauncher/` on all three OSes, independent
      of the bundle identifier; verified by a unit test asserting `data_dir` ends in
      `ApexLauncher` and a launched app writing under that folder.
- [ ] `cache/` holds `assets/`, `libraries/`, `versions/`, `java/`, `meta/`, `installers/`;
      `instances/` holds per-instance trees. No launcher-written files land directly in the
      root except `account.json` and the two top-level dirs.
- [ ] Bundle identifier is `com.apex.apexlauncher`; `productName`, window title, and visible
      sidebar branding read "ApexLauncher"; no user-facing "modloader"/"Modloader" string
      remains. Verified by grep + app launch.
- [ ] Multi-account is gone: no `accounts.json`, no `list_accounts`/`remove_account`/
      `set_active_account` commands, no `/accounts` route or `Accounts.tsx`. `cargo build` and
      `tsc` succeed with zero references.
- [ ] A sidebar control bottom-left shows logged-out state with a Login action and, when
      authenticated, the account name with a Logout action. Login persists across app restart
      (refresh token in OS keyring, profile in `account.json`); Logout clears both.
- [ ] Launching an instance materializes `instances/<slug>/libraries/` and `versions/` via
      hardlink (copy fallback) from `cache/`, builds the classpath from the instance paths,
      and the JVM spawns with `cwd=mc/` and `--assetsDir` pointing at `cache/assets`.
- [ ] Re-launching an unchanged instance re-materializes idempotently (no error if links
      already present) and adds no measurable disk beyond inode entries.
- [ ] Full Rust suite green (currently ~294); new tests cover data-root naming, single-account
      store round-trip + keyring seam, and hardlink-with-copy-fallback materialization.

## Approaches

(Full table in design doc. Chosen: data root via `path().data_dir().join("ApexLauncher")`;
single-account = `account.json` + keyring; instance materialization = hardlink libs + loader/
version jars, assets shared.)

## Recommendation

Land as one sequenced spec, A→B→C, because all three rebase on `store.rs` path helpers and C
depends on A's `cache/` split. Each checkpoint ends green and is committed independently so the
reorg is incremental and revertible per slice.

## Checkpoints

### Slice A — rebrand + data root + cache layout

| # | Checkpoint | Files/areas | Agent | Est. files | Verifies |
|---|------------|-------------|-------|------------|----------|
| A1 | Data root → `path().data_dir().join("ApexLauncher")`; add `cache_dir()` + cache subdir helpers (`assets`/`libraries`/`versions`/`java`/`meta`/`installers`); repoint `java_dir`, meta, installer, accounts-file parent to new layout | `src-tauri/src/core/store.rs` | atomic-surgeon | 1 | Unit test: `data_dir` path ends in `ApexLauncher`; cache helpers return `<root>/cache/<sub>` |
| A2 | Repoint all path consumers to cache helpers (assets/libraries/versions/java/meta/installers) so nothing writes to the old shared root paths | `src-tauri/src/core/launch.rs`, `java.rs`, `meta.rs`, `forge_installer.rs`, `resolver.rs`, `lib.rs` | atomic-builder | ~6 | `cargo test` green; grep shows no `data_dir().join("assets"/"libraries"/...)` outside `store.rs` |
| A3 | Rebrand: `identifier` → `com.apex.apexlauncher`, `productName` + window title → `ApexLauncher`; sidebar brand text + version footer | `src-tauri/tauri.conf.json`, `src/components/Sidebar.tsx` | atomic-surgeon | 2 | grep: zero `modloader`/`Modloader` user-facing strings (excl. crate/package id); app launches titled "ApexLauncher" |

### Slice B — single-account auth

| # | Checkpoint | Files/areas | Agent | Est. files | Verifies |
|---|------------|-------------|-------|------------|----------|
| B1 | Replace multi-account store with single-account store: `account.json` (profile) + keyring (refresh token); add `get_account`/`logout`; drop list/remove/set-active store logic. Keep injectable HTTP + keyring seams | `src-tauri/src/core/auth.rs`, `src/core/store.rs` (`accounts_file`→`account_file`) | atomic-builder | ~2 | Rust tests: store round-trip writes/reads one account; logout clears `account.json` + keyring; mock-HTTP login path unchanged |
| B2 | Tauri commands: keep `begin_login`/`cancel_login`, add `get_account`/`logout`, remove `list_accounts`/`remove_account`/`set_active_account`; update `ipc.ts` mirrors | `src-tauri/src/lib.rs`, `src/lib/ipc.ts` | atomic-builder | ~2 | `cargo build` + `tsc` green; no reference to removed commands |
| B3 | UI: delete `Accounts.tsx` + `/accounts` route + nav item; add bottom-left login/logout control in `Sidebar.tsx` (logged-out → Login; authed → name + Logout), wired to commands + an auth query | `src/components/Sidebar.tsx`, `src/router.tsx`, `src/routes/Accounts.tsx` (delete), `src/lib/ipc.ts` | atomic-builder | ~4 | `tsc` + `npm run build` green; control reflects auth state and survives restart |

### Slice C — instance materialization

| # | Checkpoint | Files/areas | Agent | Est. files | Verifies |
|---|------------|-------------|-------|------------|----------|
| C1 | Materialization helper: `hard_link` with copy fallback; given a list of cache-relative artifacts, build `instances/<slug>/libraries/` + `versions/`; idempotent (skip if link present) | `src-tauri/src/core/instances.rs` or new `materialize.rs` | atomic-builder | ~2 | Unit tests: hardlink path created; cross-link fallback copies; second call is a no-op |
| C2 | Launch wiring: before spawn, materialize the resolved plan's libs + loader/version jars into the instance; build classpath from `instances/<slug>/libraries`; `--assetsDir` → `cache/assets`; loader install targets `cache/` then materializes | `src-tauri/src/core/launch.rs`, `src/lib.rs` (launch path), `forge_installer.rs` | atomic-builder | ~3 | Rust tests: classpath entries resolve under instance dir; assets arg points at cache; existing launch tests green |

## Risks

| Risk | Likelihood | Mitigation |
|------|-----------|-----------|
| Hardlink fails cross-volume (cache and instances on different FS) | med | Copy fallback in C1; covered by a test forcing the fallback path |
| Path-move in A2 misses a consumer → files split across old+new roots | med | grep gate in A2 success criterion; full suite re-run; manual launch smoke |
| Single-account refactor leaves dangling refs (commands/types/UI) | med | B2/B3 grep + `cargo build` + `tsc` gates; reviewer verifies zero references |
| Window 800×600 too small for rebranded UI | low | Out of scope; noted in design Open questions |
| Asset-sharing deviates from literal request | med | Surfaced in design Open questions; confirm before C2 |

## Change log

<!-- Populated on first amendment after approval. -->
