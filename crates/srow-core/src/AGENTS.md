# src (crate 根)
> srow-core crate 源码根目录

## 地位
`srow-core` 是 srow-agent 平台的核心引擎库，提供 Agent 运行时、工具系统、MCP 协议集成、Skill 框架、安全层、记忆系统、持久化、环境管理和多 Agent 编排。

## 逻辑
`lib.rs` 声明 11 个顶级模块并大量再导出公共 API，`error.rs` 定义两个根错误类型（EngineError、SkillError）。模块依赖方向：agent -> {domain, ports, adapters, mcp, skills, environment}。

## 约束
- 遵循 DDD 六边形架构：domain（实体）-> ports（接口）-> adapters（实现）
- lib.rs 的再导出是外部 crate 的唯一公共 API 入口

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| lib | `lib.rs` | 模块声明 + 公共 API 再导出 |
| error | `error.rs` | EngineError、SkillError |
| types/ | `types/` | 共享类型再导出 |
| domain/ | `domain/` | DDD 领域模型 |
| ports/ | `ports/` | DDD 端口接口 |
| adapters/ | `adapters/` | DDD 适配器实现 |
| base/ | `base/` | 基础设施 |
| system/ | `system/` | 系统能力（占位） |
| gateway/ | `gateway/` | API 网关（占位） |
| environment/ | `environment/` | 嵌入式运行时管理 |
| mcp/ | `mcp/` | MCP 协议集成 |
| skills/ | `skills/` | Skill 可插拔能力框架 |
| agent/ | `agent/` | Agent 核心子系统 |
| bin/ | `bin/` | CLI 可执行入口 |
