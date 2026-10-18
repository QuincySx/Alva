# L4 · Utility Extension
> 真正可取消 sleep、实际 live registry 检索与有实际消费者的无害配置。

## 地位

可选 Plugin，不向 kernel 增加产品 setter，不搬 Rust config/tool_search 的旧 stub。

## 逻辑

config/tool_search验证实际 Tool/session/run/cwd 与 Guard 后执行。config 三个明确 presentation key 用同会话 typed snapshot/CAS，BeforeModel 真正影响下一请求；tool_search 读取 live AgentState.Tools 实际快照，小目录与选中 schema ref。
sleep复用timer子包的同一个普通Tool实现，独立SDK可直接注册timer.NewTool而不装配本原生Plugin。它只消费调用ctx，不读会话或宿主资源，不需要为等待打开SQLite、恢复偏好或启动Middleware。

## 约束

配置仅 response.language/verbosity/format 明确枚举，绝不 provider/key/权限/工具授权。目录最多512项，匹配name与每description前4KiB，模型preview256字节（明确截断）；schema最大64KiB，先保存后小引用，不触发MCP/AEP。sleep≤5分钟；取消不报完成。产品下一轮opt-in，不自动继承child；装配host security时仍提交实际工具实例，不以名字猜授权。独立timer不携带任何宿主授权。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Plugin | `plugin.go` | 单事实偏好/CAS恢复、真实BeforeModel消费者与生命周期 |
| Tools | `tools.go` | 原生实际注册表/选中schema引用与配置 |
| Portable timer | `timer/` | 无native依赖的普通sleep Tool，独立SDK与本Plugin共用，无新增Plugin类别 |
| 回归 | `plugin_test.go` | 真正下一请求/原system保留、CAS恢复、live registry/ref和timer取消 |
