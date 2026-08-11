# 08 — 嵌入协议转换 Gateway

**Status:** planned

## Goal

让经过验证的第三方模型能通过现有 Claude/Codex adapter 使用，同时保持协议和工具调用语义可解释。

## Scope

- 先验证 `alva-app-gateway` 与 `alva-llm-wire` 三种 adapter 的真实转换边界；
- Hub 管理本地 gateway 生命周期、随机端口、健康检查和关闭；
- Claude adapter 与 Codex adapter 增加明确的 endpoint/provider 注入契约；
- 转换 request、stream、tool call、usage 和错误类型；
- gateway failure 进入 readiness/compatibility，不伪装成模型错误。

## Acceptance

- 至少一条 OpenAI-compatible 模型路径分别通过 Claude 和 Codex 完成端到端验证；
- tool call 与 streaming 不丢失、不重排；
- adapter 不依赖 Hub crate，保持 SDK/产品边界；
- 不支持的协议能力由 07 标记为 unsupported，而不是运行时碰运气。

**Blocked by:** 06 — 供应商连接与模型目录; 07 — Engine × Model 兼容性判断.
