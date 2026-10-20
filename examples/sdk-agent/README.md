# 用公开 SDK 嵌入 Agent

这个目录有独立 `go.mod`，只导入 Alva 的公开 SDK 包。应用选择模型、会话和服务；Plugin 注册一个 Tool 和一个 BeforeModel hook。示例模型是本地确定性 fixture，无需账号或网络。

在本目录运行：

```sh
GOWORK=off go run -mod=readonly . run
GOWORK=off go run -mod=readonly . cancel
```

`run` 输出 `Hello, Alva`，经过两次模型调用和一次工具调用；输出取自已提交消息。`cancel` 在工具真正进入后取消，等待 Run 返回并确认工具以 cancelled 结算。

同一源码运行在 JS/WASM 与 WASIp1：

```sh
GOWORK=off GOOS=js GOARCH=wasm go build -mod=readonly -o /tmp/alva-sdk-js.wasm .
env -i "$(node -p process.execPath)" "$(go env GOROOT)/lib/wasm/wasm_exec_node.js" /tmp/alva-sdk-js.wasm run
env -i "$(node -p process.execPath)" "$(go env GOROOT)/lib/wasm/wasm_exec_node.js" /tmp/alva-sdk-js.wasm cancel

GOWORK=off GOOS=wasip1 GOARCH=wasm go build -mod=readonly -o /tmp/alva-sdk-wasi.wasm .
# 从仓库根目录构建与 go.mod 固定版本一致的 runner：
(cd ../.. && go build -mod=readonly -o /tmp/alva-wazero github.com/tetratelabs/wazero/cmd/wazero)
/tmp/alva-wazero run /tmp/alva-sdk-wasi.wasm run
/tmp/alva-wazero run /tmp/alva-sdk-wasi.wasm cancel
```

`../..` 的 replace 明确指向当前 checkout，`v0.0.0` 是本地占位版本。复制到自己的项目时，移除 replace，再用 `go get github.com/QuincySx/Alva@<明确提交或已发布版本>` 固定依赖。示例不声称当前提交已发布为可下载的版本。

换模型实现 `abi.LanguageModel`；换会话实现 `abi.PersistentSession`，用 `sdk/kernel/abi/sessiontest.Run` 验收。换应用服务只需实现 `Greeter` 并在装配时供应。已知依赖直接构造注入，无需增加 Bus 能力或内核 setter。
