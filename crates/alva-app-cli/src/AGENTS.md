# alva-app-cli/src
> CLI 调度、agent harness、终端 UI 与 WASIp1 host 接线源码。

## 地位

应用层实现目录；负责把配置好的 provider 与 SDK/app/sandbox 能力组合成用户可执行命令。

## 逻辑

`main.rs` 是参数和模式总路由；`agent_setup.rs` 负责普通 native agent；`os_sandbox.rs` 复用完整 worker 重入边界：macOS 用 Seatbelt 与写拒绝探针，Linux child 在 Tokio runtime 前进入 Landlock 并核验 ABI/status；`bundled_skills.rs` 解包并解析宿主内置 skill；`wasm_sandbox.rs` 负责 wasm-env context 下发、worker sidecar、preopen 映射、host proxy callbacks 及 escalation 的 cwd 翻译/审批/执行；`job_log.rs` 让 native middleware、wasm log import 与 host escalation 共用宿主 JSONL 审计格式；`repl.rs` 对未知斜杠命令做 skill registry 精确点名 fallback；其余模块承载 UI、session 与子命令。

## 约束

- `--sandbox` / `--grant` / `--allow-domain` 非法值必须在 provider setup 前失败。
- wasm 路径不得构造 native BaseAgent 或让 key 进入 guest surface。
- wasm escalation 必须在宿主用当前 grants 翻译 guest cwd 后再审批；Ask 在 headless 下 RejectOnce，不能伪造交互等待通道。
- wasm-env 必须由宿主按 Explicit 注入策略展开后经 context ABI 下发，不能把 skill 目录 preopen 给 guest。
- 新增/修改旗标必须同步 `usage_text()` 与 golden 测试。
- macOS os 档是写圈禁而非读圈禁，动态路径只经 canonicalize + `-D` 进入固定 profile，enforcement 必须由实际 EPERM 探针确认；Linux os 档在任何线程创建前施加 Landlock，ABI v1/status 不足必须 fail-closed，读写根都必须 canonicalize。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|------|-------------|------|
| 顶层入口 | `main.rs` | 模式/flag/config 分流与最终输出。 |
| Native agent | `agent_setup.rs` | BaseAgent/plugin/provider 装配。 |
| Bundled skills | `bundled_skills.rs` | 内置 skill 解包；为 wasm worker 解析并显式展开 wasm-env。 |
| WASIp1 host | `wasm_sandbox.rs` | wasm-env context 下发、sidecar 发现、spawn_blocking runner、job 域名授权、真 provider proxy 与 host escalation policy/execution。 |
| OS sandbox | `os_sandbox.rs` | canonical grant/TMPDIR/support file、macOS Seatbelt worker re-exec/写拒绝探针，以及 Linux pre-runtime Landlock 读写圈禁。 |
| Job 工具日志 | `job_log.rs` | 宿主追加 `tools.jsonl`；native/wasm tool call 与成对 escalation request/result 共用格式并按 kind+call id 去重。 |
| 终端交互 | `ui/`, `repl.rs` | TUI 与 legacy REPL；REPL 含未知斜杠命令的 skill 精确点名 fallback。 |
| 子命令/服务 | `commands/`, `*_cmd.rs`, `services/` | 命令解析及后台能力。 |
