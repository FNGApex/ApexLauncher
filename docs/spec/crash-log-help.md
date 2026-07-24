# Spec: Crash-log parsing & help

Design: `docs/design/crash-log-help.md`. Status: proposed. Build/test ONLY via
`scripts/build.sh`. Rust tests = sibling `<stem>_tests.rs`; pure parse/analyze fns
unit-tested with fixtures under `src-tauri/src/core/fixtures/crash/`.

**Goal (current truth):** when a launched instance exits abnormally (natural exit, not
killed, exit code ≠ 0 — or code unknown), ApexLauncher scans `mc/crash-reports/` once
(one bounded retry), analyzes the newest report + the in-memory log-ring tail with a local
data-driven rule table, and surfaces cause + suggested fix + suspected mod(s) in
`InstanceDetail` plus an amber toast. **No network. No new polling/watch loops.** Analysis
lives in the retained `RunState` for the session; relaunch clears it.

**New module:** `src-tauri/src/core/crash.rs` (+ sibling `crash_tests.rs`, `mod` decl in
`core/mod.rs`). **New event:** `crash://analyzed`. **New command:** `get_crash_analysis`.
**Extended:** `core/launch.rs` (`RunState.started_wall` + `RunState.crash` +
`LaunchSink::crashed` + post-exit hook in `monitor_child`), `lib.rs` (`TauriLaunchSink`,
payloads, command registration), frontend store/AppShell/InstanceDetail/Toasts.

---

## Checkpoint table

| CP | Deliverable | Files touched | Fixtures / tests | Bindings regen? | Verify via `scripts/build.sh` |
|----|-------------|---------------|------------------|-----------------|-------------------------------|
| CP-1 | Crash-report parser: `parse_crash_report(&str) -> ParsedReport` | new `core/crash.rs` (+ `mod` decl in `core/mod.rs`); sibling `crash_tests.rs`; fixtures dir `core/fixtures/crash/` | fixtures `report_fabric_classcast.txt`, `report_neoforge_annotated.txt`, `report_vanilla_simple.txt`, `report_oom.txt` (shapes in §Fixture shapes); tests: description/time extracted; exception class+message+frames (frames capped at 15); `Minecraft Version`; `Fabric Mods:` table parsed (`modid → name+version`); `Mod List:` pipe table parsed (`jar/name/modid/version`); `TRANSFORMER/<id>@` + `{mixin from mod <id>}` suspect ids; `~[jar]` suspect jars with vanilla filtered (`?`, `client-*`, `server-*`, `minecraft-*`); package-prefix fallback suspect skips exclusion list (§Attribution); malformed/truncated input → no panic, fields `None`/empty | No | `test crash` |
| CP-2 | Rule engine: `analyze(AnalyzeInput) -> CrashAnalysis` over ordered `RULES` table | `core/crash.rs`; `crash_tests.rs` | log-tail fixtures `log_fabric_unmet_deps.txt`, `log_forge_missing_deps.txt`, `log_mixin_fail.txt`, `log_duplicate_mods.txt`, `log_glfw.txt`; tests: one per rule id in §Rule table (needle in exception vs log tail both match); detail extractor pulls verbatim unmet-dep lines (capped 12); priority order (unmet-deps beats missing_class when both present; OOM beats generic); native_crash fires on exit code −1073741819 and on `jvm_error_path` present with `None` code; fallback `generic` always produces headline+suggestion; rule ids stable strings | No | `test crash` |
| CP-3 | Suspect attribution: `resolve_suspects(&ParsedReport, &[ModEntry], &[FolderMod]) -> Vec<CrashSuspect>` | `core/crash.rs`; `crash_tests.rs` | tests: report mod-id → name+jar via report's own mod table; jar → `ModEntry.file_name` match (case-insensitive, `.disabled` tolerated) → `ModEntry.name`; unmatched jar → suspect with jar only; package-fallback suspect ranked last; dedup by mod id/jar; vanilla/loader ids (`minecraft`, `fabricloader`, `forge`, `neoforge`, `fabric-api` prefix `fabric-`) excluded | No | `test crash` |
| CP-4 | Detection wiring in launch core | `core/launch.rs` (`RunState.started_wall: SystemTime` set in `new_preparing`; `RunState.crash: Option<CrashAnalysis>`; `LaunchSink::crashed` default no-op; `find_new_crash_report` + `find_jvm_error_file` helpers; post-exit hook in `monitor_child` per §Detection contract); `launch_tests.rs`; `CapturingLaunchSink` gains `crashes` vec | tests (tempdir + fake child scripts, house pattern): exit 1 + fresh `crash-reports/x.txt` → sink `crashed` fired, `RunState.crash` set, `report_path` correct; exit 0 + report present → NOT fired; killed → NOT fired; exit 1 + no report → fired with log-tail-only analysis (after single retry); pre-existing old report (mtime < start − 2 s slack) ignored; `hs_err_pid123.log` in `mc/` → `jvm_error_path` set; report > 8 MiB → skipped, log-only analysis; relaunch (`mark_preparing`) yields fresh state with `crash == None` | No | `test core::launch` + `test crash` |
| CP-5 | IPC surface: event + command | `src-tauri/src/lib.rs` (`CrashAnalyzedPayload` + `CrashAnalysisPayload`/`CrashSuspectPayload` DTOs; `TauriLaunchSink::crashed` emits `crash://analyzed`; `#[tauri::command] get_crash_analysis(slug) -> Option<CrashAnalysisPayload>` reading retained `RunState`; register command + event in `collect_commands!`/events list); `lib_tests.rs` | tests: payload `From<crash::CrashAnalysis>` mapping (camelCase serde asserted via serde_json); `get_crash_analysis` on unknown slug → `None` (core-level fn test, no Tauri runtime) | **YES** — new event + command + DTOs | `check` + `test`; regen via dev run (wait `[bindings] exported`) |
| CP-6 | Frontend surfacing | `src/lib/store.ts` (`crashes: Map<string, CrashAnalysisPayload>`, `setCrash`, `clearCrash`); `src/components/AppShell.tsx` (subscribe `events.crashAnalyzed` → `setCrash`; on `run://update` `preparing`/`running` → `clearCrash(slug)`); `src/lib/ipc.ts` (`getCrashAnalysis` wrapper); `src/routes/InstanceDetail.tsx` (CrashPanel above log console: headline, suggestion, suspect chips, collapsible detail+exception, "Open crash report" via opener `revealItemInDir`, "Java settings" link when kind ∈ {`out_of_memory`,`unsupported_java`}; on mount `getCrashAnalysis(slug)` backfill when store empty and run terminal); `src/components/Toasts.tsx` (amber toast on new crash entry, `shownRef` dedup) | tsc only (no FE test infra); dev smoke: force a crash (e.g. bogus JVM arg via Java tab args-override → spawn fails is `Failed`, so instead: add a broken mod / use `-Xmx64m` OOM) and verify panel + toast + Open-report | No (consumes CP-5 bindings) | `check`; then dev smoke |

