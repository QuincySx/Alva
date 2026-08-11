# 06 — 供应商连接与模型目录

**Status:** planned

## Goal

建立 Cindy 风格的「选择供应商 → 连接 → 选择模型」流程，形成统一模型目录。

## Scope

- 供应商目录支持 Anthropic、OpenAI-compatible、Ollama 和自定义 endpoint；
- 三步 `Sheet/Stepper` 向导：选择供应商、测试连接、选择模型；
- 模型目录保存 display name、model id、provider、endpoint 类型和可选能力声明；
- API key 走现有凭据机制或系统安全存储，Hub DB 只保存引用；
- 连接测试与模型列表错误提供可行动提示。

## Acceptance

- 用户可完成添加、编辑、禁用和删除连接；
- 测试连接不会把 secret 写入日志、timeline 或 SQLite 明文字段；
- picker 可立即看到新增模型；
- 删除连接不会破坏历史 attempt snapshot 的可读性。

**Blocked by:** 05 — Composer Engine/Model 选择器.
