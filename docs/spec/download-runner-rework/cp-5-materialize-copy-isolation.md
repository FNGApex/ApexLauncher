# CP-5 — Storage isolation (hardlink → copy)

**Parent:** `../download-runner-rework/README.md` · **Design:** `../../design/download-runner-rework.md`
**Agent:** atomic-surgeon · **Depends on:** — (independent; first wave)

## Goal

Make each instance hold independent **byte copies** of its materialized libraries + version jars instead of hardlinks, so editing/deleting one instance never affects another or the cache.

## Context the implementer must honor

- `materialize` (`src-tauri/src/core/materialize.rs:133-141`) currently passes `std::fs::hard_link` as the default `link_fn`, with a byte-copy fallback only on cross-device `EXDEV`.
- Change: the default becomes a real **`fs::copy`**. The injectable `link_fn` seam stays (tests still inject). The EXDEV note is now moot but harmless — leave it.
- **Assets stay shared** (confirmed) — scope is libs + version jars only. Do NOT touch the assets path. Mods/configs/settings are already per-instance.
- Idempotent skip (`dst.exists()` → skip) stays correct for copies.

## Success criteria

- [ ] Default materialize produces independent byte copies (no shared inode).
- [ ] Cross-instance independence test: materialize into **two** instance dirs from one cache, then modify/remove instance-A's dest and assert the cache source **and** instance-B's copy stay byte-identical (not just src≠dst).
- [ ] Existing 8 `materialize_tests.rs` updated to the copy semantics + pass.

## Files

- `src-tauri/src/core/materialize.rs`
- `src-tauri/src/core/materialize_tests.rs`

## Verifies

`scripts/build.sh test materialize` — two-instance cross-independence assertion + existing tests green.

## Out of scope

Assets isolation (assets stay shared); the download/task/runner systems (unrelated).
