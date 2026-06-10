---
id: neoforge-forge-launch-f-11
title: install://log event not filtered per instance
created: "2026-06-10"
origin: |
    docs/spec/neoforge-forge-launch.md, iter 6 reviewer (CP-4)
kind: finding
severity: risk
review_by: "2026-08-09"
status: open
file: src/routes/InstanceDetail.tsx:69
---

All mounted InstanceDetail views receive all install://log lines (no instanceId filter). Same limitation as existing launch listeners; backend serializes installs so no practical collision today. Revisit if concurrent installs or multi-window land.
