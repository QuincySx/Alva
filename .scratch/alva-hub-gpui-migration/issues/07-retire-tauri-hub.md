# 07 — Retire the Tauri Hub

**Status:** blocked by Gate B

## Deliverable

After explicit Gate B verification and user confirmation:

- delete `crates/alva-hub-desktop` and its generated artifacts;
- remove obsolete Tauri/Web packaging, test, and documentation paths;
- verify Cargo, CI, docs, and scripts no longer reference the old product;
- keep the GPUI Hub as the only desktop entry.
