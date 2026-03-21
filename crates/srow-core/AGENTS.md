# srow-core
> Rust AI Agent 平台核心引擎库

## 地位
`srow-core` 是 srow-agent 项目的核心 crate，提供完整的 AI Agent 运行时栈：从 LLM 交互、工具执行、安全检查到多 Agent 编排。

## 逻辑
遵循 DDD 六边形架构，domain 定义领域实体，ports 定义抽象接口，adapters 提供具体实现。agent/ 子系统驱动核心 agentic loop，skills/ 提供可插拔能力扩展，mcp/ 集成外部 MCP Server，environment/ 管理嵌入式运行时，orchestrator/ 编排多 Agent 协作。

## 约束
- Rust 2021 edition
- 异步运行时：tokio (full features)
- LLM 框架：rig-core
- 浏览器自动化：chromiumoxide (CDP)
- 持久化：rusqlite + tokio-rusqlite (WAL mode)

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| src/ | `src/` | crate 源码根目录 |
| tests/ | `tests/` | 集成测试（skill_system_test、acp_integration） |
| Cargo.toml | `Cargo.toml` | crate 依赖和构建配置 |
