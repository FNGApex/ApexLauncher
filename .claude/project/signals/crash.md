# crash

## Overview

Local-only crash-report parsing + rule-based analysis for abnormal instance exits. No
network, no `regex` dependency, no new polling/watch loop — detection is exit-event-driven,
runs once (with a single bounded retry) after a launched instance exits abnormally (natural
exit, not killed, exit code ≠ 0 or unknown). Pipeline: `parse_crash_report` (text →
`ParsedReport`) → `analyze` (ordered 11-rule table → `CrashAnalysis`) → `resolve_suspects`
(cross-reference against the instance's mod manifest → ranked `CrashSuspect` list). Result
is session-only: retained on `RunState.crash` for the run's lifetime; a relaunch installs a
fresh `RunState` and clears it (app restart also loses it — crash-report files remain on
disk regardless).

## CLI code

- `src-tauri/src/core/crash.rs` — pure, no Tauri types: `parse_crash_report(&str) ->
  ParsedReport` (description/time/exception+frames capped at 15/`Minecraft Version`/
  `Fabric Mods:` + `Mod List:` tables/suspect mod-ids via `TRANSFORMER/<id>@` +
  `{mixin from mod <id>}`/suspect jars via `~[jar]` with vanilla filtered/suspect-package
  fallback, always populated, ranked last); `analyze(AnalyzeInput) -> CrashAnalysis` over the
  ordered `RULES: &[RuleFn]` table (first match wins; `rule_generic` always matches so
  `analyze` never panics) — 11 rule ids (stable API strings): `fabric_unmet_deps`,
  `forge_missing_deps`, `duplicate_mods`, `out_of_memory`, `unsupported_java`,
  `mixin_failure`, `missing_class`, `native_crash` (exit-code/`jvm_error_path`-gated, not a
  needle rule), `gl_error`, `mod_crash` (fallback when a report has suspect ids/jars),
  `generic`; `resolve_suspects(&ParsedReport, &[ModEntry], &[FolderMod]) -> Vec<CrashSuspect>`
  (priority: mod ids > jars > package fallback; dedup; capped at `MAX_SUSPECTS = 3`;
  `EXCLUDED_SUSPECT_IDS` = `minecraft`/`fabricloader`/`forge`/`neoforge` + any `fabric-`
  prefix). 62 tests in `crash_tests.rs`.
- `src-tauri/src/core/launch.rs` — extended for detection wiring: `RunState.started_wall:
  SystemTime` (set in `new_preparing`; crash detection filters `crash-reports/*.txt` /
  `hs_err_pid*.log` by `mtime >= started_wall - 2s`, not `started`); `RunState.crash:
  Option<crash::CrashAnalysis>`; `LaunchSink::crashed(&self, instance_id, &CrashAnalysis)`
  (default no-op); `detect_crash` async fn — runs in `monitor_child` **after all exit
  bookkeeping** (playtime recording, terminal `sink.status(...)` emit): skips when killed or
  exit code `Some(0)`; scans `mc/crash-reports/` non-recursive, single 750ms retry if no
  fresh report found; scans `mc/hs_err_pid*.log` → `jvm_error_path`; report capped at 8 MiB
  (oversized → log-only analysis); reads the last ≤300 log-ring lines (clone under lock, lock
  dropped before `analyze`); loads the mod manifest via a direct `instance.json` read (public
  `Instance`/`ModEntry` DTOs, best-effort — `instances::load_manifest` needs an `AppHandle`
  unavailable in Tauri-free `monitor_child`); composes parse → analyze → resolve_suspects;
  stores `state.crash` (lock released before emit) then calls `sink.crashed(...)`. 8 new
  `cp4_*` tests bring `launch_tests.rs` to 54.
- `src-tauri/src/lib.rs` — `CrashSuspectPayload`/`CrashAnalysisPayload` DTOs (`From<&crash::
  Crash*>` mappings, camelCase serde); `CrashAnalyzedPayload { slug, analysis }` on the
  `crash://analyzed` typed event (`#[tauri_specta(event_name = "crash://analyzed")]`);
  `TauriLaunchSink::crashed` emits it; `#[tauri::command] get_crash_analysis(slug) ->
  Option<CrashAnalysisPayload>` — synchronous read of the retained `RunState.crash` via
  `RunningRegistry`, **not task-queued**; registered in `collect_commands!`/event list.
  `lib_tests.rs` gains payload-mapping + unknown-slug tests (57 → 59).
- `src-tauri/src/core/fixtures/crash/` — `report_fabric_classcast.txt`,
  `report_neoforge_annotated.txt`, `report_vanilla_simple.txt`, `report_oom.txt`,
  `log_fabric_unmet_deps.txt`, `log_forge_missing_deps.txt`, `log_duplicate_mods.txt`,
  `log_mixin_fail.txt`, `log_glfw.txt`.

## Artifacts

- `src/lib/store.ts` — `crashes: Map<string, CrashAnalysisPayload>` slice on `useAppStore`;
  `setCrash(slug, analysis)`, `clearCrash(slug)`.
- `src/components/AppShell.tsx` — subscribes `events.crashAnalyzed` → `setCrash`; on
  `run://update` transition to `preparing`/`running` calls `clearCrash(slug)` (a fresh launch
  always installs a backend `RunState` with `crash: None`, so the frontend mirrors that).
- `src/routes/InstanceDetail.tsx` — `CrashPanel` rendered above the log console when
  `crashes.get(slug)` is set: headline, suggestion, suspect chips, collapsible
  exception+detail, "Open crash report" (`revealItemInDir`), "Java settings" link (only for
  `kind ∈ {out_of_memory, unsupported_java}`, links to the Tech tab where JavaTab renders).
  One-shot mount backfill via `getCrashAnalysis(slug)` when the store is empty and the run is
  terminal (module-level `crashBackfilledSlugs: Set<string>` gates it, mirroring the
  `enrichedSlugs`/`refreshedSlugs` pattern).
- `src/components/Toasts.tsx` — `ToastKind` gains `"crash"` (amber, same visual treatment as
  `"partial"`); `shownCrashRef: Set<string>` dedups per-slug, pruned when the slug leaves
  `crashes` (so a later crash on the same instance toasts again).
- `src/lib/ipc.ts` — `getCrashAnalysis(slug): Promise<CrashAnalysisPayload | null>` wrapper.

## Docs

- `docs/spec/crash-log-help.md` — full spec: checkpoint table (CP-1..CP-6, all implemented),
  data shapes, rule table with needles/headline/suggestion/detail-extractor per rule,
  attribution contract, detection contract (the exact `monitor_child` sequencing), fixture
  shapes, resolved design decisions, change log.
- `docs/design/crash-log-help.md` — design doc companion.

## Coupling

- `launch` domain — detection lives inside `launch.rs`'s `monitor_child`; `crash` has zero
  reverse dependency (no Tauri types, no `launch` imports). `RunState.crash` and
  `LaunchSink::crashed` are the only two integration points.
- `instances` domain — `resolve_suspects` takes `&[ModEntry]` and `&[FolderMod]` (imported
  from `core::instances`); `FolderMod` is accepted per the locked signature but not consulted
  by any current rule (every suspect that needs a display name resolves from
  `report.mods`/`ModEntry.name`). Manifest read in `detect_crash` bypasses `instances::
  load_manifest` (needs an `AppHandle`) in favor of a direct `instance.json` parse.
- `frontend-shell` — `AppShell` is the sole event-driven subscriber (same pattern as every
  other domain's events); `Toasts`/`InstanceDetail` read from the store, never subscribe
  directly.

## Conventions

- **No network, no `regex` crate, no new watcher/poller** — this is the locked v1 boundary
  (spec §Resolved decisions). Deferred: mclo.gs upload, crash-history browser, cross-restart
  persistence, deobfuscation, `hs_err` content parsing, auto-fix actions, prep-phase (`Failed`
  status) analysis, localization.
- Rule ids in `CrashAnalysis.kind` are a stable API contract — never rename, only append.
- Detection is single-shot: one 750ms retry, no loop; capped at 8 MiB report size; capped at
  ≤300 log-ring lines, ≤15 exception frames, ≤12 detail lines, ≤3 suspects, ≤2 fallback
  packages.
- `EXCLUDED_PACKAGE_PREFIXES` (frame-package fallback) and `EXCLUDED_SUSPECT_IDS` (mod-id
  attribution) are separate exclusion lists serving different stages — don't conflate them.
- Needle matching order is always: exception line → raw report text → each log-tail line
  (case-sensitive substring, first match wins across the ordered `RULES` table).
