# alva-engine-adapter-codex
> OpenAI Codex CLI 的 EngineRuntime 适配器（`codex exec --json`）。

## 地位
Hub multi-engine 的 Codex 后端。与 Claude adapter 对称：子进程 + 事件映射，
不碰 `alva-app-core` / CLI / Tauri。SDK-clean，只依赖 `alva-engine-runtime`
与 `alva-kernel-abi`。

## 逻辑
1. `CodexAdapter::execute` 为每次请求 spawn `codex exec --json -C <cwd> -s <sandbox>`。
2. stdout JSONL（`thread.started` / `item.*` / `turn.*` / `error`）由 `EventMapper`
   转为 `RuntimeEvent`（Message / ToolStart / ToolEnd / Error / Completed）。
3. `cancel` 通过 session map 向 child 发 kill 信号。
4. 默认 `approval_policy="never"` + `workspace-write` sandbox，避免 Hub 单次执行挂在审批上。
5. `resume_session` / interactive `respond_permission` v1 明确 `Unsupported`。

## 约束
- 依赖宿主机安装 `codex`（PATH 或 `CodexAdapterConfig.codex_path`）。
- 不默认 `--dangerously-bypass-approvals-and-sandbox`；需调用方显式打开。
- 不打印/记录 auth token；stderr 仅 warn 非空行。
- 原生 CLI/Tauri 禁止依赖本 crate（Rule 18）。
- stream contract：`item.updated` 不得重复 `ToolStart`；stdout EOF 后 adapter 统一补 `Completed`。
- cindy 对齐：`Reconnecting...` / 网络类 error 标 `recoverable: true`（UI 不释放 running）；硬错误 `recoverable: false` + 最终 `Completed`。

## 业务域清单
| 名称 | 文件 | 职责 |
|---|---|---|
| CodexAdapter | adapter.rs | EngineRuntime 实现、session kill map |
| CodexAdapterConfig | config.rs | binary 路径、sandbox、model、config overrides |
| CodexProcess | process.rs | 子进程 spawn / JSONL 读 / kill |
| CodexEvent | protocol.rs | `codex exec --json` 线格式 |
| EventMapper | mapping.rs | JSONL → RuntimeEvent |
