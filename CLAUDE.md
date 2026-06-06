# CLAUDE.md

> **Read this file in full at the start of every session, before doing anything else.**

This is the long-term operating manual for the **Modloader** project. It, together with the
per-folder `SKILLS.md` files, is the project's long-term memory. `HANDOFF.md` is the
short-lived, session-to-session memory.

---

## 🧠 Memory & handoff system — READ FIRST

Three layers. Respect all three every session:

| File | Scope | Lifetime | Holds |
|------|-------|----------|-------|
| `CLAUDE.md` (root) | whole project | long-term | rules, overview, build/run, structure |
| `SKILLS.md` (one per source folder) | that folder | long-term | what's here, conventions, how it works |
| `HANDOFF.md` (root) | current work | **reset every session** | quick work summary, next plans, successful approaches only |

### Session protocol

**At session start (always):**
1. Read this `CLAUDE.md` in full.
2. Read `HANDOFF.md` to see where the previous session left off.
3. Before touching any folder, read that folder's `SKILLS.md`.

**While working:**
- Follow the conventions documented in each folder's `SKILLS.md`.
- Prefer extending the documented, *successful* patterns over inventing new ones.

**On finish — i.e. when the user signals the coding session is over:**
1. **Update `CLAUDE.md`** if any project-wide fact, rule, structure, or command changed.
2. **Update the `SKILLS.md`** of every folder you changed, so the folder-local knowledge
   stays current (new files, new conventions, new gotchas).
3. **Rewrite `HANDOFF.md` from scratch.** Always start from a *fresh* handoff — overwrite
   it completely, never append to the old one. It must contain ONLY:
   - a quick summary of the work done this session,
   - the next plans,
   - **only successful approaches** — never failed attempts, dead ends, or things that
     didn't work. (Long-term lessons belong in `CLAUDE.md`/`SKILLS.md`, not here.)

> Rule of thumb: `CLAUDE.md` + `SKILLS.md` = durable knowledge. `HANDOFF.md` = a clean
> baton pass for the next session. If something failed, it does not go in the handoff.

---

## Project overview

Modloader is a lightweight, cross-platform **Minecraft** mod launcher (PrismLauncher-like)
that imports modpacks from **both CurseForge and Modrinth**.

- **Frontend:** React 19 + TypeScript + Vite 7 + Tailwind v4 + React Router + TanStack Query (+ Zustand for UI state)
- **Backend:** Rust via **Tauri 2** (downloads, instance mgmt, Java mgmt, launch, auth)
- **Targets:** Windows, macOS, Linux

Authoritative design lives in `docs/`:
- `docs/ARCHITECTURE.md` — subsystems, on-disk layout, data model, IPC
- `docs/ROADMAP.md` — 8 phases (Phase 0 done); each ends runnable
- `docs/PROVIDERS.md` — CurseForge & Modrinth API specifics and gotchas

**Key external constraints** (don't relearn these the hard way):
- CurseForge API needs a free `x-api-key` header; Modrinth needs no key.
- Some CF mods set `allowModDistribution:false` → cannot be auto-downloaded; the UI must
  open the project page for a manual drop-in. This shapes the pack resolver.

---

## Build & run

Rust is installed via rustup but **not on the default shell PATH**, so source it first:

```bash
# Frontend only
npm install
npm run build          # tsc + vite build (typecheck + bundle)

# Full app (needs cargo on PATH)
. "$HOME/.cargo/env" && npm run tauri dev     # dev window, HMR
. "$HOME/.cargo/env" && cargo check           # (run inside src-tauri/) Rust typecheck
```

Running rustup's profile line once (or restarting the terminal) puts cargo on PATH
permanently and removes the need for the `. "$HOME/.cargo/env"` prefix.

---

## Structure & where the SKILLS.md files live

```
modloader/
├── CLAUDE.md            ← you are here (root, long-term rules)
├── HANDOFF.md           ← fresh each session (work summary / next / successes)
├── README.md
├── docs/                SKILLS.md → design docs index
├── src/                 SKILLS.md → frontend overview & conventions
│   ├── lib/             SKILLS.md → IPC wrappers + utils
│   ├── components/      SKILLS.md → shared UI
│   └── routes/          SKILLS.md → page components per route
└── src-tauri/           SKILLS.md → Rust backend overview
    └── src/             SKILLS.md → Rust entry & how to add a command
```

When you create a new source folder, add a `SKILLS.md` to it and note it here.
