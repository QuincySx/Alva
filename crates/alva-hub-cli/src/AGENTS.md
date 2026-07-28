# alva-hub-cli
> Alva Hub 的最小命令行产品入口。

## 地位

独立于 `alva-app-cli`。后者继续是原生 Alva Agent 产品；本 crate 只负责启动 `alva-hub-core` 并注册 Alva 作为一个 engine。

## 逻辑

1. 读取现有 Alva provider 配置。
2. 构造 app-level `AlvaHubAdapter` 和 Hub registry。
3. 运行单次 Hub session，输出文本 events。

## 约束

- 不向 `alva-app-cli`、`alva-app-tauri` 添加 Hub 或 engine runtime 依赖。
- 这是第一条单次执行切片；持久化、交互式 REPL、engine switch、desktop UI 后续实现。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| alva-hub | `src/main.rs` | provider bootstrap、Hub registry、event 输出 |
