# 首页与消息基础能力 · 2026-10-02

用户要求本轮完成基础可用性，不做美化。以下验收独立于原生视觉/性能预算，不以历史测试数字代替实际功能。

| 工作单元 | 核实的缺口 | 完成条件 |
|---|---|---|
| 首页 | 目录选定后空白；没有中央最近会话/起步提示 | 双模式入口保留，最近会话可打开，起步提示只填草稿、不自动调用模型 |
| 消息类型 | 终态 assistant 错误只显示徽标；用户正文被当 Markdown；工具和系统文案混杂 | 用户原文、assistant Markdown、工具参数/结果、审批、失败/中断/fallback 分别呈现，原因/动作不丢 |
| 阅读 | 有虚拟窗口但无记录搜索/整体复制；中文 IME Enter 会发送 | 搜索/复制覆盖已载入消息及工具预览（含未挂载条目，不冒充分页全文），可跳转；上翻不被流拉回、回底恢复，IME 不发送 |
| Markdown | 表格/引用/安全链接缺失；复制失败无反馈 | 代码、列表、引用、表格和安全链接可用，HTML/脚本惰性，复制失败可见 |
| 附件 | + 按钮没有 callback；用户要求引用优先而非直接喂模型 | 原字节上传/拖放，20 MiB/文件、40 MiB/8 文件每 Turn；原件/BLOB/处理清单持久化，仅引用进入 prompt，历史预览可读；图片/视频本地预览不冒充模型分析 |
| 消息模块 | 无实际消费者的扩展口 | Hub 注册可信 renderer，版本化纯数据 envelope；表格、图表、图示至少三个真实 renderer；助手 fenced block 和工具完整单页均可消费，未知/非法/失败保留原文 |

## 边界

- 附件属于 Turn，只传用户选择的字节，不接收服务端宿主路径。私有 BLOB 为权威副本，session cwd 中 `.alva/sessions/<id>/attachments/` 新建唯一副本供 Agent 按需搜索/读取；不覆盖项目原有文件。支持文本/图片/视频/其他二进制上传，HTTP 专用 body cap 64 MiB，不自动把媒体/正文送给模型。
- 上传清单含原始哈希/大小/签名类型及明确的未压缩/转码/抽帧/分析说明；delivery_scope 仅本次 upload_prompt，不假称后续工具没有读过。后续媒体操作独立保存派生清单/receipt，不改原始清单。Hub Desktop/server ALVA 的通用大结果引用、受管 MCP 单入口渐进目录和自有 native session 检索已实现，app-core 递归子树同样重绑自有策略，不读父历史/恢复 root host 工具。
- 当前产品 root ALVA 的 hub_context 可按 Turn/Attempt/branch/engine/时间检索此 HubSession 已存的跨 engine 可见历史（含失败原因），显式读取文字附件；不恢复未保存的 CLI 内部记录或其他产品会话。hub_media 提供受审批的图压缩、指定/均匀抽帧、静音视频转码、处理清单及选中派生图的单请求投递；原件不覆盖、未选中媒体不发送、receipt 区分准备/请求尝试/响应/未知接受状态。只覆盖受支持格式与声明 images/vision 的 root route，不冒充媒体语义分析、外部 engine 循环策略或子 Agent 媒体授权，详见 docs/ALVA-HUB-CONTEXT-RETRIEVAL.md。
- 消息模块是 Hub 呈现扩展，不是 Tool trait 的 UI 自述。生产者提供数据，Hub 决定展示；不运行消息中的 HTML/JS，不加载远端 renderer，不新增 kernel/agent-core/RuntimeEvent 词汇。
- 模块协议为 `alva.display` version 1 的 JSON envelope；持久化来源仍为既有 assistant 文本/工具全文，不维护第二份消息日志。
- 真实账户、图像生成服务、付费模型、远程第三方插件安装不在本轮自动执行范围。原生 WKWebView 完整验收仍按已记录的 AX 暂缓，不把 Chromium 等同于原生。
- 派生媒体专门只读 Tauri/HTTP 端点与消息 UI 已接通，索引/预览/处理清单/投递回执分开惰性读取，分页/失败重试/迟到回复/Blob 回收/焦点与重启有回归；不会因为查看而处理或投递。真实 provider 媒体验收、原生 WebView 完整验收、音频转录、场景检测、schema JSON-path 提取和精确多模态 token 预算仍待实施；本地 fixtures 不是这些验收的替代。

## 验证

逐单元做确定性反例/实现；完成后一次全前端、相关 Rust/HTTP、TypeScript/Vite 构建、架构闸门与真实 Chrome 滚动回归。证据填写到 EXECUTION.md 和正式 ACCEPTANCE；未通过项不标完成。
