# UI Overhaul + Instance Detail rework (spec)

Status: **APPROVED (human, 2026-06-19) — ready to build.** Source of truth:
`docs/handoff/ui-overhaul-brief.md`. Design + reality findings + resolved questions:
`docs/design/ui-overhaul.md` §8. One integration branch `ui-overhaul`, sequenced checkpoints.

**Locked decisions:** Java toggle = instance's own override vs global; `react-markdown` for Info;
omit `-Xms` when min unset; nested-route tabs; sidebar = localStorage live + Settings default;
toggles T1/T2/T3/T5/T6/T7 (T4 dropped); no `SCHEMA_VERSION` bump; Windows-first java picker.

Contract: rework the launcher UI per the brief and wire per-instance Java/RAM through to the JVM.
Decomposed into **six independently-shippable workstreams** (WS-A…WS-F). Recommended landing on one
integration branch `ui-overhaul` with sequenced checkpoints (design §7, Q8) — WS-A/WS-B/WS-C are
independent clusters; WS-D/WS-F share `InstanceDetail.tsx` and are sequenced.

**Gate vocabulary.** Backend checkpoints: `scripts/build.sh check` + `scripts/build.sh test <filter>`
(new tests in the sibling `<stem>_tests.rs` per CLAUDE.md). UI checkpoints: `scripts/build.sh check`
(tsc) + **smoke-test in the dev window** (`scripts/build.sh dev`) — there are no frontend unit tests
yet. **Any checkpoint that changes a Rust DTO/command/event MUST regenerate `src/lib/bindings.ts`**
via `scripts/build.sh dev` on Windows (wait for `[bindings] exported`, then stop) — called out inline.

New instance/settings fields use `#[serde(default)]`; **no `SCHEMA_VERSION` bump** unless Q9 says
otherwise.

---

## WS-A — Backend Java/RAM foundation (per-instance config reaches the JVM)

Closes gaps G1/G2/G3. **Prerequisite for WS-D's Java + Tech tabs.** No UI in this workstream.

| CP | Deliverable | Files | Done when |
|----|-------------|-------|-----------|
| **A-1** | Extend `JavaCfg`: add `min_memory_mb: Option<u32>`, `path_override: Option<String>`, `use_pack_settings: bool` — all `#[serde(default)]`. Keep `memory_mb` (max), `major`, `args_override`. Add a `recommended: Option<RecommendedJava>` block to `Source` (always `None` for now — design §5). | `src-tauri/src/core/instances.rs`, `instances_tests.rs` | `scripts/build.sh check` passes; a round-trip test deserializes an **old** `instance.json` (no new fields) into the new struct with correct defaults. |
| **A-2** | Pure `resolve_effective_java(inst: &Instance, settings: &Settings) -> EffectiveJava` helper implementing the precedence (recommended → per-instance when `use_pack_settings` → global default). Returns `{ xmx_mb, xms_mb: Option<u32>, extra_args: Vec<String>, java_path: Option<PathBuf> }`. | `src-tauri/src/core/launch.rs` (or a new `core/java_resolve.rs`), sibling `_tests.rs` | `scripts/build.sh test` green incl. new unit tests covering all three tiers + the `use_pack_settings` on/off branch + missing-min default behavior (Q5). |
| **A-3** | `build_argv` / `default_jvm_args` emit `-Xmx{xmx}M` (+ `-Xms{xms}M` when set) + `extra_args` from `EffectiveJava`; today they emit **no heap args** (`launch.rs:362-371`). | `src-tauri/src/core/launch.rs`, `launch_tests.rs` | `scripts/build.sh test` green; new test asserts `-Xmx` present in assembled argv for a given config; legacy/empty-args path still produces a valid argv. |
| **A-4** | `launch_instance` resolves effective Java at `lib.rs:840-851`: if `java_path` is `Some`, use it directly and **skip** `ensure_java`; else `ensure_java(major)` as today. Feed `EffectiveJava` into `build_argv`. | `src-tauri/src/lib.rs` | `scripts/build.sh check` + `scripts/build.sh test` green; manual dev-window launch with a custom `memory_mb` shows `-Xmx` in the spawned argv (log/smoke). |
| **A-5** | Regenerate `bindings.ts` for the new `JavaCfg`/`Source`/`Settings` fields; verify `ipc.ts` adapter compiles. | `src/lib/bindings.ts` (generated), `src/lib/ipc.ts` | `scripts/build.sh dev` emits `[bindings] exported`; `scripts/build.sh check` (tsc) passes with the new fields visible to the frontend. |

## WS-B — Provider pack-info backend (Info tab data)

Closes gap G4. **Prerequisite for WS-D's Info tab content.** Parallel to WS-A.

