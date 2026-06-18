# CP-9 — Running indicator + launch warning + InstanceDetail resync + toast

**Parent:** `../download-runner-rework/README.md` · **Design:** `../../design/download-runner-rework.md`
**Agent:** atomic-builder · **Depends on:** CP-7 (store)

## Goal

Surface the Runner in the UI and make `InstanceDetail` continuity-correct; add the done-toast.

## Context the implementer must honor

- **Running indicator**: a running-count indicator in the always-mounted shell (Sidebar or a new component), reading the `runs` slice.
- **Launch warning**: before launching a **2nd+** concurrent pack, show a confirm dialog the user must acknowledge (count comes from the `runs` slice).
- **InstanceDetail resync**: `src/routes/InstanceDetail.tsx` currently tracks `running`/`logLines` in component `useState` and subscribes to `launch://log`/`launch://exit` in a `useEffect` torn down on unmount (`InstanceDetail.tsx:88-138`). Replace with **reads from the store** — on mount, resync running state + replay buffered logs (incl. an exit that occurred while away). Never blank out a running instance. The app-level subscriber (CP-7) owns the listeners now.
- **Done-toast**: a store-driven toast with an **"Open" action** on terminal task results (e.g. a finished import → Open the new instance). No auto-navigation.

## Success criteria

- [ ] Shell shows a live running-pack count.
- [ ] Launching while ≥1 pack runs prompts a confirm the user must accept.
- [ ] Navigating away from and back to a running instance shows live status + replayed logs (incl. a missed exit) — never blank.
- [ ] A finished import raises a toast with a working "Open" action; no auto-navigation.
- [ ] `scripts/build.sh check` passes.

## Files

- `src/routes/InstanceDetail.tsx`
- `src/components/Sidebar.tsx` (or new indicator component)
- toast surface (new or existing)

## Verifies

`scripts/build.sh check` + manual: resync-on-return, 2nd-launch warning, Open-toast.

## Out of scope

The DM panel (CP-8); store + backend (CP-2/6/7).
