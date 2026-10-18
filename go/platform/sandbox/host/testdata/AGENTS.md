# 横向支撑 · Sandbox execution fixtures
> 本地真实 WASIp1 反例，不是产品或联网模型。

## 地位

测试时用当前 Go toolchain 编译到临时目录，不提交 wasm 构建产物。

## 逻辑

probe 直接触发 WASI 文件/环境/资源以及 host fetch/escalation 边界，另用真实 cmd/alva-worker-wasm 验证同份 SDK 循环。

## 约束

不读取凭据或真实用户目录；所有 grants/HTTP servers 为测试临时资源。

## 业务域清单

| 目录 | 职责 |
|---|---|
| `probe` | WASIp1 真实恶意/正常能力调用 fixture |
