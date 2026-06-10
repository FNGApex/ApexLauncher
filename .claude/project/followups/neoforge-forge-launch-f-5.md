---
id: neoforge-forge-launch-f-5
title: Type-safe artifact-vs-base URL contract for loader libraries
created: "2026-06-10"
origin: |
    docs/spec/neoforge-forge-launch.md, iter 3 reviewer (CP-2)
kind: finding
severity: risk
review_by: "2026-08-09"
status: open
file: src-tauri/src/core/resolver.rs:758
---

Correct for all current formats (fabric/quilt base URLs never end .jar) but fragile if a future loader ships a .jar-suffixed base URL. Type-safe alternative: enum variant or companion flag on LoaderLibrary.url distinguishing BaseUrl vs ArtifactUrl. Revisit when a new loader format lands.
