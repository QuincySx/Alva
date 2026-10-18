# L6 · Explicit model Gateway entry
> strict stdin与显式loopback启动独立Magpie协议Gateway，不启动Agent或读取用户配置。

## 地位
L6 process owner；不依赖Hub/native session或desktop，不保留Rust/YAML-env兼容入口。
## 逻辑
--new-token单独无网络；--secrets-stdin读取≤64KiB {token,routes}，每route明确kind/endpoint/model/api_key。
stdin借用共享secretinput，open pipe取消会真实结束读取；不支持不可取消交互终端。
实际listen后以真实authority装配Gate；退出先关闭请求准入/取消上游，再drain HTTP，超时明确失败。
## 约束
不读环境key、旧配置或stdin之外的秘密；不打印token/api_key/prompt/upstream body，不把key放argv。
仅numeric loopback；相同Bearer控制显式caller请求，不自动调模型、执行tool、fetch媒体或持久化原始数据。
## 业务域清单
| 文件 | 职责 |
|---|---|
| main.go | strict stdin、explicit routes与owned HTTP/Gateway生命周期 |
| main_test.go | token/no-network、invalid配置与真实启动/取消/关闭回归 |
