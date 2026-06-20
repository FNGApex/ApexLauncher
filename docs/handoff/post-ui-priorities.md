# Post-UI priorities (next phase after the UI overhaul)

Captured 2026-06-19 from the human. After the `ui-overhaul` branch lands the UI redesign, the next
two focus areas — in priority order:

## P1 · API-polling efficiency ("we're very API hungry") — ✅ ADDRESSED (2026-06-19, commit e8c82bd)

**Outcome:** the QueryClient globals were already well-tuned (`refetchOnWindowFocus:false`, 30s
staleTime — `src/lib/query.ts`); the hunger was N+1 fan-out, not focus churn. Two fixes landed:
- `ModSearchCard` (mod-add search) fetched `getModVersions` eagerly per result (~20 calls/page,
  ×infinite-scroll). Made lazy (enabled on hover/focus), mirroring Browse's ModpackCard. **The main fix.**
- Browse used `["mcVersions"]` vs the canonical `["mc-versions"]` → cache split → refetch. Unified.

Residual/optional (not done, low value): align the `["settings"]` staleTime across InstanceDetail/
JavaTab/TechTab/Settings (keys already match so TanStack dedupes; harmless). Re-open if hunger persists.

Original symptom + leads below (kept for the record).

Symptom (human): the app makes far more provider/API calls than it should. Likely contributes to P2
(rate-limit → download failures).

**Concrete leads (investigate first):**
- **`ModSearchCard` eager version fetch (strong suspect).** In the mod-add search list
  (`src/routes/instance-tabs/ModlistTab.tsx`, relocated from `InstanceDetail.tsx`), each result card
  runs `getModVersions(...)` with `enabled: true` — so a single search page of ~20 results fires ~20
  `get_mod_versions` API calls immediately, before the user clicks anything. Fix: lazy-fetch versions
  on demand (e.g. on Install click / hover / `enabled: false` until needed), or batch.
- **Audit other eager/`enabled: true` queries** and any polling loops. Confirm task/run progress is
  event-driven (`task://progress`, `run://update`) and not polled. Check `staleTime`/`gcTime` defaults —
  add caching where provider calls repeat (search re-fetches, version re-fetches).
- **New in WS-D:** `getPackInfo` on the Info tab is cached `staleTime: 5min` — fine, but confirm it
  doesn't refetch on every tab switch.
- Consider a provider-call rate limiter / dedupe layer in the Rust provider seam.

## P2 · Download Manager reliability ("failing quite often")

Symptom (human): downloads/tasks fail frequently. Need a real repro + the failure mode.

**Investigate:**
- Reproduce a failing download; capture the `task://update` terminal payload + backend logs. Is it a
  network error, a 429 rate-limit (ties to P1), a hash-verification failure, a `.part` resume/TOCTOU
  issue, or a task-job error?
- Relevant domains: `download` (`src-tauri/src/core/download.rs` — concurrent engine, `.part` resume,
  hash verify), `task-manager` (`src-tauri/src/core/task_manager.rs` — serial FIFO worker, job impls in
  `lib.rs`). Known pre-existing test flake: `cp4_concurrency_bound_not_exceeded`.
- Check whether failures correlate with the API hunger (rate-limited provider responses feeding bad/empty
  download plans). Fixing P1 may reduce P2.
- Surface clearer error messages in the `DownloadManager` panel (`src/components/DownloadManager.tsx`).

## Deferred during the UI push (revisit)
- Full UI smoke-test pass (Instance Detail tabs, Info description rendering, Tech readout) — deferred by
  the human mid-WS-D; do a batch smoke-test before/after this phase.
- Anything parked in `docs/STRETCH-GOALS.md` (Mac/Linux installers, signing, CI).
