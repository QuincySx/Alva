# 横向支撑 · Standard ACP client
> 新实现的官方 Agent Client Protocol v1 JSON-RPC 客户端，不是旧 Rust custom envelope 的兼容层。

## 地位
独立native协议；仅stdlib、wire纯类型校验、kernel ABI取消分类、platform/system/process。kernel/agent-core不导入本包。

## 逻辑
显式argv/env/cwd启动owned进程；先initialize major=1，再创建独立session并prompt/update/permission/cancel。
协议原字节通过可选同步Observer留给外层持久化；本包不把完整IO/schema送主AI、不选择存储或审批政策。
取消先发session/cancel并等待真实prompt响应；不在收到本地context取消时伪造Agent已停止，超时关闭并真实回收owned进程。
Client.Done只在所有协议worker和owned进程真实退出后关闭，Err此时为最终证据；Close另返回真实observer/permission handler及进程清理失败。
Observer使用独立WithoutCancel的有界deadline（默认5秒），Close/取消仍记录已读尾字节；公开ObserverError固定文本，Unwrap保留真实原因供可信审计。
stderr复用platform/system/process.ReadLines保留完整LF，UTF-8/read边界不拆frame；EOF partial和1MiB超限prefix有明确stage，超限停止而非假称完整。脱敏政策由外层按frame实施。
权限请求深拷贝交给callback，原offered option snapshot不可改；writer提交前再次按原turn取消状态投影，不复用过期权限。

## 约束
官方v1是Latest，v2仍Draft；不自动识别/转换旧acp_event_type或v2。不做ambient PATH/npm/npx安装发现、全局orphan扫描、自动重启。
队列/帧/活动请求/会话/回调有硬界；Observer失败fail-closed。用户update必须持续消费，否则明确队列超限停交互，不静默丢失。
默认1MiB/帧（最高4MiB）、共享8MiB逻辑queued字节（写/update/callback/response）、32活动RPC/回调、64会话、初始化/创建metadata各64KiB。
这是保留payload预算，不冒称进程RSS上限；typed解码/深拷贝仍有有界额外开销。Observer与PermissionHandler必须响应context，Close超时明确返回未收尾而不假称完成。
platform/system/process只提供owned生命周期，不是外部Agent自身OS行为的沙箱；启动和具体能力授权在产品/Plugin，不从cwd字符串推断授权。
初期不宣称实现fs/terminal/elicitation等宿主能力，initialize不广告它们，未实现请求返回标准method-not-found。
权限回调只能选择Agent实际提供的option；取消时必须返回cancelled，不复用旧Once/跨会话缓存。
旧Rust父子执行职责已由extension/subagent与harness.NativeChildFactory承接；原ACP只有app-core re-export，无实际产品调用。

## 官方依据
官方文档：https://agentclientprotocol.com/protocol/v1/initialization 、session-setup、prompt-turn、tool-calls、transports。
测试schema固定官方commit `00c5a02d7ab96b9d6aa214002d408ce9d972b4e4` 的schema/v1/schema.json可达定义投影；不是运行期完整schema引擎。

## 业务域清单
| 名称 | 文件 | 职责 |
|---|---|---|
| Wire DTO | `types.go` | 官方camelCase请求/响应、内容、更新与权限 |
| Connection | `client.go` | 有界双向JSON-RPC、owned进程和完整观察 |
| Session | `session.go` | 会话/prompt/update/取消与权限绑定 |
| Verification | `client_test.go`, `schema_test.go`, `testdata/` | 本地真实helper及固定官方schema验证 |
