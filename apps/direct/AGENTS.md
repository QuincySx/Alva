# L6 · Direct native application
> 直接harness桌面的耐久会话与资源owner，不经过EngineRuntime或Hub产品存储。

## 地位
独立原生应用层；消费现有harness/host、native SessionRepository与可选文件Plugin，不合并SDK或Hub的抽象。
## 逻辑
显式数据根打开私有native SQLite，Open/List/History只冷读。创建会话写版本化应用metadata；只有显式Send才装配该会话的实际harness/Controller。恢复绑定原workspace与route指纹，凭据只在本进程transport。最多16个retained binding、一个active root；UI退订不Stop。
## 约束
owner为独立alva-desktop，不读取CLI/Hub/旧Rust历史。无engine/Hub/MyGo依赖或环境凭据发现。workspace文件工具只有明确项目绑定且live时安装，Ask逐实际session/run/call审批；shell/network/MCP不因workspace默认授权。附件保存原字节和引用，8文件/40MiB、20MiB/文件，不自动送原件或处理媒体。Close先取消并joinController，再释放文件/审批/存储；失败保留owner供重试。
## 业务域清单
| 文件 | 职责 |
|---|---|
| service.go | 显式配置、session身份/route/workspace绑定、冷列表与actual harness/Controller生命周期 |
| history.go | 固定高水位metadata/显式2KiB原文、bounded conversation投影，不恢复Agent |
| export.go | 显式已选direct root的JSONL副本，只写自己的exports私有目录，随机exclusive文件名、不覆盖已有文件 |
| artifacts.go | 明确选择文件的原始保存、会话草稿refs与成功准入后消费，不自动抓媒体 |
| service_test.go | 真实SDK/SQLite重启零重放、HTTP引用/逐调用文件审批、foreign会话拒绝与Close保留DB owner |
