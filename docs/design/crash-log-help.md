# Design: Crash-log parsing & help (Phase 7 "Error reporting")

Status: proposed. Spec: `docs/spec/crash-log-help.md`.

## Problem

When a launched instance dies, the user today sees only a dead log console and an exit code.
MC writes a rich `crash-reports/crash-*.txt` and the log stream itself carries loader-level
failure text (unmet deps, mixin failures, OOM), but the user has to find and read those files
by hand. Goal: on a crashed run, ApexLauncher detects the crash, analyzes it **locally**, and
shows cause + suggested fix inside the app.

**Hard constraints** (from standing rules):
- API frugality: **zero new network calls**. No mclo.gs upload, no remote symbol/mapping
  lookup, no telemetry. "Error reporting" = surfacing to the user, not phone-home.
- No new polling loops. Detection piggybacks on the existing exit path in
  `core/launch.rs::monitor_child`; a one-shot bounded post-exit FS scan is allowed.
- Analysis core is pure (no Tauri types), tested with fixtures, sibling `<stem>_tests.rs`.

## Existing machinery (build on, never duplicate)

- `core/launch.rs::monitor_child` — sole owner of the terminal transition
  (`Exited`/`Killed` + exit code), already distinguishes kill from natural exit
  (`was_killed`), already holds `inst_dir` (game dir = `inst_dir/mc/`), already records
  playtime post-exit. **This is the detection hook.**
- `RunState` — slug-keyed registry entry, retained after exit; holds `log_ring`
  (`LOG_RING_CAP = 1000` lines of stdout+stderr) — a ready-made log tail, no need to read
  `logs/latest.log`. `mark_preparing` inserts a **fresh** `RunState` on relaunch, so any
  per-run crash field self-clears on the next launch.
- `LaunchSink` trait — default-method extension point (precedent: `status()` was added the
  same way); `TauriLaunchSink` in `lib.rs` maps sink calls to events; `CapturingLaunchSink`
  captures for tests.
- Frontend: `AppShell` is the sole event subscriber; Zustand `useAppStore` holds
  runs/runLogs; `InstanceDetail.tsx` has the log console; `Toasts.tsx` has the amber-toast
  pattern; `tauri-plugin-opener` is already a dependency (reveal report file for free).

## Evidence: crash artifact formats (verified against real reports)

