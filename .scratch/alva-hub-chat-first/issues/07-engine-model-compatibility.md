# 07 — Engine × Model 兼容性判断

**Status:** planned

## Goal

明确告诉用户某个模型是否支持 Alva、Claude Code 或 Codex，并让 fallback 使用同一判断结果。

## Scope

- 定义兼容性结果：supported、unsupported、unknown、probe_failed；
- 区分静态协议要求、供应商声明和运行时 probe；
- 缓存 probe 结果并记录版本、时间和失败原因；
- picker 展示「不支持 Codex」「未验证」等 badge；
- fallback eligibility 与手动选择复用同一服务。

## Acceptance

- `unknown` 不得被当作 `supported`；
- UI、fallback 和 readiness 对同一 model/engine 给出一致结果；
- probe 有超时、取消和无副作用的测试请求；
- 兼容性变化不会修改历史 attempt snapshot。

**Blocked by:** 06 — 供应商连接与模型目录.
