---
id: modpack-import-c-toast-stacking
title: Per-card install toasts stack at same position
created: "2026-06-17"
origin: |
    docs/spec/modpack-import.md, slice C iter 3-4 reviewer
kind: finding
severity: nit
review_by: "2026-08-16"
status: open
file: src/routes/Browse.tsx:444
---

Each Browse ModpackCard renders its own fixed bottom-6 right-6 install-result toast from local state; two installs completing close together stack toasts at the same position. Home gates a single route-level toast. Fix: lift install-result toast state to the Browse route level (one at a time), mirroring Home's pattern. Deferred to slice D polish.
