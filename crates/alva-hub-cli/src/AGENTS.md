# alva-hub-cli
> Alva Hub 的最小命令行产品入口。

## 地位

独立于 `alva-app-cli`。后者继续是原生 Alva Agent 产品；本 crate 负责启动 `alva-hub-core`，并按显式 engine 选择注册 Alva 或 Claude adapter。

## 逻辑

1. `cli.rs` fail-closed 解析 `--engine alva|claude`、`--` 与 prompt。
2. 只初始化被选中的 adapter：Alva 读取既有 provider 配置；Claude 复用 Claude adapter 的默认环境入口（`node` 来自 PATH、模型由 SDK 默认选择、认证来自 `ANTHROPIC_API_KEY` 或本地 Claude Code 登录）。
3. 运行单次 Hub session，输出 final Message；遇到 permission request 立即 fail-closed deny 并继续 drain 到 Completed。

## 约束

- 不向 `alva-app-cli`、`alva-app-tauri` 添加 Hub 或 engine runtime 依赖。
- 这是第一条单次执行切片；持久化、交互式 REPL、engine switch、desktop UI 后续实现。
- 不打印 stream delta：Alva 和 Claude 都会追加 final Message，双打会重复输出文本。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| alva-hub | `src/main.rs` | selected-engine bootstrap、Hub registry、fail-closed permission 与 event 输出 |
| CLI grammar | `src/cli.rs` | engine/prompt 参数解析及纯单元测试 |
