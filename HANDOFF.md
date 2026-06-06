# HANDOFF

> Fresh every session. Overwrite completely at the end of each coding session.
> Contains only: work summary · next plans · successful approaches. No failed attempts.
> Last updated: 2026-06-06

## Where things stand
**Phase 0 is complete and the project memory system is in place.** The app is a runnable
Tauri 2 + React 19 desktop launcher with a working nav shell and a live React↔Rust bridge.
Dev server was stopped at end of session (no processes left running).

## Work done this session
- Bootstrapped from an empty folder.
- Design docs written: `docs/ARCHITECTURE.md`, `docs/ROADMAP.md`, `docs/PROVIDERS.md`.
- Installed Rust (rustup 1.96) and scaffolded **Tauri 2 + React 19 + TS** via
  `create-tauri-app`; added Tailwind v4, React Router, TanStack Query, Zustand, lucide-react.
- **Phase 0:** native window, sidebar nav (Instances · Browse · Accounts · Settings),
  dark theme with themeable tokens, typed IPC (`app_info` → "Connected" pill on Home).
- **Memory system:** root `CLAUDE.md` (rules + overview), `HANDOFF.md`, and a `SKILLS.md`
  in every source folder (`docs`, `src`, `src/lib`, `src/components`, `src/routes`,
  `src-tauri`, `src-tauri/src`).

## Verified working
- `npm run build` (tsc + vite) — clean.
- `cargo check` in `src-tauri/` — clean.
- `npm run tauri dev` — window launches, IPC connected, hot-reload confirmed.

## Next plans (from docs/ROADMAP.md)
- **Phase 1 — instances & local management:** `instance.json` model, on-disk layout
  (shared content-addressed assets/libraries), create/list/delete instances, instance
  detail page reconciled from the folder.
- **Phase 2 (highest risk — worth an early spike):** vanilla Minecraft install + launch —
  Mojang piston-meta client, Java/Temurin manager, hash-verified concurrent download engine,
  launch a vanilla instance with a live log console.
- Still-open choice from this session: start Phase 1, spike Phase 2's launch path first, or
  `git init` before more code (project is **not** a git repo yet).

## Quick start for next session
```bash
. "$HOME/.cargo/env" && npm run tauri dev
```
Read `CLAUDE.md` first, then the `SKILLS.md` of whatever folder you touch.
