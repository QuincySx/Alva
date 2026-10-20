# JavaScript AEP author SDK
> 显式Node插件侧协议客户端，无运行时npm依赖。
## 地位
integration/protocol/aep的可选作者SDK，不是另一个Agent runtime；Go构建不依赖Node。
## 逻辑
definePlugin固定identity/tools/七类hooks，run通过受限stdio承接JSON-RPC并允许await中的reverse callbacks。事件handle随处理器结束过期；宿主授权、持久化与进程owner仍在Go。
## 约束
不扫描配置/安装包/发现凭据；stdout仅协议。state不跨事件异步复用，作者必须响应host.signal并await副作用；原生Node不冒充OS沙箱。失败停止且不重放，借用streams不转移所有权。仓库内package未发布。
## 业务域清单
| 文件/目录 | 职责 |
|---|---|
| index.mjs | 零依赖作者API、严格帧/队列/回调预算和物理协作收尾 |
| index.d.ts | TypeScript作者声明，Node类型由消费者工具链提供 |
| package.json | 本地ESM入口、Node版本和test命令 |
| README.md | 明确bundle/运行/授权方式与限制 |
| examples/ | 完整echo工具和模型前metadata/log hook bundle源码 |
| test/ | 实际作者runtime单测；真实Go loader/Hub互操作另验收 |
