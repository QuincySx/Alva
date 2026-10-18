# L4 · Portable Utility Tools
> 独立偏好配置、实际工具目录检索与可取消 timer。

## 地位
普通工具能力；不要求 SQLite、原生会话、Guard 或 Plugin。只依赖标准库、SDK 和纯 strictjson。

## 逻辑
NewConfigTool(PreferenceStore) 只消费明确 presentation key；ApplyPreferences 可从已有 BeforeModel hook 调用，使下一请求消费同一后端，不污染原始历史。NewSearchTool(snapshot, retain) 每次读取实际 live registry，只在明确选择 schema 时保存全文引用，不执行被发现的工具。timer/ 为独立 sleep Tool。需要原生产品政策时，native/ 将相同工具装入既有同名 utility Plugin。

## 约束
PreferenceStore 由宿主绑定，拥有并发/持久化并返回 detached snapshot；工具与请求投影独立校验三类允许值，不能借自有后端注入 provider/key/权限。普通工具没有默认 middleware；宿主明确选择 ApplyPreferences 的消费 hook。目录512项、说明扫描4KiB/preview256字节、schema64KiB、回执16KiB；拒绝输入歧义与无效引用，不触发 MCP/AEP 启动。原生 CAS、身份、授权、脱敏与 next-request 行为继续由 native 适配执行。

## 业务域清单
| 文件/子目录 | 职责 |
|---|---|
| tools.go | 独立 config/tool_search 与严格参数/实际 Tool 绑定、实时目录和 schema refs |
| preferences.go | PreferenceStore/快照/值校验及共享 request-only 投影 |
| tools_test.go | 真内存 SDK 下一请求偏好、late registry/schema 和不安全后端输出拒绝 |
| timer/ | 无原生依赖的可取消 sleep Tool |
| native/ | 原生 utility Plugin、偏好 snapshot/CAS 与具体会话/Guard/引用适配 |