Each CP lands independently: CP-1..CP-3 are pure (`test crash`); CP-4 makes detection real
in core; CP-5 exposes it; CP-6 is the first user-visible milestone. TDD throughout —
fixture/failing test first.

---

## Data shapes (CP-1..CP-3; CP-5 mirrors with camelCase serde)

```rust
// core/crash.rs — NO Tauri types anywhere in this module.
pub struct ParsedReport {
    pub description: Option<String>,
    pub time: Option<String>,
    pub exception: Option<ExceptionInfo>,
    pub minecraft_version: Option<String>,
    pub suspect_mod_ids: Vec<String>,   // from TRANSFORMER/<id>@ and {mixin from mod <id>}
    pub suspect_jars: Vec<String>,      // from ~[<jar>…], vanilla filtered
    pub suspect_packages: Vec<String>,  // fallback: first non-excluded frame packages
    pub mods: Vec<ReportMod>,           // union of "Fabric Mods:" + "Mod List:" tables
}
pub struct ExceptionInfo { pub class: String, pub message: Option<String>, pub frames: Vec<String> }
pub struct ReportMod { pub id: String, pub name: String, pub version: String, pub jar: Option<String> }

pub struct AnalyzeInput<'a> {
    pub report: Option<&'a ParsedReport>,
    pub report_text: Option<&'a str>,   // raw, for needle matching + detail extraction
    pub log_tail: &'a [String],         // last ≤300 ring lines, stream-agnostic
    pub exit_code: Option<i32>,
    pub report_path: Option<String>,
    pub jvm_error_path: Option<String>, // hs_err_pid*.log if present
}

#[derive(Clone)]
pub struct CrashSuspect { pub display: String, pub mod_id: Option<String>, pub jar: Option<String> }

#[derive(Clone)]
pub struct CrashAnalysis {
    pub kind: String,               // stable rule id (§Rule table)
    pub headline: String,           // human cause, one sentence
    pub suggestion: String,         // actionable fix, one-two sentences
    pub exception: Option<String>,  // "class: message" single line
    pub suspects: Vec<CrashSuspect>,
    pub detail: Vec<String>,        // verbatim key lines (≤12), e.g. unmet-dep listing
    pub report_path: Option<String>,
    pub jvm_error_path: Option<String>,
}
```

