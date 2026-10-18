# 横向支撑 · Platform
> 宿主执行、OS 原语、耐久存储和 WASM 平台实现，不是产品或 SDK 容器。

## 地位

按平台职责归类，不宣称四个子域处于同一依赖层：host 可以消费 L5 harness Runner，system 提供更底层 OS 原语，storage 实现 SDK 窄能力，sandbox 连接真实 native 宿主与 WASIp1 guest。

## 逻辑

host 保有 Agent 根执行、审批/问题 rendezvous 与 command/browser 资源所有权；system 只提供显式进程/媒体/本地 HTTP/秘密输入原语；storage 保有原始 typed facts/artifacts 的单一耐久权威；sandbox 按每 job 能力和预算执行同一 SDK guest。它们不复制 Hub 的产品事实或 UI 状态。

## 约束

SDK 不反向依赖本目录；只有实际 consumer 的显式配置决定访问和授权。执行取消必须等待物理收尾，不以 UI 退订、EOF 或 context bit 冒充回收。Windows DACL/LockFileEx/创建时Job已实现，真实Windows运行与Unix fd3/非Unix沙箱adapter的缺口单独列入MIGRATION.md；fuel与独立QuickJS WASM核心已有实际回归，交叉编译不代替平台运行验收。

## 业务域清单

| 目录 | 职责 |
|---|---|
| `host/` | Agent Controller/审批/问题与 command/browser 的执行及资源 owner |
| `system/` | 底层 owned process、媒体派生/播放、loopback/strictjson、可取消秘密输入 |
| `storage/` | 私有 SQLite、native session records/artifacts、可选持久 memory，不合并 Hub product store |
| `sandbox/` | 真实 WASIp1 guest/native wazero host；共享纯 ABI 在 sdk/sandbox/abi |
