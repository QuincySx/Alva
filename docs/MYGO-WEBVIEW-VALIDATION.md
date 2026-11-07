# MyGo v0.6.0 混合界面验证

2026-10-21。本文是依赖候选与隔离实验的检查记录，不是桌面技术栈变更 ADR。
当时生产 `go.mod` / `go.sum` 保持 MyGo v0.5.5；没有把实验页面接入 Alva 产品或修改 SDK。
后续2026-11-05的原生自绘窗口栏维护已将生产依赖固定为MyGo v0.7.4；仍是Go自绘。
本文保留v0.6.0隔离实验的原始证据，不改标为新版混合界面验收；当前窗口验收见根MIGRATION。

## 结论

MyGo 明确实现并维护自绘与系统 WebView 的混合合成。作者持续处理布局、裁剪、输入命中、
焦点、非阻塞帧呈现和性能诊断；不能仅因功能较新就推断作者忽视这些问题。
本机实验支持“自绘外壳加整块 HTML 内容区”可行，没有观察到持续的网页调度退化。

细粒度混排仍需要应用负责的机制：网页内容高度回传、嵌套滚动交接、页面数量与生命周期。
实验复现了非滚动 HTML 卡片上的滚轮不会自动滚动外层原生列表；显式转发可以补上。
这是一项组合行为缺口，不直接判定为平台渲染器缺陷。

当前建议以完整内容区域作为混合边界。若采用独立 HTML 消息卡片，须另外验收交接、
可见性和回收政策，不为每条历史消息永久创建一个 WebView。

## 固定版本与环境

- 候选：`github.com/egoist/mygo v0.6.0`，正式发布于 2026-10-21。
- 上游 commit：`081639f2a55d62a09aca742684827937dac6cb39`。
- module sum：`h1:ex6ggOqOwtrLjfzmTkcRc6hrS6wTqUSKjRp3cVZSn+U=`。
- Apple M4 Pro、48 GiB 内存、macOS 27.0、Go 1.27.2、darwin/arm64。
- Go 模块查询与 GitHub API 均确认该版本；网页缓存的 `/releases/latest` 一度仍指向旧版，
  未据此选取测试版本。
- 上游源码复制到独立临时目录，通过 Go overlay 加入实验，不修改共享 module cache 源码。
- Alva 兼容测试使用临时 `-modfile` 指向 v0.6.0，不修改生产依赖。

## 引擎和多窗口

| 平台 | HTML 引擎 | 本次查到的装配方式 |
|---|---|---|
| macOS | 系统 WebKit / WKWebView | 每页有自己的 WebView 与脚本消息控制器；网页/GPU/网络辅助进程由 WebKit 管理 |
| Windows | 系统安装的 WebView2 Runtime | Backend 首次按需创建共享 environment，后续页面创建 controller；进程复用仍服从 WebView2 的隔离策略 |
| Linux | 安装的 WebKitGTK | 按需加载，使用默认 WebContext；页面仍各自有内容和消息管理对象 |

自绘窗口由 MyGo 自己的渲染器绘制，不因是 MyGo 窗口就创建 WebView。
显示 HTML 时也不是每个窗口再分发一套浏览器安装包。共享引擎库/环境不等于共享 DOM、
JS 堆或零额外内存，亦不能保证“一页必有一个进程”或“所有页只有一个进程”。
本机实验实际看到 1/4/8 个 WebView 对应 1/4/8 个 WebContent 进程，外加 GPU/网络进程。
这是此版本和此系统下的观察，不是跨平台进程数量承诺。

MyGo 的桌面 WebView 与 Alva 工具脚本的 QuickJS-NG WASM 是不同用途和不同生命周期。

## 作者已有的针对性机制

1. WebView 位于自绘 surface 下方，自绘帧在相应区域留透明孔；后绘制的原生弹层可以盖住网页。
   不需要逐帧把 HTML 截图转成 Go 位图。
2. 原生布局下发 frame、clip、遮挡区域；位置和遮挡未改变时跳过重复 placement。
   指针按命中区域路由，焦点和 Tab 进入/离开网页有专门处理。
3. v0.6.0 的原生渲染有局部重绘、增量布局、非阻塞帧呈现与不可见窗口停绘。
   原生停绘不能当作任意网页 JS 已停止的证明；网页定时器仍需界面宿主管理。
4. `MYGO_FRAME_STATS` 提供 build/layout/paint/present 的 CPU 耗时诊断；上游另有原生输入到
   drawable 实际呈现的 `internal/inputlatency`。后者不是混合网页已经通过相同验收的证明。
5. IPC channel 有确认与背压；每页有独立上下文及身份，页面关闭/导航可解除其通信。
   产品执行 owner 不应绑定到页面观察上下文。

