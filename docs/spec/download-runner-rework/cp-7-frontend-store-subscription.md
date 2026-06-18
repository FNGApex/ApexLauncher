# CP-7 — Frontend store + app-level subscription + invoke audit

**Parent:** `../download-runner-rework/README.md` · **Design:** `../../design/download-runner-rework.md`
**Agent:** atomic-builder · **Depends on:** CP-2 (task events/commands), CP-6 (run events/commands)

## Goal

Introduce an app-level Zustand store fed by an `AppShell`-level event subscriber, hydrated from backend queries, so all lane tracking survives navigation. Remove every now-broken `await invoke(…)` of a now-async command.

## Context the implementer must honor

- **Zustand is installed but unused** — create `src/lib/store.ts` with a `tasks` slice + a `runs` slice.
- **Subscriber lives in `AppShell`** (`src/components/AppShell.tsx`) which never unmounts (route components mount under its `<Outlet/>`). It `listen()`s to `task://update`, `task://progress`, `run://update`, `launch://log`, `launch://exit`, `install://log` and updates the store.
- **Hydrate on mount** via `list_tasks` / `list_running` / `get_run_logs` so a fresh load or reload re-syncs.
- **`ipc.ts`**: add listen + query wrappers and mirror the new task/run payload structs (camelCase). Mutating-op wrappers now return task ids.
- **Audit-and-remove**: the command-contract change means several call sites that did `await invoke(...)` for a result are now broken — `NewInstanceModal` (import auto-nav), `Home.tsx` (`ModpackInstallResult` handling), mod-op call sites. Replace each with the task-id + store-driven flow. tsc will NOT catch a missed site (the wrapper still returns an awaitable id), so this must be checked deterministically.

## Success criteria

- [ ] Store has `tasks` + `runs` slices, updated by the `AppShell` subscriber.
- [ ] Store hydrates from `list_tasks`/`list_running`/`get_run_logs` on mount.
- [ ] **Deterministic check: grep/`sg` for direct `await` of the now-task command wrappers returns zero hits.**
- [ ] `scripts/build.sh check` passes (tsc clean).
- [ ] Manual: operation + run state survives navigation and re-hydrates on reload.

## Files

- `src/lib/store.ts` (new)
- `src/components/AppShell.tsx`
- `src/lib/ipc.ts`
- `src/components/NewInstanceModal.tsx`
- `src/routes/Home.tsx`

## Verifies

`scripts/build.sh check` + zero-residual-`await invoke` grep + manual navigation/reload check.

## Out of scope

The DM panel (CP-8) and running indicator / InstanceDetail resync / toast (CP-9) — this CP provides the store they read.
