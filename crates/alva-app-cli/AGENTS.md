# alva-app-cli
> Native CLI harness：provider 配置、headless/interactive UX，以及可选 WASIp1、macOS Seatbelt 或 Linux Landlock 派活入口。

## 地位

本 crate 位于应用层，可依赖 app/host/sandbox；不得把 CLI 决策下沉到稳定 SDK。

## 逻辑

1. `main.rs` 先执行严格的 top-level argv grammar 校验（未知 flag、缺值、重复 singleton 均失败，`--` 后才允许 dash 开头 prompt），再装配 provider 与普通 BaseAgent 路径。
2. `-p --sandbox wasm|os-write` 在 provider 前验证并 canonicalize grant；wasm 只构造宿主 provider，os 则重启完整 native worker，并由 macOS host 用参数化 Seatbelt 包裹或由 Linux child 在创建 runtime 前进入 Landlock。
3. wasm host policy 先解析 bundled `wasm-env` 并以 Explicit 策略展开全文，再通过限大小且限调用次数的 context import 下发；blocking 线程执行 runner，域名白名单进入每次 `RunRequest`，LLM import callback 回到原 Tokio handle 调模型。
4. wasm host 把 PermissionMode 与 max-turns 传入 guest：Ask/Plan 使用只读 preopen，guest permission middleware 让禁用工具返回可恢复错误；LLM callback 同时执行 wall-clock deadline、调用次数预算、输出 token 上限与序列化 response 字节预算，并删除 guest 的 free-form provider `extra_body`。
5. host escalation 位于 WASM 圈禁之外，因此 Ask/AcceptEdits/AcceptShell/Plan 全部 fail-closed；只有 Bypass + `--dangerously-allow-unsandboxed` 双重显式 opt-in 可执行，且仍必须先经过 per-run import call budget、按本次 grants 把 guest cwd 翻译为 canonical host cwd，并受剩余 run deadline 约束。
6. jobs 子进程由宿主持有 `tools.jsonl` 路径：native middleware、限次数且只接受 `tool_call` kind 的 wasm log import 与可信 host `escalation_request`/`escalation_result` 实时追加同一格式；job runner 独占 `-p --output-format json`，拒绝调用方覆盖，job 目录/文件使用 0700/0600 且不进入 guest args/env/preopen。
7. os 档允许全网络；macOS 只圈禁写、不圈禁读，Linux 则把读写限制到 grant 与显式 runtime support paths。子进程继承 Seatbelt/Landlock，真实内核 enforcement 成功后才开放依赖沙箱的权限模式，`request_escalation` 因不存在外部 host 边界而移除。
8. native agent 只读取 canonical workspace 内、单文件不超过 1 MiB 的 project context；session id 必须是 canonical UUID，`.alva/sessions` 拒绝软链接，canonical typed records（兼容旧 event 文件）/index/RunRecord 以 0700/0600 权限原子替换并串行化写入。
9. REPL 未知斜杠命令先查 skill registry：精确命中由 harness 加载正文，未命中保留原 unknown-command 错误；slash command 产出的 `allowed_tools` 作为单轮工具白名单执行并在该轮后清空；TUI 的启动恢复与 `/resume ID` 同步恢复历史 UI，并在每次完成/退出生成 RunRecord。
10. native print/REPL/TUI 全部消费 BaseAgent bounded 事件流，慢终端会向 kernel 施加背压，避免流式 delta 无界堆积。

## 约束

- API key 只保留在宿主 provider，不得写入 module bytes、WASI args/env/preopen 或结果。
- wasm runner 同步调用必须放入 `spawn_blocking`。
- host escalation 除 Bypass + dangerous-unsandboxed 双重 opt-in 外必须拒绝；路径授权必须先于执行，且只能使用当前 `RunRequest.grants`，即使放行也不能接受 guest 提供的 host path。
- worker 生产物按 sidecar 交付；开发期允许 target fallback 或 `ALVA_WORKER_WASM`。
- bundled skill 仓库只在宿主解析，不得作为 grant/preopen 或宿主路径暴露给 worker。
- project context 与 session 路径不得通过 workspace 内软链接逃逸；session 持久化不得 truncate-in-place，typed batch 的文件替换失败必须回滚内存批次，且敏感会话/job 文件不得对 group/other 开放。
- bundled skill cache 必须逐文件校验嵌入内容，不能仅凭版本目录与文件存在性信任可写缓存；不完整、被修改或含软链接的版本目录必须安全重建。
- macOS os 档不得宣称限制读取；Linux os 档必须如实列明额外只读 runtime roots。grant、host 创建的私有 TMPDIR 与 jobs 目录内固定名称 support file 必须 canonicalize，且不能仅凭平台/旗标把 invocation 标为 enforced。
- native 用户界面不得退回 BaseAgent unbounded prompt；兼容 unbounded API 只留给 SDK/wasm 与受控内部调用。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|------|-------------|------|
| CLI 源码 | `src/` | 主入口、agent 装配、UI/subcommand、wasm host policy 与 macOS/Linux os worker 重启。 |
| golden 测试 | `tests/` | 真实二进制 argv/config/stdout/stderr、wasm E2E 与 macOS/Linux os 档完整 worker 契约；核心 wasm 升级测试在 `src/wasm_sandbox.rs` 内运行。 |
