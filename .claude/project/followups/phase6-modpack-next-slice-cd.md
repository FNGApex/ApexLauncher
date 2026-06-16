---
id: phase6-modpack-next-slice-cd
title: 'Phase 6 next: modpack slice C (browse install) or D (pack update)'
created: "2026-06-16"
origin: |
    session 2026-06-15 — slices A+B shipped+pushed
kind: plan
review_by: "2026-08-15"
status: open
file: docs/spec/modpack-import.md
---

Phase 6 modpack import: slices A (`.mrpack`) and B (CurseForge `.zip`) are shipped and pushed to origin/main (`505670b`..`241294f`). Backend + UI complete, full lib 436 tests green, npm build green.

**Pick up next (choose one):**
- **Slice C** — Browse & one-click pack install from both providers (CF `classId=4471`, Modrinth `project_type:modpack`). Backend headless-testable; UI needs GUI. Reuses the slice A/B pure-planner + executor seams.
- **Slice D** — Pack update / re-resolve installed pack against a newer index (diff installed vs new). Also where the batch CF resolution (`POST /v1/mods/files`) optimization and partial-import rollback belong.

Design + spec: `docs/design/modpack-import.md`, `docs/spec/modpack-import.md` (slices C–D not yet specced — run `/atomic-plan` for the chosen slice).

**Open before declaring Phase 6 done:**
- GUI end-to-end verify: import a real `.mrpack` AND a real CF `.zip`, launch each (not headless-testable in WSL — needs the Windows GUI run).
- Follow-ups: [[modpack-import-cf-overrides-dir]] (non-default overrides dir), [[modpack-import-cf-manual-slug-link]] (manual link uses numeric id), [[modpack-import-partial-cleanup]] (no rollback on mid-import failure).

**Build/test reminder:** Rust builds on the Windows cargo toolchain over WSL, NOT WSL-native — `cd /mnt/c && cmd.exe /c "C:\\Users\\drgor\\apex-build.bat" <args>`. Frontend `npm run build` runs in WSL.
