# SKILLS — src-tauri/ (Rust backend)

The Tauri 2 Rust side. Owns everything heavy: downloads, instance management, Java
management, game launch, and Microsoft auth (most not built yet — see `docs/ROADMAP.md`).

## ⚠️ Must source cargo env first
Rust isn't on the default shell PATH. Prefix every cargo/tauri command:
```bash
. "$HOME/.cargo/env" && cargo check     # run from this folder
```

## Files
- `Cargo.toml` — crate `modloader` (lib name `modloader_lib`, crate-type staticlib/cdylib/rlib).
  Deps so far: `tauri`, `tauri-plugin-opener`, `serde`, `serde_json`. Add HTTP/zip/hashing/
  keyring crates as phases need them.
- `tauri.conf.json` — app config. `identifier` is `com.bear.modloader` (replace before
  shipping). Window 800×600, `beforeDevCommand: npm run dev`, `frontendDist: ../dist`.
- `build.rs` — `tauri-build` codegen.
- `capabilities/` — Tauri permission sets (ACL). Grant plugin/command perms here.
- `icons/` — bundle icons.
- `src/` — Rust source (see its SKILLS.md).

## Conventions
- Keep command handlers thin; real logic goes in `core/` modules (to be created per phase:
  `instances`, `providers`, `packs`, `minecraft`, `loaders`, `java`, `download`, `launch`, `auth`).
- Structs crossing the IPC boundary use `#[serde(rename_all = "camelCase")]` to match the TS side.
- Editing files here triggers the `tauri dev` watcher to rebuild.
