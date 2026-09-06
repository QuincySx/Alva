# 02 Real Engine Desktop Matrix

## 目标

用 packaged Desktop GUI 驱动每个 release Engine，证明 UI → Tauri → adapter → native session → persistence 的真实路径。

## 工作

- 按 Ticket 00 的 release 范围逐个执行 Engine，而不是默认把已注册 Engine 都算作 V1；
- 每个 Engine 执行 fresh session、terminal completion、app restart 和历史重开；
- 对声称支持 native resume 的 Engine 执行 restart/resume；
- 检查 Engine/model/provider 身份、Turn/Attempt/Branch、usage 和 timeline 一致；
- 检查 Stop、permission response、switch 与错误呈现；
- 证据只保留脱敏 prompt、结果摘要、版本、ID 关系和截图。

## 验收

- 每个 release Engine 至少一条 L + P GUI 证据；
- resume 能力按 adapter 实际能力陈述，不强行等价；
- 失败 Engine 保留 truthful readiness/repair 原因，不靠 PATH 手工绕过；
- preview Engine 未通过时不会阻塞 V1，但 UI 和文档必须明确标识。

## 依赖

Ticket 01。

