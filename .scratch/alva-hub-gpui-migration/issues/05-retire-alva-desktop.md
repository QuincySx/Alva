# 05 — Retire the Standalone Alva Desktop

**Status:** blocked by Gate A

## Deliverable

After explicit Gate A verification and user confirmation:

- delete `crates/alva-app-gpui` and `crates/alva-app-tauri`;
- remove `alva-app-gpui` from the workspace and dependency checks;
- remove its dedicated packaging scripts and generated artifacts;
- retain `alva-app-cli`, `alva-app-core`, Alva engine support, and Inspector data
  production;
- add no session migration or compatibility path.
