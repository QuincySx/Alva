# alva-hub-cli
> Alva Hub 的最小命令行产品入口。

## 地位

独立于 `alva-app-cli`。后者继续是原生 Alva Agent 产品；本 crate 负责启动 `alva-hub-core`，并按显式 engine 选择注册 Alva / Claude / Codex adapter。

## 逻辑

1. `cli.rs` fail-closed 解析 `--engine alva|claude|codex`、`--` 与 prompt。
2. 只初始化被选中的 adapter：
   - Alva：既有 provider 配置 + `ALVA_API_KEY`
   - Claude：Node bridge + SDK 默认模型 + `ANTHROPIC_API_KEY` / 本地 Claude Code 登录
   - Codex：`codex exec --json`（PATH 上的 Codex CLI + CODEX_HOME 认证）
3. 运行单次 Hub session，输出 final Message；遇到 permission request 立即 fail-closed deny 并继续 drain 到 Completed。
4. `Error.recoverable == true` 只打印 `(retrying)` 横幅，**不**把进程 exit code 置失败；fatal error 才失败（对齐 cindy willRetry）。
5. 流结束后若 `HubSessionSnapshot` 有 stream contract hard/soft 诊断，打印到 stderr（不单独改 exit code）。

## 约束

- 不向 `alva-app-cli`、`alva-app-tauri` 添加 Hub 或 engine runtime 依赖。
- 这是第一条单次执行切片；持久化、交互式 REPL、engine switch、desktop UI 后续实现。
- 不打印 stream delta：Alva 和 Claude 都会追加 final Message，双打会重复输出文本。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| alva-hub | `src/main.rs` | selected-engine bootstrap、Hub registry、fail-closed permission 与 event 输出 |
| CLI grammar | `src/cli.rs` | engine/prompt 参数解析及纯单元测试 |