`get_crash_analysis(slug: String) -> Option<CrashAnalysisPayload>` — synchronous read of
the retained `RunState.crash`; not task-queued. `crash://analyzed` payload:
`{ slug: String, analysis: CrashAnalysisPayload }`, camelCase.

---

## Rule table (CP-2 contract — ids are stable API)

Ordered; first match wins. Needles are case-sensitive substrings matched against the
exception line, then the raw report text, then each log-tail line. No regex crate.

| # | id | needles (any-of) | headline / suggestion gist | detail extractor |
|---|----|------------------|----------------------------|------------------|
| 1 | `fabric_unmet_deps` | `Mod resolution encountered an incompatible mod set`, `Incompatible mods found`, `Unmet dependency listing` | Missing/incompatible mod dependencies / install-update the listed mods | verbatim lines starting `- Mod '`, containing `requires version`, or `which is missing` (≤12) |
| 2 | `forge_missing_deps` | `Missing or unsupported mandatory dependencies` | same gist, Forge/NeoForge wording | verbatim block after the needle line (≤12) |
| 3 | `duplicate_mods` | `DuplicateModsFoundException`, `Found duplicate mods`, `duplicate mods` | Same mod present twice / remove one jar | lines naming the duplicates |
| 4 | `out_of_memory` | `java.lang.OutOfMemoryError` | Ran out of memory / raise memory in Java settings | OOM subtype line (heap space / GC overhead / Metaspace) |
| 5 | `unsupported_java` | `UnsupportedClassVersionError`, `has been compiled by a more recent version of the Java Runtime` | Wrong Java version / check Java settings | class-version line |
| 6 | `mixin_failure` | `MixinApplyError`, `InjectionError`, `Mixin apply failed`, `mixin from mod` (in exception frames only) | Mod's mixin failed to apply — likely version incompatibility / update or remove suspect | mixin config + mod id line |
| 7 | `missing_class` | `ClassNotFoundException`, `NoClassDefFoundError` | A mod references a class that isn't present (missing dependency or version mismatch) | the missing class name |
| 8 | `native_crash` | exit code ∈ {−1073741819, −1073740791, 134, 139} OR (`jvm_error_path` set) — flag-based, not needle | JVM/native crash / update graphics drivers + Java; links hs_err file | none |
| 9 | `gl_error` | `GLFW error`, `Failed to create GLFW window`, `does not appear to support OpenGL`, `org.lwjgl.` (exception only) | Graphics/driver problem / update GPU drivers | GLFW error line |
| 10 | `mod_crash` | (fallback when a report exists AND suspects non-empty) | Crash implicates <suspect> / try updating or disabling it | none |
| 11 | `generic` | (always matches) | Game crashed (exit <code>) / open the crash report; check the log | none |

Suggestion strings live beside the table as plain `&'static str` templates with `{}` slots
filled by a small format helper — data-driven, one row + one fixture test per new rule.

## Attribution (CP-1/CP-3 contract)

Frame package exclusion list (prefixes, not suspects): `java.`, `javax.`, `jdk.`, `sun.`,
`com.sun.`, `net.minecraft.`, `com.mojang.`, `net.fabricmc.`, `org.quiltmc.`,
`net.minecraftforge.`, `net.neoforged.`, `cpw.mods.`, `org.spongepowered.`, `org.lwjgl.`,
`io.netty.`, `com.google.`, `org.apache.`, `org.slf4j.`, `it.unimi.`, `org.joml.`.
Suspect priority: mod ids (TRANSFORMER/mixin annotations) > jars (`~[…]`, content taken up
to first `%`, `!`, or `:` — `:` is required for the common `~[jarname.jar:?]` form) >
package fallback (first 2 non-excluded packages, always populated by CP-1; ranked last by
CP-3, not gated on ids/jars being empty). Resolution order
in CP-3: id → report mod table → manifest; jar → manifest `ModEntry.file_name`
(case-insensitive, `.jar`/`.jar.disabled`); dedup; ≤3 suspects surfaced.

