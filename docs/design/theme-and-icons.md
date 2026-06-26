# Design: dark/light themes + instance icons

> Workstream 2 of the Phase-7 polish slices (runs **after** the rustls-tls switch ships).
> Status: design — approved-ready, not implemented.
> Companion spec: `docs/spec/theme-and-icons.md`.

Two related but separable user-facing polish features, sequenced **themes first, then icons**.
Roadmap: "Instance icons, themes (dark/light), skin/cape preview, import from other launchers."
(`docs/ROADMAP.md:116`)

## Problem

1. **Themes.** The app ships a dark UI only. The token system to support a light theme is
   *already present* but inert — there is no way for a user to switch, no persistence, and no
   honoring of the OS preference. We need a user-facing light/dark/system control that applies
   before first paint (no flash).
2. **Instance icons.** Managed packs show their provider pack icon (`source.iconUrl`), but
   **vanilla/unmanaged instances** (and any instance the user wants to personalize) fall back to
   a generic placeholder (`<Box>`/`<Package>`). The manifest already has an `icon` field — but
   it is **dead**: set to `None` at create and never written or read. We need to let the user
   set a custom image for any instance and render it everywhere instances appear.

## Goals

- **Themes:** a `system | light | dark` control in Settings → Appearance; applied with no FOUC;
  live-reacts to OS theme change when set to `system`; persists across restarts.
- **Icons:** "Set icon…" (file picker) + "Remove" for any instance; the chosen image is copied
  into the instance tree; rendered on Home cards and the InstanceDetail header with sane
  precedence (custom icon > pack icon > placeholder).

## Non-goals

- No theme *editor* / custom palettes — just the two built-in token sets already in
  `styles.css` plus `system`.
- No per-theme art, no accent-color picker, no high-contrast mode.
- No icon cropping/resizing UI, no icon library/emoji picker — a single user-supplied image
  file, copied verbatim.
- No skin/cape preview (separate Phase-7 line).

---

## Evidence trail (file:line)

| Fact | Source |
|------|--------|
| Token system already defined as CSS vars; **dark is `:root` default, `.light` flips tokens** | `src/styles.css:6-30` |
| Tailwind v4 light variant already wired: `@custom-variant light (&:where(.light, .light *))` | `src/styles.css:4` |
| `@theme inline` maps `--color-*` utilities to the vars (so `bg-surface`, `text-muted`, etc. re-theme automatically) | `src/styles.css:32-45` |
| `body { background: var(--background); color: var(--foreground) }` — boot paint is dark until a class flips it | `src/styles.css:70-79` |
| No `.dark` class exists — light is opt-in via `.light`; dark = absence of `.light` | `src/styles.css` (whole file) |
| App entry; no theme application today | `src/main.tsx:1-23` |
| `index.html` is minimal: `<head>` has no inline script, body loads `/src/main.tsx` as a module | `index.html:1-13` |
| Precedent: live UI state in **localStorage** via `useUiStore` (`sidebarCollapsed`, `browseProvider`), persisted under key `apex-ui` | `src/lib/store.ts:114-133` |
| Precedent: a Settings field used only as a **first-run seed** for localStorage live state (`sidebarStartCollapsed`) | `src-tauri/src/core/settings.rs:40-41`; `src/components/AppShell.tsx:33-44` |
| Settings model + serde-default back-compat pattern (every field `#[serde(default …)]`) | `src-tauri/src/core/settings.rs:17-106` |
| Settings UI = `SettingRow` rows + `Toggle` controls; "Behavior" section groups UI defaults | `src/routes/Settings.tsx:116-181` |
| AppShell already does an async `getSettings()` on boot (for the sidebar seed) | `src/components/AppShell.tsx:33-44` |
| `Instance.icon: Option<String>` exists, **no serde attr** | `src-tauri/src/core/instances.rs:160` |
| `Instance.icon` is set to `None` at create and **never written or read afterward** (dead field) | `src-tauri/src/core/instances.rs:257` |
| `Source.icon_url` (pack icon) is what's actually rendered today | `src-tauri/src/core/instances.rs:76-78` |
| Home card icon render + placeholder | `src/components/InstanceCard.tsx:113-120` (`<img src={src.iconUrl} …>`), `:122-125` (`<Box>` placeholder) |
| InstanceDetail header icon render + placeholder | `src/routes/InstanceDetail.tsx:255-262` (`<img src={data.instance.source.iconUrl} …>`), `:264-267` (`<Package>` placeholder) |
| Remote images load via plain `<img src={url} referrerPolicy="no-referrer" loading="lazy">` — no wrapper, no `convertFileSrc` | `src/components/InstanceCard.tsx:113-120` (and siblings) |
| `convertFileSrc` is **not** imported anywhere in the frontend yet | (grep `src/` — zero matches) |
| Tauri security: `"security": { "csp": null }` — **no `assetProtocol`** configured | `src-tauri/tauri.conf.json:22-24` |
| Capabilities: `["core:default","log:default","opener:default","dialog:default"]` — no `fs`/`asset` scope | `src-tauri/capabilities/default.json:6-11` |
| File-picker pattern already used: `open({ multiple, directory, filters })` from `@tauri-apps/plugin-dialog` | `src/routes/instance-tabs/JavaTab.tsx:62-78`; `src/routes/instance-tabs/InfoTab.tsx:72-80` |
| App data root = `app.path().data_dir()` + `"ApexLauncher"`; instances under `<root>/instances/<slug>/` | CLAUDE.md (App data dir); `src-tauri/src/core/store.rs` |
| `getAppPaths()` already exposes `dataDir` to the frontend (queried as `["app-paths"]` in Settings) | `src/routes/Settings.tsx:15`, `:101-113` |

