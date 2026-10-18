# Registrar 贡献边界

当前契约在 [sdk/agent/core](../sdk/agent/core/AGENTS.md)。
公开方法由 `architecture/testdata/public-surface/agent-core.txt` 显式记录。

## 当前契约

Plugin 使用 Name 与可失败的 Register；需要完整装配结果时实现独立 Finalizer。
Registrar 的 Tool、Middleware、SystemPrompt、Command、ContextSystem、ContextSource 贡献
进入本次装配；能力使用 BusWriter 配合类型化 Provide 注册。
Bus、BusWriter、Workspace 是既有装配访问入口，不新增产品状态 getter。

Register 返回后其 Registrar 同步封闭，Seal 后注册集不可写；Finalize 使用只读 LateContext。
重复工具/中间件/命令与无效贡献失败关闭。Builder 冻结当前 recipe，callback 修改 Builder
仅影响下一次 Build，不得改写正在组装的 Agent。

## 演进规则

新增方法须有真实消费者与独立职责。优先使用已有贡献或构造传参，不把 Registrar 扩成运行期
服务容器，不为某个产品增加设置/存储/权限/观测读取面。既有读取方法不构成继续扩张的理由。
公开 API 增删改须同批显式更新快照，并说明收益、损失及外部源码兼容影响。

在 Go module 根目录执行 `go test -mod=readonly ./architecture/... ./sdk/agent/core`。
公开快照和依赖闸是执行证据，不宣称存在另一套已删除的脚本/lint。
