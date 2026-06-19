# Brief — UI Overhaul + Instance Detail rework (captured 2026-06-19)

Raw requirements captured from the human during a live click-through on Windows. This is the
**source of truth** for the planning pass (`docs/design/ui-overhaul.md` + `docs/spec/ui-overhaul.md`
are derived from it). Do not lose any item here.

Context: pivot to **Windows functionality + UI**. Mac/Linux installer work is parked in
`docs/STRETCH-GOALS.md` (SG-1). This is described by the human as a **huge rework** to be planned
thoroughly across **multiple branches / planning agents**.

## Order of operations (as dictated)

### 1. Sidebar — interactive / collapsible
- The main-page sidebar should **not always be popped out**. It should be **interactive /
  collapsible** (toggle open/closed).
- The collapse/expand state must be **toggleable in UI Settings**.
- **General principle:** *everything that sounds toggleable should actually be a toggle in
  Settings.* Audit the UI for implicit toggles and surface them.

### 2. Pack instances — Home/main tab
- The main instances tab **looks great** → leave it as-is (no change requested).

### 3. Instance Detail page — major rework
When opening an instance:
- Keep the **current setup header**: name, version, etc.
- **Also show the downloaded modpack version** (the provider pack version, distinct from MC
  version).
- The page should **auto-open into the Info section** of the modpack as downloaded from the
  **provider** (Modrinth / CurseForge / any future provider) — i.e. the provider's pack info /
  description is the default landing view.
- Add a **navbar (tab bar)** under the instance-name header window. Tabs:
  - **Info** (default landing) — provider modpack info/description.
  - **Modlist** — a **full-screen scrollable list** of every downloaded mod. **Remove / scrap the
    current "Manage installs" button** and replace it with this full-screen scrollable list.
  - **Tech Info** — a more technical readout: **Playtime, Last Played, Java, Memory**, and similar.
  - **Java** (settings tab) — a **toggle to use pack settings (pack-specific) or global Java
    settings**. If the modpack ships **recommended settings from the provider**, use them;
    otherwise use the **global default** settings.

### 4. Java / global settings
- **Global default RAM allocation = 4 GB (4096 MB).**
- Java should be **fully configurable on all platforms** — *cross-platform configurability is
  deferred* (plan it, implement Windows first).
- Per-instance Java settings override global when "use pack settings" is on; provider-recommended
  settings take precedence when present, else global default.

## Cross-cutting / principles
- **Toggle-everything:** any implicit on/off behavior should become an explicit Settings toggle
  (sidebar collapse is the first example).
- **Provider-recommended settings:** treat as best-effort — flag during planning whether Modrinth
  `.mrpack` / CF manifests actually expose recommended RAM/Java (likely NOT reliably) and define
  the fallback to global default.
- **Data model additions implied:** playtime accumulation, last-played timestamp, per-instance
  Java config (path, RAM min/max, extra args), pack-version field on the instance.

## Planning instructions (from human)
- Plan this **thoroughly**, broken into **multiple branches** with planning agents.
- This is a **huge rework** — phase it so each branch/checkpoint is independently shippable.
