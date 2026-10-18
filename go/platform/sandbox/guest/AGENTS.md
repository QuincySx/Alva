# 横向支撑 · WASIp1 guest
> 同一 SDK AgentLoop + WASI 文件工具和有界宿主 import proxy。

## 地位

纯 SDK 消费者；不依赖具体 provider、harness、原生 platform/host/system、wazero 或凭据。只有 wasip1/wasm 实现真实 import，其余目标显式不支持。

## 逻辑

host context 提供任务/guest workspace/额外 policy；ProxyModel 保留全部 wire chunks。文件 tool 在 guest 内调用 os WASI API，实际 syscall 由 host root handle 限制；大文件先入本 job 的有限 volatile reference，仅按需页读/搜索，不把全文塞模型。

## 约束

- 不包含 key、宿主路径或宿主权限模式；guest policy 只能额外收窄，host 永远重审网络/升级能力。
- file tools 的路径词法检查是 UX，不是 sandbox；真正越界防护在 host WASI adapter。
- guest自己产生的reference当前job lifetime（8 MiB 总量）；native附件/历史reference仅经host实际会话的只读page/search代理，不复制成第二份历史、不挂DB。外层observer负责durable IO；无host能力时显式失败。
- 不提供shell；仅host明确advertise后注册run_script，把源交给独立fresh QuickJS WASM实例，共享job文件/fetch授权而不引入第二套Go runtime；普通脚本错误可恢复，整体fuel耗尽仍停止job。不能以native unsandboxed shell冒充guest script；escalation 明确是离开沙箱的宿主审批路径。
- 单批执行 concurrency=1，import mutex 守一个 pending response；不冒称 WASIp1 线程并行。

## 业务域清单

| 文件 | 职责 |
|---|---|
| `proxy.go` | typed context/model/fetch/escalation/audit proxy |
| `imports_wasip1.go` / `imports_other.go` | 显式目标下的内存 import 或 unsupported |
| `script.go` | 只在明确advertise后注册run_script，严格64KiB源、reference-only结果/可恢复错误，guest无wazero/FFI依赖 |
| `worker.go` | 同一 SDK agent 装配与审计输出 |
| `files.go` | WASI bounded 文件/目录/搜索/编辑工具 |
| `references.go` | volatile/native引用分路、宿主只读代理与native_history小索引tool |
| `files_test.go` | native目标绝不降级直接执行OS文件操作 |
