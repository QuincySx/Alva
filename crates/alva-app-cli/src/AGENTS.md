# alva-app-cli/src
> CLI 调度、agent harness、终端 UI 与 WASIp1 host 接线源码。

## 地位

应用层实现目录；负责把配置好的 provider 与 SDK/app/sandbox 能力组合成用户可执行命令。

## 逻辑

`main.rs` 是参数和模式总路由；`agent_setup.rs` 负责普通 native agent；`wasm_sandbox.rs` 负责 worker sidecar、preopen 映射与 LLM proxy callback；其余模块承载 UI、session 与子命令。

## 约束

- `--sandbox` / `--grant` / `--allow-domain` 非法值必须在 provider setup 前失败。
- wasm 路径不得构造 native BaseAgent 或让 key 进入 guest surface。
- 新增/修改旗标必须同步 `usage_text()` 与 golden 测试。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|------|-------------|------|
| 顶层入口 | `main.rs` | 模式/flag/config 分流与最终输出。 |
| Native agent | `agent_setup.rs` | BaseAgent/plugin/provider 装配。 |
| WASIp1 host | `wasm_sandbox.rs` | sidecar 发现、spawn_blocking runner、job 域名授权与真 provider proxy。 |
| 终端交互 | `ui/`, `repl.rs` | TUI 与 legacy REPL。 |
| 子命令/服务 | `commands/`, `*_cmd.rs`, `services/` | 命令解析及后台能力。 |