External (primary sources, verified):
- Tauri 2 `app.security.assetProtocol = { enable: true, scope: [...] }` registers the `asset:`
  custom protocol; scope is a glob whitelist. Scope **path variables** include `$DATA`
  (= the OS base data dir, i.e. `app.path().data_dir()`), distinct from `$APPDATA`
  (= `data_dir()/<bundleIdentifier>`). — v2.tauri.app/reference/config + /security/asset-protocol.
- `convertFileSrc(path)` (`@tauri-apps/api/core`) converts an absolute FS path to a webview-
  loadable URL (`http://asset.localhost/<enc>` on Windows; `asset://localhost/…` elsewhere).
  Requires `assetProtocol.enable` + the path inside scope; with `csp: null` no `img-src` rule
  blocks it. — same sources.

---

## Part A — Themes

### Decision A1 — token strategy: reuse `.light`, no `dark:` utilities

`styles.css` already does the right thing: a single set of CSS custom properties under `:root`
(dark) that `.light` overrides, with `@theme inline` mapping every `--color-*` Tailwind utility
to those vars. **Switching theme = toggling one class on `<html>`.** Nothing in the component
tree changes; `bg-surface`/`text-muted`/`border-border`/etc. re-resolve automatically.

We therefore do **not** introduce `dark:` variant utilities anywhere — that would be a second,
divergent source of truth. The contract stays: **dark = no class; light = `.light` on
`document.documentElement`.** (A `.dark` class is unnecessary because dark is the `:root`
baseline; we add only `.light`.)

The existing `.light` block defines a partial override (it omits `--primary`, `--accent`,
`--danger`, `--success`, `--radius`, which correctly inherit from `:root`). That is fine — light
mode reuses the dark accent/semantic colors against light surfaces. Polishing the light palette
is in-scope only insofar as the spec's visual gate ("legible in both themes") requires; no new
tokens are mandated.

### Decision A2 — tri-state `system | light | dark`, resolved via `matchMedia`

The stored preference is one of `system` (default) | `light` | `dark`. Resolution:
- `light` → add `.light`.
- `dark` → remove `.light`.
- `system` → `window.matchMedia("(prefers-color-scheme: light)").matches` decides; a `change`
  listener re-applies while the preference stays `system`.

### Decision A3 — persistence location: **localStorage (dedicated key), not the backend Settings model**

This is the one genuinely contestable call (the task leaned toward a backend `Settings` field).
Recommendation and justification:

- Theme is **pure presentation state**, exactly like `sidebarCollapsed` and `browseProvider`,
  which already live in localStorage via `useUiStore` (`store.ts:114-133`). The established
  precedent for *live* UI state in this codebase is localStorage; the backend `Settings` holds
  only first-run **seeds** (`sidebarStartCollapsed`). Theme has no "seed vs live" split worth
  modeling — `system` is a perfectly good first-run default with zero configuration.
