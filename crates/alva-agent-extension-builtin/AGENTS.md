# alva-agent-extension-builtin
> Reference tool implementations + Plugin wrappers for the built-in agent toolbox.

## Role
Houses every first-party tool (file I/O, shell, web, notebook, worktree, team,
task, utility, planning) plus the `Plugin` wrappers that group them into
cohesive bundles. Consumers opt into subsets via Cargo features. Browser-style
wasm keeps host-only modules cfg-gated off, while WASI builds expose the six
core file tools through a synchronous filesystem adapter.

## Cargo Features
- `core` (default) — file I/O, shell, interaction, plan-mode primitives.
- `utility` (default) — `sleep`, `config`, `tool_search`, `view_image`; `skill` 由 SkillsPlugin 注册并通过 bus registry 接线。
- `web` — `internet_search`, `read_url` (pulls in `reqwest`, native only).
- `notebook` — `notebook_edit`.
- `worktree` — `enter_worktree`, `exit_worktree`.
- `team` — `team_create`, `team_delete`, `send_message`.
- `task` — `task_create/update/get/list/output/stop`.
- `schedule` — `schedule_cron`, `remote_trigger`.
- `browser` — re-exports `BrowserExtension` from `alva-app-extension-browser`.

## Public Surface
- `tool_presets::*` — curated tool bundles used by host-native assembly; the shell preset includes local shell and permission-gated host escalation.
- `register_builtin_tools` — registers all enabled tools with a `ToolRegistry`.
- `wrappers::{Core, Shell, Interaction, Task, Team, Planning, Utility, Web, Browser}Plugin` — nine Plugin wrappers.
- `LocalToolFs` — native `ToolFs` adapter (cfg-gated off for wasm).
- `WasiFs` / `WasiFsMetadata` — WASI `ToolFs` adapter plus synchronous facade
  for capability-confined guest embeddings; subprocess execution is unsupported.

## Dependency Policy
- Depends on `alva-kernel-abi`, `alva-agent-core`, plus native crates
  (`tokio` sync/process/fs/io/time, `ignore`, optional `reqwest`). WASI enables
  only `ignore` for synchronous file search; it does not enable Tokio fs/process.
- The `Plugin` trait itself lives in `alva-agent-core` — do not redefine it here.
- Heavy app-level domain plugins (browser CDP, SQLite-backed memory) belong
  in `alva-app-extension-*` crates, not here.

## Module Map
| Name | Path | Role |
|------|------|------|
| Tool impls | `src/` | One file per tool; `request_escalation.rs` separates the stable request/permission contract from its replaceable native-or-host-import executor. |
| Skill invocation | `src/skill_tool.rs` | 统一 `skill` tool + bus `SkillRegistry` capability contract；真实 registry 由 app SkillsPlugin 发布。 |
| `wrappers/` | `src/wrappers/` | Nine Plugin wrappers grouping tools into bundles |
| `local_fs.rs` | `src/local_fs.rs` | Native `ToolFs` adapter (cfg `not(wasm)`) |
| `wasi_fs.rs` | `src/wasi_fs.rs` | WASI `ToolFs` adapter and synchronous script-binding facade (cfg `wasm + wasi`) |
| `walkdir.rs` | `src/walkdir.rs` | `walk_dir` / `walk_dir_filtered` helpers over `ToolFs` / `ignore` (native + WASI) |
| `truncate.rs` | `src/truncate.rs` | Byte- and line-level output truncation helpers |
| `lib.rs` | `src/lib.rs` | Feature gates, module wiring, `register_builtin_tools` |
