# SKILLS — src/lib/

Cross-cutting frontend helpers.

## Files
- `ipc.ts` — **the only place that calls Tauri `invoke`.** Every Rust command gets a typed
  wrapper function here plus its TS interface. Currently: `getAppInfo()` → `app_info`.
- `utils.ts` — `cn(...)` class-name merge (clsx + tailwind-merge).

## Convention: adding a backend call
1. Add/confirm the `#[tauri::command]` in Rust and register it (see `src-tauri/src/SKILLS.md`).
2. Add a typed wrapper + interface here. Use `camelCase` fields to match the Rust structs'
   `#[serde(rename_all = "camelCase")]`.
3. Call the wrapper from components via TanStack Query (`useQuery`/`useMutation`), never `invoke` directly.

Future: these types will be generated from Rust (specta/ts-rs) so signatures can't drift —
until then, keep the TS interfaces here matching the Rust structs by hand.
