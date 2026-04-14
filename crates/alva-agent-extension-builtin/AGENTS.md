# alva-agent-extension-builtin
> Reference tool implementations + Extension wrappers for the built-in agent toolbox.

## Role
Houses every first-party tool (file I/O, shell, web, notebook, worktree, team,
task, utility, planning) plus the `Extension` wrappers that group them into
cohesive bundles. Consumers opt into subsets via Cargo features; on wasm32 the
native-only modules are fully cfg-gated off so the crate builds as an
essentially-empty shell.

## Cargo Features
- `core` (default) — file I/O, shell, interaction, plan-mode primitives.
- `utility` (default) — `sleep`, `config`, `skill`, `tool_search`, `view_image`.
- `web` — `internet_search`, `read_url` (pulls in `reqwest`, native only).
- `notebook` — `notebook_edit`.
- `worktree` — `enter_worktree`, `exit_worktree`.
- `team` — `team_create`, `team_delete`, `send_message`.
- `task` — `task_create/update/get/list/output/stop`.
- `schedule` — `schedule_cron`, `remote_trigger`.
- `browser` — re-exports `BrowserExtension` from `alva-app-extension-browser`.

## Public Surface
- `tool_presets::*` — curated tool bundles used by host-native assembly.
- `register_builtin_tools` — registers all enabled tools with a `HostAPI`.
- `wrappers::{Core, Shell, Interaction, Task, Team, Planning, Utility, Web, Browser}Extension` — nine Extension wrappers.
- `LocalToolFs` — native `ToolFs` adapter (cfg-gated off for wasm).

## Dependency Policy
- Depends on `alva-kernel-abi`, `alva-agent-core`, plus native crates
  (`tokio` full, `ignore`, optional `reqwest`, optional
  `alva-app-extension-browser`).
- The `Extension` trait itself lives in `alva-agent-core` — do not redefine it here.
- Heavy app-level domain extensions (browser CDP, SQLite-backed memory) belong
  in `alva-app-extension-*` crates, not here.

## Module Map
| Name | Path | Role |
|------|------|------|
| Tool impls | `src/` | One file per tool (e.g. `read_file.rs`, `execute_shell.rs`) |
| `wrappers/` | `src/wrappers/` | Nine Extension wrappers grouping tools into bundles |
| `local_fs.rs` | `src/local_fs.rs` | Native `ToolFs` adapter (cfg `not(wasm)`) |
| `walkdir.rs` | `src/walkdir.rs` | `walk_dir` / `walk_dir_filtered` helpers over `ignore` |
| `truncate.rs` | `src/truncate.rs` | Byte- and line-level output truncation helpers |
| `lib.rs` | `src/lib.rs` | Feature gates, module wiring, `register_builtin_tools` |