## Detection contract (CP-4 — the builder follows verbatim)

In `monitor_child`, after the terminal transition + `sink.status(...)` emit:
1. Skip when `was_killed` OR `exit_status == Some(0)`.
2. `game_dir = inst_dir.join("mc")`. Scan `game_dir/crash-reports/*.txt` (non-recursive)
   for files with mtime ≥ `started_wall − 2 s`; pick the newest. If none: `tokio::time::
   sleep(750 ms)` once, rescan once. (Single retry — no loop, no watcher.)
3. Scan `game_dir/hs_err_pid*.log` (non-recursive, same mtime filter) → `jvm_error_path`.
4. Read report (skip if > 8 MiB → report `None`), `parse_crash_report`, take last ≤300
   ring lines (clone under lock, drop lock), `analyze`, `resolve_suspects` against the
   manifest loaded via `instances::load_manifest` (best-effort; on error → empty mods).
5. Store `state.crash = Some(analysis.clone())` (lock released before emit), then
   `sink.crashed(&slug, &analysis)`.
All steps best-effort: any I/O error degrades to log-only analysis, never panics, never
blocks exit bookkeeping (runs after playtime recording).

## Fixture shapes (CP-1/CP-2 — synthesized from verified real reports, trimmed)

- `report_fabric_classcast.txt` — Fabric 1.20.1 layout: header + witty comment + Time +
  Description + `ClassCastException` with intermediary frames (`net.minecraft.class_…`) +
  `-- Head --`/`-- System Details --` + `Fabric Mods:` (`\tmodid: Name version` lines).
- `report_neoforge_annotated.txt` — NeoForge 1.21 layout: frames with
  `TRANSFORMER/<id>@<ver>/pkg.Class.m(F.java:N) ~[modjar.jar:?] {}`, one
  `{mixin from mod <id>: <cfg>}` frame, `Mod List:` pipe-column table.
- `report_vanilla_simple.txt` — no loader sections, obfuscated-ish frames, plain NPE.
- `report_oom.txt` — `java.lang.OutOfMemoryError: Java heap space` as the exception.
- `log_*.txt` — plain log-line sequences carrying each rule's needle text verbatim (Fabric
  wording per FabricMC discussions #3350: `Unmet dependency listing:` +
  `- Mod 'X' (x) 1.0 requires version 2.0 or later of mod 'Y' (y), which is missing!`).

---

## Resolved decisions (locked at design time — 2026-07-23)

- **Detection trigger** ✅ exit-event-driven only (hook in `monitor_child`); one-shot scan +
  single 750 ms retry; no watcher, no polling. Killed and exit-0 runs are never analyzed.
- **Log source** ✅ in-memory log ring tail (≤300 lines), NOT `logs/latest.log`.
- **Network** ✅ zero new calls; no mclo.gs, no mapping downloads, no telemetry.
- **Rules** ✅ ordered static table, substring needles, no `regex` dependency.
- **Persistence** ✅ session-only (retained `RunState`); relaunch clears (fresh
  `RunState::new_preparing`); app restart loses analysis (files remain on disk) — v1.
- **UX** ✅ CrashPanel in `InstanceDetail` + amber toast; last-crash-only; no history UI.
- **Deferred** ✅ mclo.gs upload/share · crash-history browser · cross-restart persistence ·
  deobfuscation · hs_err content parsing · auto-fix actions (disable-mod button, auto-RAM) ·
  prep-`Failed` analysis · localization.

---

## Change log

- 2026-07-23 — CP-1 implemented (`core/crash.rs`, 23 tests, 4 fixtures). Jar extraction
  stop-chars amended `%`/`!` → `%`/`!`/`:` (real `~[jar:?]` frame syntax requires it);
  `suspect_packages` documented as always-populated. Exception-header scan is
  first-plausible-line-only by contract (graceful `None` → log-tail analysis).
- 2026-07-23 — Initial spec drafted (ax-plan). Pure `core/crash.rs` (parse → analyze →
  attribute) + exit-hook wiring in `monitor_child` + `crash://analyzed` event +
  `get_crash_analysis` command + CrashPanel/toast. CP-1..CP-6. No checkpoints implemented.
