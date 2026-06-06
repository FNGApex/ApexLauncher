# SKILLS — docs/

Authoritative design documentation. This is the source of truth for *how the system is
meant to work*; keep it in sync when design decisions change.

## Files
- `ARCHITECTURE.md` — system shape (React ↔ Tauri IPC ↔ Rust), on-disk layout, the
  `instance.json` data model, the `ModProvider` trait, download engine, launch sequence,
  Microsoft auth flow, frontend structure, testing strategy.
- `ROADMAP.md` — the 8-phase build plan (Phase 0 done). Each phase ends with something
  runnable. Update phase status here as work completes.
- `PROVIDERS.md` — CurseForge & Modrinth API details and gotchas (CF API key, CF
  `allowModDistribution:false` manual-download case, hash algorithms, `.mrpack` vs CF-zip
  modpack formats, normalized domain types, resolution flow).

## Conventions
- Design changes land here **first**, then in code.
- Keep diagrams ASCII so they render anywhere.
- When a phase completes, tick it in `ROADMAP.md` and reflect any new durable facts in the
  root `CLAUDE.md`.
