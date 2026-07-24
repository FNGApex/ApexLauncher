# Signals steering
#
# Ground-truth hints for the signals inferrer. Read before writing signals.md;
# steering wins over detection on conflict.

## Style
- Be terse. Facts only — no marketing adjectives, no "robust/comprehensive/seamless",
  no narrating what a reader can see. One clause per fact.
- Don't restate git history or per-commit changelogs in signals; signals describe the
  current shape of the code, not how it got there.
- Skip filler ("this domain handles...", "responsible for..."); lead with the noun.

## Framework
- Frontend: React 19 + TypeScript + Vite 7 + Tailwind v4 + React Router + TanStack Query + Zustand
- Backend: Rust + Tauri 2; reqwest (rustls-tls, no OpenSSL)
- Crate name is `modloader` / `modloader_lib`; product is ApexLauncher

## Build
- Build/test: scripts/build.sh (check | test [filter] | build | bundle | dev) — never raw cargo/npm
- CI is the only exception (calls cargo/npm/tauri directly on native runners)

## Test layout
- Unit tests in sibling `<stem>_tests.rs`, wired via `#[cfg(test)] #[path=...] mod tests;`

## Ignore for domains
- target/, node_modules/, gen/, .worktrees/, scratch
