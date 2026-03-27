# alva-app-devtools-mcp
> MCP Server 可执行程序，通过 stdio 传输向外部工具暴露 alva 应用的检查与控制能力。

## 地位
独立可执行 crate，不被其他 crate 依赖。通过 HTTP 代理调用 alva-app 内置的 debug API（默认 `http://127.0.0.1:9229`），将其包装为标准 MCP 工具协议。面向 AI 编码助手或外部自动化工具提供运行时应用检查能力。

## 逻辑
1. `AlvaDevtools` 结构体持有 `reqwest::Client` 和 base URL，通过 `rmcp` 框架的 `#[tool_router]` 宏自动生成工具路由。
2. 暴露五个 MCP 工具：`alva_views`（列出视图）、`alva_inspect`（读取视图状态）、`alva_action`（执行视图方法）、`alva_screenshot`（截图）、`alva_shutdown`（关闭应用）。
3. `ServerHandler` 实现通过 `#[tool_handler]` 宏委托给 tool_router，`main` 函数启动 stdio 传输的 MCP 服务。
4. 参数类型 `InspectParams`、`ActionParams` 使用 `schemars::JsonSchema` 派生，自动生成 MCP 工具的 JSON Schema。

## 约束
- 所有工具调用均为 HTTP 代理转发，本 crate 不包含任何 alva 应用的内部逻辑。
- base URL 默认为 `http://127.0.0.1:9229`，需要 alva-app 的 debug HTTP server 处于运行状态。
- 传输层固定为 stdio，适配 MCP 标准客户端调用方式。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| DevTools MCP Server | main.rs | 定义 AlvaDevtools 结构体、5 个 MCP 工具实现和 stdio 服务启动入口 |