参考：[混合布局](https://mygo.egoist.dev/docs/ui/web-view)、
[渲染与诊断](https://mygo.egoist.dev/docs/ui/rendering)、
[v0.6.0 架构源码](https://raw.githubusercontent.com/egoist/mygo/v0.6.0/docs/architecture.md)、
[版本说明](https://github.com/egoist/mygo/releases/tag/v0.6.0)、
[WebView2 进程模型](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-model)。

## 已完成检查

| 检查 | 结果 |
|---|---|
| v0.6.0 WebView / frame stats 单元用例 | 19 个测试及子测试通过 |
| v0.6.0 真 GUI：点击、原生弹层遮挡、双向 Tab、Edit menu | 3 个通过 |
| v0.6.0 真 GUI：窗口 drop target 路由 | 1 个按上游平台条件跳过，不能计作通过 |
| Alva 候选依赖：共享 chat/modules、direct desktop、Hub desktop | 4 个包，62 个测试通过；这些是现有自动化测试，不等于完整产品 GUI 验收 |
| 隔离窗口实验 | HTML 单窗口、混合单窗口、混合四窗口、单窗口八张独立卡片均完成 |

没有调用模型、读取账户或发送付费请求。Windows/Linux 只审阅源码和文档，没有实机验收。

## 实验方法与测量

单窗口和四窗口实验使用 500 行简单 HTML 消息，持续追加文本、改变图形并程序化滚动；
四窗口场景仅前台窗口运行负载，另外三个窗口保留页面。卡片场景是八个各有五行消息的
独立网页，多个可见页各自更新。内容、同时可见页数和控制器数不同，不将表格当作严格的
同负载性能排名，也不计算“每个 WebView 固定消耗多少内存”。所有页面均未做列表虚拟化。

活跃段约 6 秒；缩放段每 40 ms 请求一次尺寸变化，共 100 次，再复位。四个场景的网页均
收到 101 次 resize，最终尺寸符合预期。原生滚动/裁剪实验与缩放实验分别执行，避免把
采样页面主动滚出视口后暂停 rAF 的长间隔误算成可见画面掉帧。

| 场景 | 活跃 rAF P95 间隔 | 缩放 rAF P95 间隔 | 原生帧 CPU P95 | 活跃段物理 footprint 快照 |
|---|---:|---:|---:|---:|
| 整窗 HTML | 15 ms | 15 ms | 不适用 | 462 MiB |
| 自绘外壳 + 一个 HTML 区域 | 15 ms | 16 ms | 2.0 ms | 495 MiB |
| 四个混合窗口 | 15 ms | 15 ms | 2.5 ms | 649 MiB |
| 原生滚动区 + 八个 HTML 卡片 | 15 ms | 15 ms | 2.0 ms | 1261 MiB |

注意：

- rAF 是网页回调调度间隔，不是实际显示帧率，也不是输入到屏幕的延迟；不能据此承诺 60/120 FPS。
- 四窗口缩放期间出现一次 47 ms 的 rAF 间隔，原生帧 CPU 最大 43.8 ms；不能宣称完全没有长帧。
- footprint 包括 Go 测试宿主与归属匹配的 WebKit 内容/GPU/网络进程，不是仅 Go heap、RSS 或发行版体积。
  测量沿用上游 idlemem 的 macOS 归属方法；它是快照，非整个运行的峰值或长期稳态。
  多个持续更新的独立网页开销明显，必须有数量上限和回收政策。
- 关闭实验窗口后 footprint 有下降；短期下降不能证明长期无泄漏，系统也可保留辅助进程/缓存。
- CPU 计数按 Mach timebase 转成纳秒；不把 ARM 平台原始 Mach tick 误标为纳秒。
- 实验包含简单 CSS/文本，没有真实 Markdown、大媒体、模型请求、完整 Inspector 或插件沙箱。

## 可复现的滚动交接边界与解决实验

实验向 AppKit 命中的真实 responder 投递合成的连续滚轮事件（含 began/changed/ended）。
不是 JS `dispatchEvent`，也不等同于人手触控板、惯性滚动或系统事件队列的完整验收。

1. 指针位于网页：命中 `MyGoWebView`，网页 scrollTop 从 0 变成 228。
2. 指针位于原生消息行：命中 `MyGoSurfaceView`，原生列表滚动 12 px。
3. 指针位于禁止自身滚动的网页：外层原生列表位移仍为 0。CSS 禁止内部滚动不自动建立
   网页与原生列表的父子滚动链。
4. 对所有卡片安装非 passive wheel listener，按 rAF 合并 delta，再经绑定实际调用页的
   Go 方法更新同一原生 ScrollState：相同 10 个滚动变化事件使原生列表移动 120 px。
5. ResizeObserver 经显式 bridge 回传卡片高度，在最后一轮约两秒内提交 37 次更新，
   宽高/点击检查通过。该 bridge 是实验代码，不是 MyGo 自动的内容自适应能力。

这证明可以用现有接口补上转发，不证明惯性/触摸/横向滚动/选区自动滚动/缩放/焦点的全部
交接已完成。生产方案还需要边界判断、一次性消费、delta 合并、取消和可见性政策。
另一种成本较低的方案，是让整个连续消息列表及其动态卡片在同一个 HTML 区域中管理滚动。

## 证据与复现入口

本次原始 JSONL、逐场景 result/summary、页面截图、Go overlay、实验源文件与运行脚本位于：

`/tmp/alva-mygo-validation-QJJWJ1/`

该目录是本机临时证据，不作为仓库生产依赖；临时目录丢失后不能从本文数字反推原始测量。
运行器复制固定 module 源码后执行实验，未修改共享缓存。临时 `alva-candidate.mod/sum` 用于
上述 62 个项目测试。最终依赖哈希校验另存于 `final-integrity.json`。

```sh
python3 /tmp/alva-mygo-validation-QJJWJ1/run_probe.py
go test -mod=readonly -modfile=/tmp/alva-mygo-validation-QJJWJ1/alva-candidate.mod -tags desktop ./apps/ui/... ./apps/direct/desktop ./apps/hub/desktop
```

`run_probe.py` 会打开并回收自建 GUI 测试窗口，应在 macOS 桌面会话中显式运行。
它没有接入普通 CI，也没有替换产品 UI。后续是否升级生产依赖或变更自绘决策，应独立验收。
