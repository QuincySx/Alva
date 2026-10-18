# L4 · Go Extensions
> 通过 Tool、Plugin、Middleware 接入可选能力，不向 SDK 塞产品政策。

## 地位

具体实现与插件 wrapper；由 app 默认配方或产品显式注册。

## 逻辑

安全插件调用独立 sdk/agent/security 能力，工具通过真实宿主边界执行。

## 约束

无历史兼容桥；未知工具不因为自报只读就获取宿主权力，配置和授权必须显式。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Files | `files/` | 严格文件参数、实际会话绑定、引用读取与备份写入；宿主根句柄位于 platform/host/workspace |
| Security | `security/` | 安全 wrapper、实际 Tool 值身份验证与 effect/HITL 闸 |
| Model retry | `modelretry/` | 显式有界次数/字节/数量/总期限的同模型重试，失败流私有，完整成功流才发布 |
| Tool validation | `toolvalidation/` | 可替换默认schema校验，准备阶段拒绝与冻结执行前复核，不把依赖引入SDK |
| Skills | `skill/` | 元数据提示与按需Skill Tool，替换仓库同样执行参数/实际调用/输出/取消边界，完整正文按会话引用读取 |
| Managed MCP | `mcp/` | 按需单 gateway、实际Tool/native authority、审批至进程收尾的owned stdio 与同会话完整脱敏 journal |
| Subprocess loader | `loader/` | 安全 bundle、AEP七钩子/过期句柄、实际调用gateway、审批取消/join与同会话完整脱敏I/O；产品设置接线继续 |
| Memory | `memory/` | 显式 service 替换、实际调用/严格参数与引用工具、受控 MEMORY.md 扫描与结构化笔记提取 |
| SubAgent | `subagent/` | SDK scope驱动的真实独立native子会话、直接父实际值授权与有限递归；可执行贡献/Attach返回值在child IO前校验，部分setup返回owner也收尾 |
| Hooks | `hooks/` | 显式授权外部Hook与严格控制JSON、pre-intent改参、Notify取消/join、terminal Close/physical cleanup和真实输入/输出引用 |
| Diagnostics | `diagnostics/` | 显式诊断命令/可替换 Manager、严格调用与同 native 证据核对、真实 exit，不搬空 LSP stub |
| Media | `media/` | 显式原件派生、完整清单/哈希与单请求选中图/真实receipt及Tool/model完整取消/join；不自动送原件 |
| Browser | `browser/` | 七实际Tool、显式owned私有profile/启动risk/HITL，join实际Tool/事实与进程收尾，全量大结果仅引用 |
| Shell | `shell/` | 明确shell/clean env、同一真实global lease持至reap，run-owned后台stdin/output/kill、审批/管理调用取消与join及脱敏完整引用 |
| Web | `web/` | 明确network opt-in、无ambient proxy/凭据、审批/risk取消、逐跳安全URL/DNS与有界原文/HTML文本引用，搜索不冒充完整SERP |
| Planning | `planning/` | 实际Tool消费可替换Service；默认typed任务/DAG与单事实CAS，保留native执行/引用authority，不注册假plan-mode stub |
| Interaction | `interaction/` | 独立问题broker与真实回答/取消，非审批假扮回答，长文本仅同会话refs |
| Utility | `utility/` | 独立零native依赖timer Tool与原生Plugin共用；live工具目录/schema refs、session偏好CAS和下一request消费者 |
| Notebook | `notebook/` | 受管JSON cell编辑、pinned祖先与明确hash/mode冲突检查，严格参数/保护字段，不执行notebook或冒充OS hash-CAS |
