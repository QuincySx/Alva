# L0 · Sandbox 共享契约
> 只保留 guest/host 共用的纯 ABI，不包含 native runner 或 guest 产品装配。

## 地位

可移植 SDK；与 wire 同层。实际 sandbox/host 和 sandbox/guest 位于 platform/sandbox。

## 逻辑

共享版本、硬限额、context/model/fetch/escalation/audit/reference DTO 与严格编解码；承载请求和证据，不授予宿主能力。

## 约束

无 OS、网络、文件、wazero、凭据、授权和持久化实现。新路径不是旧 Rust ABI 的兼容别名；不自动识别历史格式。

## 业务域清单

| 目录 | 职责 |
|---|---|
| `abi/` | L0 版本化有界纯值契约，供 platform/sandbox 宿主与 guest 共用 |
