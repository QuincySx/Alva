# L6 · Explicit native UI diagnostics
> 自绘产品的独立、默认关闭的认证调试服务，不是 Hub facts 或 Agent runtime。

## 地位
direct/Hub UI各自注册实际视图owner；本包不依赖Hub/engine，不向SDK加入调试政策。原Rust手工注册的InspectNode/ActionRegistry在这里以版本化纯DTO和UI线程调度保留。
## 逻辑
显式numeric-loopback和独立token启动；日志ring/level、已注册产品视图树/状态、own-window PNG和声明action共用owner。每view最多一个待处理UI closure；queued取消不能迟到执行。action要求当前snapshot revision和唯一operation ID，重复ID只返回原结果，不重放。
原生窗口用TrackRendering的rendered/changed两个UI线程回调，只有真实Build之后且无迟到状态变更才可截图；MyGo CapturePage缓存旧scene时返回frame_pending并请求真实重绘，不能用另一个Tester/离屏新view冒充该窗口。HTTP/client/MCP保留此明确安全错误；Build跟踪不把原始正文/配置加入诊断DTO。
## 约束
只读默认；控件mutation与执行类action分别由进程operator显式启用，仍走原产品方法/审批/计费。注册树是实际产品视图语义树，未伪称MyGo内部layout树；可选bounds来自实际owner。截图只取注册窗口并验证完整PNG，不调用全屏截图或读取host path。关闭诊断不Stop Agent。128KiB快照、4MiB PNG、512条log、256个operation/lifetime、32个view、4个HTTP并发/16条TCP连接；partial-header连接也由Close回收。禁止旧API aliases/环境凭据发现。
## 业务域清单
| 文件 | 职责 |
|---|---|
| model.go | typed view/node/method/action DTO与有界校验 |
| view.go | UI线程one-closure owner、取消/陈旧revision与真实action结算 |
| logs.go | 可变slog Level、bounded ring与显式secret/known-field脱敏 |
| server.go | 独立认证HTTP、注册/关闭owner、at-most-once operation与窗口PNG |
| client.go | 显式独立HTTP client与官方SDK stdio MCP，不发现产品/凭据 |
| startup.go | 两个desktop共用冷启动debug flags/秘密JSON解析，不自动监听 |
| startup_test.go | 精确秘密JSON/冷启动与真实open pipe取消；两个命令另验关闭诊断后实际Send仍可执行 |
| server_test.go / view_test.go / client_test.go | 真实HTTP/stdio/取消/重复action/权限/PNG与受控UI owner反例 |