1. **Report location/shape** — MC writes `<game_dir>/crash-reports/crash-YYYY-MM-DD_HH.MM.SS-client.txt`
   (also `-fml.txt` for Forge/NeoForge loading crashes) *before* the process exits. Layout
   (verified: Fabric docs example report, FabricMC/fabric-docs
   `public/assets/players/crash-report-example.log`; NeoForge 1.21 reports in
   GeyserMC/Hydraulic#73 and Apollounknowndev/lithostitched#120):

   ```
   ---- Minecraft Crash Report ----
   // <witty comment>

   Time: 2023-12-28 13:21:22
   Description: Tessellating block in world - Indium Renderer

   java.lang.RuntimeException: java.lang.ClassCastException: class net.minecraft.class_3924 ...
   	at snownee.snow.block.ShapeCaches.get(ShapeCaches.java:51)
   	...
   -- Head --
   ...
   -- System Details --
   Details:
   	Minecraft Version: 1.20.1
   	...
   ```
   Description line, first exception block (class[: message] + `\tat ` frames), `-- X --`
   section headers, `Key: Value` details. Stable across vanilla + all loaders since ~1.14.

2. **Fabric/Quilt specifics** — frames are intermediary-mapped (`net.minecraft.class_4970`),
   **no jar/mod annotations on frames**; System Details contains a `Fabric Mods:` list,
   one `\t<modid>: <Name> <version>` per line. Mod attribution on Fabric therefore comes
   from (a) non-vanilla package prefixes in frames and (b) loader error text.

3. **Forge/NeoForge specifics** (verified, GeyserMC/Hydraulic#73, 1.21.x):
   frames carry a module prefix and a jar annotation:
   ```
   at TRANSFORMER/hydraulic@1.0.0-SNAPSHOT/org.geysermc.hydraulic.pack.PackManager.lambda$initialize$0(PackManager.java:99) ~[hydraulic-neoforge.jar:?] {}
   at java.base/java.util.stream.ReferencePipeline$3$1.accept(...) ~[?:?] {}
   ```
   `TRANSFORMER/<modid>@<version>/` → mod id directly; `~[<jar>...]` → jar name (vanilla =
   `~[?:?]` or `client-…jar%23N!/`-style refs). Newer NeoForge additionally annotates
   `{mixin from mod <modid>: <mixin class>}` on affected frames (verified,
   lithostitched#120). System Details has `Mod List:` with pipe-separated columns:
   ```
   	Mod List:
   		hydraulic-neoforge.jar    |Hydraulic    |hydraulic    |1.0.0-SNAPSHOT    |Manifest: 3e5e…
   ```

4. **Loader startup failures print to the log, not always to a crash report.**
   Fabric: `net.fabricmc.loader.impl.FormattedException: Mod resolution encountered an
   incompatible mod set!` followed by `Unmet dependency listing:` /
   `- Mod '<Name>' (<id>) <ver> requires version X or later of mod '<Name>' (<id>), … !` /
   `…which is missing!` (verified: FabricMC discussions #3350, #4237). Forge/NeoForge:
   `Missing or unsupported mandatory dependencies:`. So the analyzer must consume the
   **log tail** as a first-class input, not just the report — the in-memory log ring covers
   this with no file I/O.

5. **JVM-native crashes** write `hs_err_pid<N>.log` into the JVM cwd (= `mc/`) and exit with
   e.g. `-1073741819` (0xC0000005) on Windows or signal codes (134/139 → `code() == None`
   on Unix). No MC crash report exists in that case.

## Decision

**New pure module `src-tauri/src/core/crash.rs`** (+ sibling `crash_tests.rs`), three pure
layers, one thin wiring layer:

1. **Parse** — `parse_crash_report(&str) -> ParsedReport`: description, first exception
   (class/message/frames, frame count capped), `Minecraft Version`, suspect mod ids
   (`TRANSFORMER/<id>@`, `{mixin from mod <id>}`), suspect jars (`~[<jar>…]`, vanilla
   filtered), mod tables (`Fabric Mods:` / `Mod List:` → id/name/version/jar), loader hint.
   Line-oriented, substring/prefix matching only — **no regex dependency** (house style;
   the grammar is trivially line-shaped).

2. **Analyze** — `analyze(AnalyzeInput) -> CrashAnalysis`: a **data-driven, ordered rule
   table** (`RULES: &[Rule]`). Each rule = stable id + any-of substring needles matched
   against exception line + log tail + report text, plus an optional detail-extractor
   (pure fn) that pulls verbatim key lines (e.g. the unmet-dependency listing). First match
   wins; priority: loader-startup deps → duplicate mods → OOM → wrong Java → mixin →
   missing class → native/hs_err (exit-code + file-presence input flags) → GL/driver →
   generic-with-suspects → generic. Adding a rule = adding a table row + fixture test.

3. **Attribute** — `resolve_suspects(...)`: map suspect mod ids/jars against the report's
   own mod tables and the instance manifest (`ModEntry.file_name`/`name`) to produce
   display-ready suspect names. Pure; manifest passed in by the caller.

4. **Wire** — in `monitor_child`, after the terminal transition: skip if killed or
   `exit_code == Some(0)`; else one-shot scan of `mc/crash-reports/` for `*.txt` with
   mtime ≥ run start (wall clock — `RunState` gains `started_wall: SystemTime`), **one**
   750 ms sleep + rescan if none found (bounded race absorber; MC normally flushes the
   report before exiting), plus an `hs_err_pid*.log` presence scan in `mc/`. Feed report
   text (8 MiB cap) + last 300 ring lines + exit code into `analyze`; store the result in
   `RunState.crash`; call new `LaunchSink::crashed(slug, &analysis)` (default no-op).
   `TauriLaunchSink` emits a new `crash://analyzed` event.

**Surfacing UX (minimal new surface):**
- New Zustand map `crashes: Map<slug, CrashAnalysis>` fed by `AppShell` subscribing to
  `crash://analyzed`; cleared for a slug when a `run://update` arrives with
  `preparing`/`running` (relaunch resets — mirrors the backend's fresh `RunState`).
- `InstanceDetail`: a "Crash analysis" panel above the log console when the slug has an
  entry — headline, suggestion, suspect chips, collapsible verbatim detail + exception,
  "Open crash report" (opener plugin `revealItemInDir`), and a "Java settings" link when
  the rule is OOM/wrong-Java. `getCrashAnalysis(slug)` command backfills after an FE
  reload (reads retained `RunState`).
- Amber toast on new crash ("<name> crashed — <headline>"), same pattern as import
  warnings in `Toasts.tsx`.
- Last-crash-only, in-memory per app session (crash files themselves persist on disk and
  the panel links to them). No history browser.

## Alternatives rejected

- **Watch `crash-reports/` with `notify`** (like PendingWatcher) — a persistent watcher per
  running instance for an event that the exit path already signals; violates "no new
  polling/watch loops" spirit and adds lifecycle complexity. Exit-triggered one-shot scan
  is strictly simpler and race-safe enough (report is written pre-exit; one bounded retry).
- **Parse `logs/latest.log`** — redundant with the in-memory log ring (same stream, already
  capped, no file race with the JVM's log flusher), and latest.log is shared/overwritten
  across runs. Ring tail wins. Cost: prep-phase + very-early lines may have scrolled past
  1000 lines in pathological cases — acceptable; the crash report covers late crashes.
- **Exit-code-only detection** — misses "crashed with report but exit 0" (not observed) and
  can't distinguish native crash vs mod crash; report-scan + code together are cheap.
  Conversely **report-only detection** misses Fabric/Forge startup aborts that write no
  report — log-tail rules cover those.
- **Regex crate for rules** — the needed patterns are line prefixes/substrings; adding a
  dependency for that contradicts the repo's lean-deps posture (no `regex` anywhere today).
- **Runtime deobfuscation of vanilla frames** (Mojang mappings / yarn) — requires
  downloading mappings (network, ~10 MB) and a remapper; suspects and rules don't need it
  (exception classes are JDK/loader-namespaced; mod frames are unobfuscated). Deferred
  indefinitely.
- **Persist analysis to `instance.json` / `last-crash.json`** — state duplication for v1
  with unclear invalidation; the source artifacts already persist. Revisit only if users
  ask for post-restart crash review.
- **Auto-fix actions** (one-click disable suspected mod, auto-raise RAM) — destructive
  guesses off heuristic attribution; v1 navigates and names, the user decides.

## Deferred (named, out of v1 scope)

mclo.gs upload/share · crash-history browser (multi-crash list) · analysis persistence
across app restart · mapping-based deobfuscation · `hs_err_pid` content parsing (presence +
path only in v1) · auto-fix actions · Failed-prep (pre-spawn) analysis (already surfaced
via `mark_failed` + prep logs) · localization of rule text.

## Evidence trail

- Fabric crash-report structure + example report — FabricMC/fabric-docs
  (`players/troubleshooting/crash-reports.md`, `crash-report-example.log`) — **supported**.
- NeoForge frame annotations (`TRANSFORMER/<id>@…`, `~[jar:?] {}`), `Mod List:` pipe
  columns — GeyserMC/Hydraulic#73 issue body (full 1.21 crash report) — **supported**.
- `{mixin from mod <id>}` frame annotation — Apollounknowndev/lithostitched#120 (NeoForge
  1.21.1 report) — **supported**.
- Fabric unmet-dependency wording (`Mod resolution encountered an incompatible mod set!`,
  `Unmet dependency listing:`, `requires version … of mod …`, `which is missing!`) —
  FabricMC org discussions #3350/#4237 — **supported**.
- `-fml.txt` crash-report variant exists — observed filename `crash-…-fml.txt` in the wild
  (scribd-hosted NeoForge loading report) — **supported (weak source; harmless either way,
  the scan globs `*.txt`)**.
- Forge 1.20.x `~[…%23N!/…]` jar-in-jar refs inside frame brackets — consistent across
  secondary sources; parser treats bracket content opaquely (extract up to first `%` or
  `!`) — **mixed (format tolerated generically, not load-bearing)**.
- Crash report written before process exit — MC behavior, universally observed; the 750 ms
  single retry absorbs stragglers — **supported**.
