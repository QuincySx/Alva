# L7 · Headless Hub entry
> 通过共享 bootstrap.Application 提供认证 loopback HTTP/SSE。

## 地位
与 Hub desktop 并列的进程资源所有者，不是 SDK 或第二套产品逻辑。

## 逻辑
显式 data-dir/listen/模型/外部执行授权；stdin 只接 token/api_key JSON。
共享secretinput在open pipe收到取消时真实结束读取；不支持不可取消交互终端。
实际 listener authority 通过安全校验后装配，冷启动不请求模型/启动 MCP。
退出先关闭 transport 观察者、drain HTTP，再取消等待应用拥有的执行并关闭 stores。

## 约束
不读环境凭据、不打印/保存 secret、不接受公网/wildcard/DNS bind，无 React/HTML 兼容壳。
`--new-token` 单独生成 256-bit token，不创建目录/会话或监听。
所有真实功能与 desktop 消费同一个 product/native 配方。
受管Worker工作树需显式--git-executable与--git-host-approved；HTTP只传已拥有Worker和evidence/revision，隔离/采纳各自独立确认，不接客户端任意host path。

## 业务域清单
| 文件 | 职责 |
|---|---|
| main.go | 显式配置、loopback listener、stdin secrets、安全资源关闭 |
| main_test.go | token 单独生成、坏输入不创建 DB、真实 server 退出/无泄密 |