- **No-flash demands a synchronous read at boot.** A backend field requires an async
  `getSettings()` IPC round-trip (AppShell already pays this latency for the sidebar seed,
  `AppShell.tsx:33-44`) — during which the dark `:root` paints, then a light user sees a flash
  when the class flips. localStorage is readable synchronously from an inline `<head>` script
  **before the bundle loads**, eliminating FOUC. This is the standard web pattern (next-themes
  et al.).
- **Zero `bindings.ts` regen** for the entire theme feature — no Rust DTO/command/event change.
- Cost of *not* using the backend: the preference doesn't travel inside `settings.json`. For a
  per-device cosmetic choice that's acceptable (sidebar collapse already behaves this way).

**Why a dedicated `apex-theme` key rather than folding into `useUiStore`'s `apex-ui` blob:** the
inline boot script must read the value with a trivial, envelope-free `localStorage.getItem` — it
cannot depend on Zustand's `persist` JSON wrapper shape. So theme gets its own raw string key
`apex-theme` and a tiny standalone module `src/lib/theme.ts` (`getThemePref`/`setThemePref`/
`applyTheme`/`subscribeSystem`), keeping it out of the Zustand store entirely.

Rejected: a `Settings.theme` backend field (+ Settings UI bound to it). Rejected for the FOUC
and bindings-churn reasons above. **Open for the approver to overturn** if cross-device sync of
the theme is wanted — see open questions.

### Decision A4 — application points

- **`index.html`** gets a tiny inline `<script>` in `<head>` (runs during HTML parse, before
  the module bundle) that reads `apex-theme`, resolves `system` via `matchMedia`, and toggles
  `.light` on `document.documentElement`. This is the no-flash guarantee.
- **`src/lib/theme.ts`** owns the same logic for runtime changes + the `system` media-query
  listener. `main.tsx` calls `applyTheme()` once at startup (idempotent with the inline script)
  and registers the system listener.
- **Settings → Appearance** (new section above "Behavior" in `Settings.tsx`) renders a 3-way
  segmented control (System / Light / Dark) bound to `theme.ts`. Changing it applies instantly
  and writes `apex-theme`.

### Themes — UX wireframe (ASCII)

Settings → Appearance (new section, mirrors the existing `SettingRow` styling):

```
┌─ Appearance ───────────────────────────────────────────────┐
│  Theme                                                      │
│  How the launcher looks. "System" follows your OS setting.  │
│                          ┌─────────┬────────┬────────┐      │
│                          │ System  │ Light  │  Dark  │      │
│                          └─────────┴────────┴────────┘      │
│                            (selected = filled bg-primary)   │
└─────────────────────────────────────────────────────────────┘
```

---

## Part B — Instance icons

### Decision B1 — storage: copy into the instance tree, persist a timestamped filename

When the user picks an image, copy it to `<instances>/<slug>/icon-<unixMillis>.<ext>` and set
`instance.icon = "icon-<unixMillis>.<ext>"` (a **relative filename**, resolved against the
instance dir). Remove any prior `icon-*.*` in that dir first.

- **Why on-disk file, not base64-in-manifest:** consistent with the instance tree (mods, mc/,
  manifest already live there); keeps `instance.json` small and diff-friendly; the icon travels
  with the instance folder on copy/backup. Base64 bloats the manifest ~33% and re-serializes a
  blob on every unrelated manifest write — rejected.
- **Why a timestamped filename, not a fixed `icon.png`:** the asset/webview layer caches by URL.
  A fixed name means replacing the image keeps the same URL → the webview serves the **stale**
  cached bitmap. A fresh `icon-<ts>.<ext>` filename changes the URL on every set, so the new
  image shows immediately and the old file is deleted — no cache-busting query-string hacks.
- **Why a relative filename, not an absolute path in the manifest:** the data root is
  machine-specific; an absolute path breaks if the folder moves. The relative name is resolved
  to an absolute path at render time (Decision B3).
- **Validation:** accept a small extension allowlist (`png`, `jpg`, `jpeg`, `webp`, `gif`),
  reject anything else; optionally cap size (e.g. ≤ 4 MB) to avoid pathological copies.

### Decision B2 — command surface (both sync, off the task queue)

Per the task-queue contract, instant local FS ops stay synchronous (like `set_mod_enabled`/
`set_pack_lock`) — copying one small file is not queue work.

