# 横向支撑 · Go Protocols
> 对外协议解析与交互，不拥有产品授权、凭据或历史持久化政策。

## 地位

SDK 外侧的独立协议实现，由 Plugin 消费；kernel/agent-core 不导入本目录。

## 逻辑

Skill 渐进读取、MCP 受控子进程交互分别保持独立职责，原始正文与来源不自动进入模型。

## 约束

不发现全局配置、执行下载或继承宿主凭据；目录/正文/传输均有预算，失败明确返回。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Skill | `skill/` | 元数据、正文和资源三级读取，显式文件根与可恢复归档 |
| MCP | `mcp/` | owned stdio、完整 I/O observer、握手/目录/调用；无产品存储或审批政策 |
| AEP | `aep/` | 第三方 Plugin 双向 JSON-RPC、声明能力上限和有界宿主回调；不等同安全 bundle loader |
| ACP | `acp/` | 新标准v1 JSON-RPC客户端/会话/权限/取消，官方schema回归；旧custom退役、无兼容解析 |
| Browser | `browser/` | 显式owned CDP连接与七类真实操作，不发现/连接用户浏览器或持有产品授权 |
