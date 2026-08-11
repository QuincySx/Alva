# alva-hub-cli
> Alva Hub 的命令行验证、诊断与自动化入口。

## 地位

独立于 `alva-app-cli`。后者继续是原生 Alva Agent 产品；本 crate 负责启动 `alva-hub-core`，并按显式 engine 选择注册 Alva / Claude / Codex adapter。

已接受的 V1 主产品 `alva-hub-desktop` 已实现 ticket 01–03 的 Alva timeline、
私有 SQLite session/attempt resume 与崩溃修复。本 crate 继续承担 headless 自动化、adapter
合约验证、故障诊断和 CLI 回归，不是 Desktop 会话数据库或 UI 状态的事实来源。目标规范见
[`docs/ALVA-HUB-V1.md`](../../../docs/ALVA-HUB-V1.md)。

## 逻辑

1. `cli.rs` fail-closed 解析 `--engine`、`--claude-mode`、`--cwd`、`--session`、`--repl`、`--verbose`、`--permission deny|allow|ask`。
2. Session journal（`session_store.rs`）：`~/.config/alva/hub/sessions.json` 或 `$ALVA_HUB_HOME/sessions.json`；0700 目录 / 0600 文件、符号链接拒绝、跨进程锁 + 原子替换；每 turn 结束后 upsert。
3. `--session <id>`：从 journal 恢复；显式 `--engine` 不同 → `restore` + `switch_engine` + **handoff preamble**（journal 预览 + 可选 Alva transcript 摘录）。
4. Adapter 懒注册；handoff 不自动迁完整对话，只注入一次上下文摘要。
5. 每 turn：stdout final Message；journal 记 `last_prompt_preview` / `last_result_preview`。
6. Permission：Alva / Claude Full 用回调执行 `deny` / `allow` / `ask`；Claude Light 映射 `deny=dontAsk`、显式 `allow=bypassPermissions` 且拒绝 `ask`；Codex exec 仅接受 `deny`，不会为 auto-allow 暗中关闭 sandbox。
7. `--repl`：`/engine <name>` 切引擎并准备 handoff context。

## 约束

- 不向 `alva-app-cli` / `alva-app-tauri` 添加 Hub 或 engine runtime 依赖。
- Journal 保存 product→engine 路由；Alva 另存 `alva-history/alva-agent-records.db` 类型记录以支持跨进程 resume 与 handoff 摘录。
- Claude：`claude --resume`；Codex：`codex exec resume <id> --json`。
- stdout 不混 tool 噪声；verbose / permission / journal 一律 stderr。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| alva-hub | `src/main.rs` | selected-engine bootstrap、Hub registry、fail-closed permission 与 event 输出 |
| CLI grammar | `src/cli.rs` | engine/prompt 参数解析及纯单元测试 |
| Session journal | `src/session_store.rs` | 私有、加锁、原子持久化 product→engine session 路由 |
| Handoff context | `src/handoff.rs` | 从 journal / Alva transcript 构造一次性跨引擎上下文 |