| Command | Sync? | Signature | Effect | bindings regen? |
|---------|-------|-----------|--------|-----------------|
| `set_instance_icon` | sync | `(slug: String, src_path: String) -> Result<(), String>` | validate ext, copy into instance dir as `icon-<ts>.<ext>`, delete prior `icon-*`, set `instance.icon`, `write_manifest` | **yes** (new command) |
| `clear_instance_icon` | sync | `(slug: String) -> Result<(), String>` | delete the icon file, set `instance.icon = None`, `write_manifest` | **yes** (new command) |

`Instance.icon` is already a generated DTO field (`instances.rs:160`) so **no DTO shape change** —
only the two new commands force the regen. The picker uses the existing
`@tauri-apps/plugin-dialog` `open()` (image filters), and `dialog:default` is already in
capabilities (`default.json`), so no new permission.

### Decision B3 — display: Tauri asset protocol via `convertFileSrc`

Render a local file in the webview through Tauri's **asset protocol** (the idiomatic,
synchronous mechanism — a string transform, no IPC per image):

1. **Enable the protocol** in `tauri.conf.json` `app.security`:
   ```jsonc
   "assetProtocol": { "enable": true, "scope": ["$DATA/ApexLauncher/instances/**"] }
   ```
   `$DATA` resolves to `app.path().data_dir()` — the **base** OS data dir — so
   `$DATA/ApexLauncher/instances/**` matches the real icon paths on all three platforms. This is
   the key subtlety: we **cannot** use `$APPDATA`, because Tauri resolves that to
   `data_dir()/<bundleIdentifier>` (`…/com.apex.apexlauncher`), whereas this app deliberately
   roots its data at `data_dir()/ApexLauncher` (not the bundle id). `$DATA` sidesteps that
   mismatch with a static scope — no runtime `asset_protocol_scope()` plumbing needed.
2. **CSP stays `null`** (current state) — with no CSP there is no `img-src` rule to block
   `asset:` URLs, so nothing else changes. Recorded caveat: *if* a CSP is ever introduced
   (a later hardening item), it must include `img-src 'self' asset: http://asset.localhost`.
