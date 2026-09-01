# alva-hub-cli
> Alva Hub 的命令行验证、诊断与自动化入口。

## 地位

独立于 `alva-app-cli`。后者继续是原生 Alva Agent 产品；本 crate 负责启动 `alva-hub-core`，并按显式 engine 选择注册 Alva / Claude / Codex adapter。

已接受的 V1 主产品 `alva-hub-desktop` 已实现 ticket 01–03 的 Alva timeline、
私有 SQLite session/attempt resume 与崩溃修复。本 crate 继续承担 headless 自动化、adapter
合约验证、故障诊断和 CLI 回归，不是 Desktop 会话数据库或 UI 状态的事实来源。目标规范见
[`docs/ALVA-HUB-V1.md`](../../../docs/ALVA-HUB-V1.md)。

## 逻辑

1. `cli.rs` fail-closed 解析 engine/session/permission，并提供 `--timeout`、`--max-turns`、`--version`。
2. Session journal（`session_store.rs`）：`~/.config/alva/hub/sessions.json` 或 `$ALVA_HUB_HOME/sessions.json`；带 schema 版本与容量/条数上限，0700 目录 / 0600 文件，拒绝符号链接，用跨进程锁、fsync 与原子替换持久化。
3. `--session <id>` 从 journal 恢复；跨 engine 前先持久化 `pending_handoff_target`，再 `restore` + `switch_engine`，重启会重放未完成的切换。
4. Handoff 只传递经过清洗的 engine/产品会话元数据；不进 journal 的 prompt、model output、tool input、凭据与 native session ID 不跨 engine 传递。
5. stdout 只输出成功 turn 的最终结果，且有 8 MiB 上限；中间事件与脱敏诊断仅进 stderr，失败和 journal 错误必须非零退出。
6. Permission：Alva / Claude Full 用回调执行 `deny` / `allow` / `ask`；Claude Light 映射 `deny=dontAsk`、显式 `allow=bypassPermissions` 且拒绝 `ask`；Codex exec 仅接受 `deny`，不会为 auto-allow 暗中关闭 sandbox。
7. `--repl`：`/engine <name>` 先记录 durable intent，再切引擎并准备 metadata-only handoff context。
8. 每 turn 响应 SIGINT 和 wall-clock deadline；中断/超时会请求 cancel，有界等待 stream 收尾，并返回 130/124。

## 约束

- 不向 `alva-app-cli` / `alva-app-tauri` 添加 Hub 或 engine runtime 依赖。
- Journal 只保存 product→engine 路由与 handoff intent；Alva 另存 `alva-history/alva-agent-records.db` 类型记录以支持跨进程 resume。
- Claude：`claude --resume`；Codex：`codex exec resume <id> --json`。
- stdout 不混 tool 噪声；verbose / permission / journal 一律 stderr。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| alva-hub | `src/main.rs` | selected-engine bootstrap、超时/中断生命周期、持久化路由与最终输出合约 |
| CLI grammar | `src/cli.rs` | engine/prompt/执行边界参数解析及纯单元测试 |
| Session journal | `src/session_store.rs` | 私有、有界、加锁、原子持久化 product→engine 路由与 handoff intent |
| Handoff context | `src/handoff.rs` | 构造一次性、可注入防护的 metadata-only 跨引擎上下文 |