| CP | Deliverable | Files | Done when |
|----|-------------|-------|-----------|
| **B-1** | Add `get_project(client, project_id) -> PackInfo` to the `ModProvider` trait (`providers.rs:273-292`). `PackInfo { title, description, icon_url, body_is_html }`. | `src-tauri/src/core/providers.rs`, `providers` sibling tests | `scripts/build.sh check` passes; trait + DTO compile. |
| **B-2** | Modrinth impl: `GET /v2/project/{id}` → `body` (Markdown) → `PackInfo`. Injectable HTTP seam (existing `ProviderHttpClient`), fixture-tested. | `src-tauri/src/core/modrinth.rs`, sibling tests, `core/fixtures/` | `scripts/build.sh test providers` (or modrinth filter) green with a fixture-driven parse test. |
| **B-3** | CurseForge impl: `GET /v1/mods/{id}` (+ `/description` for full body, HTML) → `PackInfo { body_is_html: true }`. Fixture-tested; honors CF API key resolution. | `src-tauri/src/core/curseforge.rs`, sibling tests, `core/fixtures/` | `scripts/build.sh test` green with a fixture parse test. |
| **B-4** | `get_pack_info(provider, project_id)` Tauri command in `lib.rs` dispatching to the provider; regenerate `bindings.ts`. | `src-tauri/src/lib.rs`, `src/lib/bindings.ts`, `src/lib/ipc.ts` | `scripts/build.sh check` + `test` green; `bindings.ts` shows `getPackInfo` + `PackInfo`; `[bindings] exported` seen. |

## WS-C — Collapsible / interactive sidebar (independent)

Closes gap G5. No dependency on A/B. Requirement 1.

| CP | Deliverable | Files | Done when |
|----|-------------|-------|-----------|
| **C-1** | Add a persisted `ui` slice to the Zustand store: `sidebarCollapsed: boolean` + `toggleSidebar()`, wrapped in `zustand/persist` (localStorage key) — design §4.3 / Q1. | `src/lib/store.ts` | `scripts/build.sh check` (tsc) passes; collapse state survives a dev-window reload (smoke). |
| **C-2** | Sidebar renders collapsed (icon-only, narrow width) vs expanded; a toggle control (hamburger/chevron) flips `sidebarCollapsed`; width transitions; nav labels/auth control adapt. | `src/components/Sidebar.tsx`, `src/components/AppShell.tsx` | dev-window: toggle collapses/expands; layout doesn't break at min window size (800px); `check` passes. |
| **C-3** | "Sidebar starts collapsed" **default** toggle in Settings, backed by `Settings.sidebar_start_collapsed` (new `#[serde(default)]` field). On app start, seed the store default from this if no localStorage value. Regenerate `bindings.ts`. | `src-tauri/src/core/settings.rs` (+tests), `src/routes/Settings.tsx`, `src/lib/bindings.ts` | `scripts/build.sh test` (settings round-trip) + `check` green; toggling in Settings changes the first-run collapse state (smoke). |

## WS-D — Instance Detail: Info / Tech Info / Java tabs (content)

Requirement 3 (Info, Tech Info, Java) + 4. **Depends on WS-A (Java/RAM) + WS-B (pack info) + WS-F shell.**

| CP | Deliverable | Files | Done when |
|----|-------------|-------|-----------|
| **D-1** | **Info tab** (default landing): fetches `getPackInfo(provider, projectId)` via TanStack Query (`["pack-info", …]`) when `instance.source` is set; renders title + description + icon. Markdown rendering per Q3 (renderer dep or preformatted first cut). Empty state for instances with no provider source. | `src/routes/InstanceDetail.tsx` (Info panel), maybe `src/components/PackInfo.tsx` | dev-window: opening a Browse-installed instance lands on Info showing the provider description; `check` passes. |
| **D-2** | **Tech Info tab**: Playtime, Last Played, Java (effective path/version), Memory (effective Xmx/Xms), loader, MC version, mod count — reuses existing stat data (`InstanceDetail.tsx:183-202`) relocated + expanded. | `src/routes/InstanceDetail.tsx` (Tech panel) | dev-window: Tech tab shows playtime/last-played (already recorded) + effective memory; `check` passes. |
| **D-3** | **Java tab**: "use pack settings (per-instance) vs global" toggle bound to `JavaCfg.use_pack_settings`; inputs for max RAM, min RAM, extra args, java path (text input — Windows file-picker per Q6); saves via a `set_instance_java(slug, cfg)` command (new) or existing instance-save path. Shows which tier is effective. | `src/routes/InstanceDetail.tsx` (Java panel), `src-tauri/src/lib.rs` (+ `set_instance_java` command + tests), `src/lib/bindings.ts` | `scripts/build.sh test` (command + resolution) + `check` green; dev-window: changing per-instance RAM with toggle on is reflected in `-Xmx` at next launch (smoke). |

## WS-E — "Toggle-everything" Settings audit (mostly independent)

Requirement 2 / design §6. Independent except T2 wiring shares `bindings.ts` regen with WS-A.

