# Standalone WASIp1 plugin

独立 `go.mod`、仅标准库的 AEP v0.1 协议示例，不链接 Agent 或宿主实现。它是可单独加载的 `.wasm` 插件，和把整套 Go SDK 编译为 WASM 是两种接入方式。
也可用其他语言生成 WASIp1 模块，只要实现同一有界 stdio 协议；当前示例与实际验收使用 Go。

在仓库根目录准备 bundle：

```sh
mkdir -p /absolute/my-wasm-plugin
cp examples/aep-wasm/alva.toml /absolute/my-wasm-plugin/alva.toml
cd examples/aep-wasm
GOWORK=off GOOS=wasip1 GOARCH=wasm CGO_ENABLED=0 go build -mod=readonly -o /absolute/my-wasm-plugin/plugin.wasm .
```

Hub Desktop 的 AEP 设置：

```json
[{"id":"my-wasm-plugin","source_directory":"/absolute/my-wasm-plugin"}]
```

`interpreter`、`interpreter_args`、`env` 留空。先 Inspect，再明确批准，下个 Alva Turn 才由内嵌 wazero 加载，无需外部 WASM CLI、Node 或 QuickJS。
Go 应用同样通过 `extension/loader.OpenPlugin` 注册标准 Plugin；授权、实际 Tool/调用校验、Guard、完整脱敏会话证据和引用输出复用现有路径。

manifest 的 `runtime="wasm"`、`entry="plugin.wasm"` 与模块全部绑定快照 pin。格式是 WASIp1 command module，使用 `_start` 和 JSON-RPC newline frames；不是浏览器 `js/wasm` 或任意导出函数插件 ABI。
默认每个实例独立 runtime、64 MiB linear memory、64 MiB 双向 stdio 总预算、从编译开始的 10 分钟总期限；AEP 请求另有 120 秒期限和帧/回调预算。
底层执行 peer 可由宿主程序显式配置内存16–256 MiB、IO1 KiB–64 MiB、期限1ms–30min，模块最多8 MiB；loader当前使用默认值。未实现指令fuel配额，本入口用实际内存/IO/时间限额。
没有目录 preopen、宿主环境、网络或自定义 host import；只提供 WASIp1 和 AEP 上层明确授权的回调。取消会中断CPU循环、解除阻塞IO、关闭实例并等待真实结束，然后才移除快照。

`main.go` 包含 echo 工具、事件状态回调和修改模型/工具的互操作示范，也含显式 `hang`、`fail`、`probe` 验收参数。
`hang` 用来验证CPU中断；实际使用时可从示例删除这些验收分支。stderr 是诊断，stdout 仅协议。大结果由宿主保留为引用，不自动展开进模型。
示例以小型单循环处理回调，未提供另一套Go Agent SDK；事件/工具动作沿用既有AEP词汇。

```sh
go test -mod=readonly ./platform/sandbox/plugin
go test -mod=readonly ./extension/loader -run 'TestAuthorSDK|TestPortable|TestWASMPlugin'
go test -mod=readonly ./apps/hub/bootstrap -run TestPortableAEP
```

这些测试实际编译并执行本模块，验证工具成功/失败、七钩子中的模型/工具修改、取消后的实际收尾、无环境/宿主文件访问，以及Hub下一Turn装配。其他语言工具链和真实Windows运行另行验收。
