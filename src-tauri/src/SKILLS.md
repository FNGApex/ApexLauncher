# SKILLS — src-tauri/src/

Rust entry point and command surface.

## Files
- `lib.rs` — `run()` builds the Tauri app: registers plugins and the command handler via
  `tauri::generate_handler![...]`, then runs. Also defines the commands themselves for now.
  Current command: `app_info()` → `AppInfo { name, version, tauriVersion }` (the Phase-0 IPC
  smoke test).
- `main.rs` — tiny; calls `modloader_lib::run()`. Leave it alone.

## Convention: adding a Tauri command
1. Define `#[tauri::command] fn my_cmd(...) -> T` (return `Result<T, String>` for fallible ops).
2. Derive `Serialize` on returned structs with `#[serde(rename_all = "camelCase")]`.
3. Add the fn name to `tauri::generate_handler![app_info, my_cmd]` in `run()`.
4. Add a typed wrapper in `src/lib/ipc.ts` (see `src/lib/SKILLS.md`).

As the backend grows, move command logic into `core/` modules and keep `lib.rs` as just the
builder + handler registration.
