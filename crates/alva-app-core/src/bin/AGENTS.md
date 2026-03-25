# bin
> 可执行入口

## 地位
提供 `srow-cli` 命令行交互工具，用于测试 Agent 引擎。

## 逻辑
`cli.rs` 初始化 LLM provider、ToolRegistry、MemoryStorage，创建会话并进入 REPL 循环，将 EngineEvent 实时输出到终端。

## 约束
- 需设置 OPENAI_API_KEY 环境变量
- 支持 OPENAI_BASE_URL 和 OPENAI_MODEL 自定义

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| cli | `cli.rs` | srow-cli REPL 入口 |
