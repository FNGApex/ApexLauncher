# CP-8 — Download Manager panel UI

**Parent:** `../download-runner-rework/README.md` · **Design:** `../../design/download-runner-rework.md`
**Agent:** atomic-builder · **Depends on:** CP-7 (store)

## Goal

A shell-level panel that renders the Download Manager task queue from the store: parent label, current child, counts, status, cancel.

## Context the implementer must honor

- Read **entirely from the `tasks` slice** of the store (CP-7) — no component-local operation state.
- Panel placement: **Sidebar drawer** (Sidebar never unmounts) is the lean; header dropdown acceptable — implementer's call (open question in the index).
- Each task row shows: parent label (e.g. pack name), current child label (e.g. mod being downloaded), `done/total`, status; a **cancel control** driving the `cancel_task` command.
- Existing shared components (`SlideOver`, `ProviderBadge`) may be reused.

## Success criteria

- [ ] Panel lists active + recent tasks from the store with live parent + child + counts.
- [ ] Cancel control invokes `cancel_task` and the row reflects `Cancelled`.
- [ ] `scripts/build.sh check` passes.
- [ ] Manual: enqueuing a task shows it live in the panel with progressing child + counts.

## Files

- `src/components/DownloadManager.tsx` (new)
- `src/components/Sidebar.tsx` or `src/components/AppShell.tsx` (mount point)

## Verifies

`scripts/build.sh check` + manual: live progress + working cancel.

## Out of scope

Running indicator / InstanceDetail / toast (CP-9); store + events (CP-7).
