# JavaScript AEP plugin SDK

Node 20+ 的插件侧 SDK，使用现有 AEP v0.1 双向 JSON-RPC。无运行时 npm 依赖，附 TypeScript 声明；当前为仓库内源码，尚未发布 npm 包。
Go 的 Plugin、Tool、Middleware 和存储插口保持不变。普通 Go 程序不要求 Node，只有选择原生 JS 插件时需要。

从仓库根目录准备一个独立作者 bundle：

```sh
mkdir -p /absolute/my-echo
cp integration/protocol/aep/javascript/index.mjs /absolute/my-echo/alva-sdk.mjs
cp integration/protocol/aep/javascript/examples/echo.mjs /absolute/my-echo/echo.mjs
cp integration/protocol/aep/javascript/examples/alva.toml /absolute/my-echo/alva.toml
```

在 Hub Desktop 的 Session settings → AEP Plugins 配置中填写：

```json
[{"id":"my-echo","source_directory":"/absolute/my-echo","interpreter":"/absolute/path/to/node"}]
```

先 Inspect 保存 disabled pin，再明确批准；下一个 Alva Turn 才装配执行。源码/SDK/manifest 都进入同一个快照哈希，修改后须重新检查和批准。
此插件走原生子进程，具有 Node 可用的宿主访问权；能力声明只限制 AEP 宿主回调，不是 Node 的 OS 沙箱。
Go 应用可直接用 `extension/loader.ReadManifest`、`Stage`、`OpenPlugin`，显式传入自己的会话存储、Guard 和执行风险记录器。
`OpenPlugin` 返回非 nil Plugin 时，包括尚未收尾的错误，该 Plugin 接管 bundle；调用方必须保留并关闭到 `CleanupComplete()`。

`definePlugin` 接收 identity、capabilities、tools 和 handlers。`examples/echo.mjs` 给出完整工具与模型前钩子；七个事件是：

- `on_agent_start`、`on_agent_end`、`on_user_message`
- `on_llm_call_start`、`on_llm_call_end`
- `before_tool_call`、`after_tool_call`

返回 `actions.continue()`、`block(reason)`、`modifyArgs(args)`、`replaceResult(result)`、`modifyResult(result)`、`modifyMessages(messages)` 或 `modifyResponse(message)`。
宿主逐阶段验证动作，改参发生在提交 tool intent 前。消息使用 AEP 的 `type` 内容块，不能直接传 Go 内部 wire JSON。
`message(role, text)` 和 `textResult(text, isError)` 生成对应值；SDK 负责 tools/call 与 hook result 的错误字段编码。

显式请求并获批后，`host` 支持 log/notify/emitMetric，以及当前事件的 getMessages/getMetadata/countTokens/readReference/searchReference。
state 读取仅限事件存活期间；定时器和后台任务不能复用过期句柄，tools/call 本身不授予事件状态。当前宿主未提供 memory 或 request_approval 回调。
状态正文有界，大结果通过引用读取；notify/metric 是耐久事实，不代表已交付系统通知或外部遥测。

始终 await 宿主调用，长操作响应 `host.signal`。SDK 序列化事件并允许等待中的反向回调完成；16 MiB 帧、128 个排队请求/回调和 4096 次 lifetime 回调超限后失败关闭。
协议仅占用 stdout；作者诊断写 stderr。宿主取消会终止并 join 整个子进程，未知副作用不会自动重放。
`run` 借用流，退出时移除监听并等待协作处理器；自定义流的关闭仍由流的 owner 负责。

```sh
node --test integration/protocol/aep/javascript/test/runtime.test.mjs
go test -mod=readonly ./extension/loader -run 'TestAuthorSDK|TestPortable|TestWASMPlugin'
go test -mod=readonly ./apps/hub/bootstrap -run TestPortableAEP
```

这里的 Node 作者插件与 [QuickJS 脚本能力](../../../../docs/USAGE.md) 是独立的可选入口；SDK 并不冒充浏览器或 QuickJS 的 Node 兼容层。需要独立受限模块见 [WASIp1 示例](../../../../examples/aep-wasm/README.md)。
