# agent/runtime
> Agent 运行时层 —— 引擎 + 工具 + 安全

## 地位
Agent 运行时三大子系统的聚合入口：核心引擎（engine）、工具集（tools）、安全层（security）。

## 逻辑
engine 驱动 agentic loop，tools 提供可执行能力，security 在执行前拦截和审批。三者通过 ToolRegistry 和 SecurityGuard 协作。

## 约束
- runtime 模块不直接暴露给外部，通过 lib.rs 的 pub use 选择性导出

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | 声明 engine、tools、security 子模块 |
| engine/ | `engine/` | 核心 agentic loop 引擎 |
| tools/ | `tools/` | 内置工具集 |
| security/ | `security/` | 安全检查层 |