| CP | Deliverable | Files | Done when |
|----|-------------|-------|-----------|
| **E-1** | Add a reusable `Toggle` control + a "Behavior" section to Settings. Wire **T2 Offline mode** to the existing `Settings.offline_mode` (settings.rs:35) and fix the missing `offlineMode` in the frontend Settings type (regenerate `bindings.ts`). | `src/routes/Settings.tsx`, `src/lib/bindings.ts`, `src/lib/ipc.ts` | `check` passes; toggling Offline mode persists + round-trips; dev-window smoke. |
| **E-2** | Add the resolved toggle fields **T3 auto-download-Java, T5 show-console-by-default, T6 keep-launcher-open-after-launch, T7 maximize-window-on-start** to `Settings` (`#[serde(default)]`) + UI controls; honor each at its call site (T3 gates `ensure_java`; T5 sets `InstanceDetail` console default; T6 launcher window behavior post-launch; T7 reads in place of the hardcoded `tauri.conf.json maximized:true`). **T4 confirm-delete is OUT of scope** (stays hardcoded). Regenerate `bindings.ts`. | `src-tauri/src/core/settings.rs` (+tests), `src/routes/Settings.tsx`, relevant call sites, `src-tauri/src/lib.rs`/window setup (T7), `src/lib/bindings.ts` | `scripts/build.sh test` (settings round-trip) + `check` green; each toggle visibly changes its behavior in the dev window. |

## WS-F — Instance Detail shell: header + tab bar + Modlist (replace slide-over)

Requirement 3 (header keeps name/version + **adds pack version**; tab bar; Modlist replaces "Manage
installs"). The shell + Modlist can land **before** WS-D fills Info/Java/Tech (stub those tabs).

| CP | Deliverable | Files | Done when |
|----|-------------|-------|-----------|
| **F-1** | Restructure `InstanceDetail` into a persistent **header** (name, MC version, loader, **provider pack version** from `source.pack_version`, Launch/Stop, running badge, console) + a **tab bar** under it. Tabs via nested routes or local state (Q2). Info/Tech/Java start as placeholder panels. | `src/routes/InstanceDetail.tsx`, possibly `src/router.tsx` | dev-window: header shows pack version; tab bar switches between (stub) tabs; Launch/Stop + console still work; `check` passes. |
| **F-2** | **Modlist tab**: relocate the existing installed-mods list (`folderMods` + enable/disable/update/remove) into a full-screen scrollable panel; relocate the provider-search "Add mods" flow (existing `useInfiniteQuery`) into the tab. **Delete** the "Manage installs" button, `SlideOver` usage, and `ManageInstallsPanel` (`InstanceDetail.tsx:249-275, 439-509`). Carry over `pack_locked` gating. | `src/routes/InstanceDetail.tsx`, remove `SlideOver` import (check no other consumers) | dev-window: Modlist tab lists all mods full-screen; enable/disable/update/remove + add-mod work; the old slide-over is gone; `check` passes; `scripts/build.sh test` still green (no backend change). |

---

## Notes / constraints

- **bindings.ts is generated** — never hand-edit; regenerate on Windows via `scripts/build.sh dev`
  (wait for `[bindings] exported`). Affected CPs: A-5, B-4, C-3, D-3, E-1, E-2.
- **Test convention:** new Rust tests go in the sibling `<stem>_tests.rs`, wired via the `#[path]`
  stub (CLAUDE.md). Module-scope `#[cfg(test)]` scaffolding stays in the source file.
- **No frontend unit tests exist** — UI CPs are gated by `tsc` (`check`) + dev-window smoke only.
- **Provider-recommended settings are plumbing only** — neither modpack format exposes RAM/Java
  (design §5). The "recommended" tier is always empty today; the real fallback is per-instance →
  global 4096 MB. UI copy must not imply the modpack author shipped these values.
- **Cross-platform Java config deferred** — data model + resolution are platform-agnostic; native
  file-picker + `java -version` validation are Windows-first (Q6).
- **Global 4 GB default already satisfied** — `settings.default_memory_mb` defaults to 4096
  (settings.rs:42-44); no change needed to meet the brief's number, only to make it *reach the JVM*
  (WS-A).
- **Out of scope:** CI/frontend test harness, theming overhaul, Browse/Home redesign (Home explicitly
  "leave as-is" per brief §2), signing/auto-update.

## Change log

- 2026-06-19 — **Approved.** All 9 open questions resolved (design §8): one `ui-overhaul` branch;
  Java toggle = instance override vs global; `react-markdown`; omit `-Xms` when unset; nested-route
  tabs; toggles T1/T2/T3/T5/T6/T7 (T4 dropped); no schema bump; Windows-first java picker. E-2 toggle
  set updated. Ready for implementation.
- 2026-06-19 — Initial spec drafted (planning only). Six workstreams WS-A…WS-F; reality check on
  provider-recommended settings recorded; 9 open questions surfaced for human approval. Not built.
```