3. **Resolve + render.** In the icon render sites, compute the absolute path from the existing
   `getAppPaths().dataDir` query (already cached under `["app-paths"]`) +
   `/instances/<slug>/<icon>`, pass it through `convertFileSrc`, and use it as `<img src>`:
   ```tsx
   const custom = instance.icon
     ? convertFileSrc(`${dataDir}/instances/${slug}/${instance.icon}`)
     : null;
   const iconSrc = custom ?? source?.iconUrl ?? null;  // precedence
   ```
   Precedence: **custom icon > pack icon (`source.iconUrl`) > placeholder**. Wired at both
   `InstanceCard.tsx:113-125` and `InstanceDetail.tsx:255-267` (and the same pattern is
   available to BrowsePackInfo, but Browse shows remote packs, not instances, so it's untouched).

**Rejected display alternatives:**
- *Runtime `app.asset_protocol_scope().allow_directory(instances_dir, true)` in `setup`.* Works
  and is robust to a relocated data dir, but the static `$DATA/ApexLauncher/instances/**` scope
  already matches and needs no Rust setup code. Keep the simpler static scope; revisit runtime
  scope only if the data root ever becomes user-relocatable.
- *A `get_instance_icon(slug) -> data:image/...;base64` command.* Avoids the protocol/scope/CSP
  surface entirely and is dead-simple, but costs an async IPC + a base64 decode per rendered
  icon (bad for the Home grid) and re-reads the file on every render. Rejected as the primary;
  noted as the fallback if the asset protocol proves troublesome on a target.

**Minor caveat (recorded, not blocking):** the absolute path is assembled in JS by string-joining
`dataDir` with `/instances/<slug>/<icon>`. On Windows `dataDir` uses `\`, so the result mixes
separators; the asset protocol normalizes `/` fine and Windows accepts `/` in paths, so this
works in practice. If it proves flaky on any target, the fallback is to have `set_instance_icon`/
the getter return the already-joined absolute path from Rust (where `PathBuf` handles
separators). See open question.

### Icons — UX wireframe (ASCII)

InstanceDetail header — icon becomes a hover-actionable control:

```
┌──────────────┐
│              │   All the Mods 10            [ ▶ Launch ] [ ⏹ ]
│   <icon>     │   Fabric · 1.20.1 · 4.7
│   ┌────────┐ │
│   │Set icon│ │   ← small overlay button on hover (and a "Remove"
│   └────────┘ │     item when a custom icon is set)
└──────────────┘
```

Icon picker affordance (placeholder state, vanilla instance):

```
┌──────────────┐
│   ┌──────┐   │   Vanilla 1.21
│   │  ▦   │   │   Vanilla · 1.21
│   │ Set… │   │
│   └──────┘   │   (clicking opens the OS file dialog filtered to images)
└──────────────┘
```

---

## New surface summary

### Rust
- `lib.rs`: commands `set_instance_icon(slug, src_path)`, `clear_instance_icon(slug)` (both sync,
  off-queue); a small icon-copy helper (extension allowlist, timestamped target name, prior-icon
  cleanup) — ideally a pure fn in `instances.rs` so it's unit-testable.
- `instances.rs`: the icon-write helper + tests; `Instance.icon` finally becomes a *written*
  field (no shape change).
- `tauri.conf.json`: `app.security.assetProtocol = { enable: true, scope: ["$DATA/ApexLauncher/instances/**"] }`.

### Events / IPC
- No new events. Two new commands → **regenerate `src/lib/bindings.ts`** (one regen, at the icon
  command checkpoint). Themes touch **no** Rust surface → no regen.

### Frontend
- `index.html`: inline no-flash `<head>` theme script.
- `src/lib/theme.ts`: `getThemePref`/`setThemePref`/`applyTheme`/system-listener over the
  `apex-theme` localStorage key.
- `main.tsx`: call `applyTheme()` + register the system listener at startup.
- `Settings.tsx`: new "Appearance" section with the System/Light/Dark segmented control.
- `InstanceCard.tsx`, `InstanceDetail.tsx`: custom-icon precedence via `convertFileSrc`
  (+ `dataDir` from the existing `["app-paths"]` query); a "Set icon…"/"Remove" affordance on
  the InstanceDetail header.
- `ipc.ts`: thin wrappers for `set_instance_icon`/`clear_instance_icon` (generated DTOs
  re-exported as usual).

---

## Tradeoffs / rejected alternatives (consolidated)

- **Theme via `dark:` utilities** — rejected; the `.light`-flips-tokens model already in
  `styles.css` is the single source of truth. No utility sprinkling.
- **Theme in backend `Settings`** — rejected as primary for FOUC (async boot read) + bindings
  churn; localStorage matches the established live-UI-state precedent. (Approver may overturn.)
- **Theme inside `useUiStore`'s `apex-ui` blob** — rejected; the inline boot script needs a raw,
  envelope-free key (`apex-theme`).
- **Icon as base64 in manifest** — rejected; bloats/serializes the manifest. On-disk file is
  consistent with the instance tree.
- **Fixed `icon.png` filename** — rejected; URL-stable name serves stale cached bitmaps after a
  replace. Timestamped filename changes the URL each set.
- **Absolute icon path in manifest** — rejected; breaks on folder move. Relative filename +
  render-time resolution.
- **`get_instance_icon → data URL` command for display** — rejected as primary (async IPC +
  decode per icon); kept as the asset-protocol fallback.
- **Runtime asset-protocol scope** — rejected as unnecessary; the static `$DATA/...` scope
  already matches the custom data root.
- **Icon set/clear as TaskJobs** — rejected; instant local FS op, stays synchronous per the
  task-queue contract.

## Open questions for approval

1. **Theme persistence — localStorage (recommended) vs a backend `Settings.theme` field?** The
   recommendation is localStorage (`apex-theme`) for no-flash + zero bindings churn, matching the
   sidebar/browse-provider precedent. Confirm, or require the backend field if you want the theme
   to live in `settings.json` (cross-device).
2. **Light-palette polish scope.** The existing `.light` block reuses dark accent/semantic colors
   and omits some tokens. Is "legible and coherent in both themes" enough for this slice, or do
   you want a deliberately tuned light palette (more token work)?
3. **Icon path assembly — JS string-join (recommended, simplest) vs Rust-returned absolute path?**
   The JS join relies on the asset protocol normalizing mixed separators on Windows (works in
   practice). If you'd rather not depend on that, the icon command/getter can return the
   Rust-joined absolute path (small extra surface). Default to JS-join unless you object.
