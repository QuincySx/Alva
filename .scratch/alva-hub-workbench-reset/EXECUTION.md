# EXECUTION · 逐卡执行记录

## 2026-10-03 继续 · 当前 Agent 的只读 MCP 日志 Inspector

- 新增产品只读 journal API/正式 Inspector 入口；Tauri/HTTP POST 复用共享 HubInspectorService 与同一 native reader。native metadata + 版本 1 typed 父链接验证到本 HubSession 的 ALVA branches，256 层/循环拒绝；query 路径不恢复会话/创建未知 ID/Agent/MCP/模型/append/重放，不给主 AI 增加父/其他会话日志权限。
- list 只扫 128 metadata/回 20 小索引，正文保留 literal 200 字符、128 records/64 MiB scan、20 hits/100-byte previews、2 KiB UTF-8 read/16 KiB response、blob hash-size/type-version/typed 媒体排除。正文损坏仍能列索引，读取则失败；定时 Inspector DTO 仅 MCP 小引用。整个 Inspector 的全量恢复/RunRecord 构建仍未分页/性能验收，不夸大本条 metadata API。
- UI 当前页替换/固定 through/空页继续，scope/导航/retry 绑定结果防迟到及旧页闪现；关键词原样、不执行 HTML、通用 alert/重试、索引来源焦点与 Inspector Back/Esc。明示脱敏/typed 媒体排除/unknown acceptance，不触发工作；browser-only 无服务明确失败。正式 child Inspector 入口测试确认只查询当前 child。
- **589 passed：53 adapter、261 Desktop lib / 4 精确真实账户 filtered、4 bin、16 HTTP、245 UI / 26 files、10 Chrome**；14 条新增，6 条既有 Chrome 为 100/1000/10000 history/DOM/stream anchor/窗口外搜索。真实私有 SQLite/typed child/blob/HTTP 覆盖重启、损坏、未知 schema/metadata-only links、越权、限额及不追加/启动/重放；Chrome EN/ZH×1440/720 的日志 HTTP 拦截 fixture 不冒充真实 MCP/provider/WKWebView。
- production build/all-target check（29 既有 warnings）/diff/public snapshots/依赖/bus/wasm32/full host link 通过，wasip2 缺 target 明确 skipped。首轮 Result/import/Alert 夹具错误、StrictMode 双 metadata 请求、Axum unknown JSON 422 已按真实行为纠正，未放宽生产闸门；代码复查补充 cursor 身份防闪现与 unknown link version 拒绝。Chrome DevTools 工具不可用，使用仓库现有 Playwright/已装 Chrome，不安装依赖。
- coding/git 规范同步三级文档，仅本批本地提交、不 push；用户 CONTEXT.md/无关目录保留。仓库只有 sample Git hooks，已提示可另行安装 guardrails-agent-plugin，本次不擅自安装。内核公开表面未变，新增业务仍在 adapter/产品层，这是扩展边界有效的证据而非内核完美证明；跨层装配复杂度/完整 Inspector 大规模内存/真实 provider 与 native 验收仍待继续。

## 2026-10-03 继续 · 递归子/孙 MCP 的自有 worker 与日志

- 受管产品配方初始化发布窄 McpSpawnPolicy；app spawn 完成直接父实际工具选择后，只有获准 mcp_runtime + 共享 SecurityGuard 才重绑。实际 native child authority 校验先于 host IO，沿用本轮批准 server/cwd snapshot，不发现 ambient 配置；每层独立 gateway/recorder/cache/worker，不借父日志或恢复 root 工具。
- 启动受父取消且从同一 child timeout 扣时；超时仅取消自身，失败不回退父 gateway，不调用 child provider。child 结束回收自身连接，父可继续调用。常规 HITL/锁/工具 identity/深度仍保留，空/撤销授权不启动 MCP。既有 middleware 对虚构危险名称可先审批再报 Tool not found；即使批准也不恢复权限，本批不改 SDK 执行顺序。
- **866 passed：446 app-core、51 adapter、90 MCP protocol、260 Desktop lib / 4 精确真实账号 filtered、4 bin、15 HTTP**，含 7 新回归，10 既有 doc cases ignored。真实本地 stdio + AllowOnce/RejectOnce + 模型替身验证父/子/孙 3 PID、批准 cwd/清环境、600 kB 自有结果→引用→文字搜索、交叉日志拒绝/重启/结束回收/父继续执行；grant/guard/错误 gateway、启动 timeout/父取消、native authority 缺失 fail-closed。
- all-target check 0 errors/29 既有 warnings；diff/new-file rustfmt/public snapshots/依赖/bus（max external surface 2）/wasm32/full host link 通过，wasip2 缺 target 明确 skipped。初始 fixture 的 grandchild 标记包含 child 子串、Mac cwd canonical 前缀不同，以及 MCP error 同样投影引用，均修正断言位置/标记，不放宽生产权限、日志或模型引用闸门。
- domain-modeling 更新 ADR-0011 与三级文档的真实实现范围；coding 不改 kernel/agent-core/CI allowlist，git 保留用户 CONTEXT.md/无关目录，只提交本批，不 push。未安装依赖、调用用户 MCP/真实账户/计费；未续验 UI/Chrome/native/性能。递归 MCP 重绑已补齐，专门日志 UI、外部 engine 循环与真实 provider 验收仍不宣称完成；JSON-path 不恢复为待做项，下文为历史。

## 2026-10-03 MCP 澄清 · owned I/O worker 与完整日志文本检索

- 用户明确不要 JSON-path/字段提取；改为 owned 后台 I/O worker，不新起子 LLM/Hub WorkerSession。四 gateway 动作保留完整 input/output 后只给主 AI 短引用；握手、目录分页、调用/错误、空闲通知、客户端回复和 stderr 持续记录。沿用单 gateway/HITL/cwd/环境/协议预算，不动 kernel/agent-core，也不影响 legacy 未启用 caller。
- 同一 native SessionRepository 的 SQLite/内容寻址 zstd 文件为权威；完整脱敏 frame 同存内存，复用现有安全脱敏规则。有效 JSON 保存完整语义而非原始空白；stderr 保存文本/原字节块。queue/response 各 1 项，stderr 4 KiB/frame 1 MiB，recorder 每装配最多 4096 条/64 MiB 序列化（不是 RSS 硬上限，idle 跨 Turn 不重置）；预算/落盘失败锁存并关连接，gateway 自身记日志失败也关受管连接，不静默淘汰或自动重放。取消/超大/EOF 前缀不冒充完整输入或远端未执行，owned child/PID 闸防迟到 reader 杀 recycled 进程/发布假 Connected。
- 主 AI 用 context_retrieval 的 kind=mcp/read/search_record 自行有界文字搜索；精确 namespace/type/version、实际自有 session、blob hash-size 验证，typed 媒体/其他 extension/internal reasoning 不暴露。日志不成为 conversation，引用重复投影/压缩不套链，重启可读。Hub-owned Worker 独立 factory 有自己的 journal；复制 root gateway 到递归 child 明确拒绝，独立 child MCP worker 重绑尚未实现，不能授权父日志。
- **859 passed：443 app-core、47 adapter、90 MCP protocol、260 Desktop lib / 4 精确真实账号 filtered、4 bin、15 HTTP**，含 11 新测试，10 既有 doc cases ignored。真实自编 stdio/AllowOnce/模型替身验证 600 kB 完整结果→引用→文字搜索、空闲 stderr/通知/ping、取消/错误/日志失败关闭、完整内存与文件一致、预算锁存、Unicode/重启/损坏/越权。all-target check 0 errors/29 既有 warnings；new-file rustfmt/diff/public snapshots/依赖/bus/wasm32/full host link 闸通过，wasip2 缺 target 显式 skipped。
- domain-modeling 记录已接受 ADR-0011 的澄清，JSON-path 已撤销而非待做；coding 控制在协议机制/harness worker/产品日志，git 仅提交本批、保留用户 CONTEXT.md/无关文件。无依赖安装、真实账户/计费、用户 MCP 或 push。未续验 UI/Chrome/native/性能、不冒充专门 MCP 日志 UI 已完成；外部 engine 循环、递归 child MCP 重绑和真实 provider 验收仍未覆盖。下文历史 pending 不覆盖本次明确决策。

## 2026-10-03 继续 · 消息内派生媒体/处理清单/投递回执

- 历史图片/视频消息接“派生媒体”只读入口，打开只取 4 项索引、点击才读核验私有 JPEG/MP4 BLOB；原始清单不改，处理/帧策略/实际时间/理由/范围/损失明示。单份 metadata 16 KiB 在 SQL 分配前拒绝，字符串值脱敏、React 惰性展示；不读取工作区副本、执行 decoder/模型或重放选择。
- 共享 HubStore 三个 reader/DTO 经 Tauri 注册命令及 HTTP GET 同构传输，按产品→来源→产物归属验证；receipt 5 项固定 row 高水位/状态可前进/显式刷新。准备、尝试、响应、未知接受真实区分，空页不冒充从未尝试、响应不冒充理解。翻页替换 DOM、失败重试、迟到结果/会话切换隔离、Blob 回收、无 autoplay、Esc/焦点恢复。
- **528 passed：260 Desktop lib / 4 精确真实账号 filtered、4 bin、15 HTTP、239 UI / 25 files、10 Chrome**；含 13 新测试。reader 最后 cursor guard 后另跑其 2 项窄回归，不重复计数。真实私有 Store/HTTP 临时 DB 核验重启、工作区副本变化、损坏/越权/预算；4 Chrome EN/ZH×1440/720 fixture 使用实际合成 JPEG/HTTP 拦截，另续验 6 条 100/1000/10000 消息滚动/流 anchor/窗口外搜索。不是 WKWebView/provider 验收。
- production build/all-target check（29 既有 warnings）/rustfmt/diff/public snapshots/依赖/bus/wasm32/full host-link 全通过；wasip2 缺 target 显式 skipped。首轮 HTTP 缺 qualified Deserialize、测试 helper 误把 Axum 400 文本当 JSON 均修正，未放宽生产解析/归属/损坏闸门。仅复用缓存 test-only rusqlite/sha2，无安装/付费/真实账号/用户 MCP/SDK 改动或 push；依 coding/git 技能同步三层文档、保留用户 CONTEXT.md/无关目录。
- 专门派生预览 API/UI 已补上；原生 UI/性能、真实 provider 媒体验收、音频转录/场景检测、schema JSON-path/精确多模态预算和外部 engine 循环政策继续待实施。下文为历史，不将其旧 pending/测试数当作当前结果。

## 2026-10-03 继续 · 产品历史检索与真实媒体处理/选中投递

- Desktop/server 在 Turn 认领后，以现有 HubStore/实际 native root 装配 SessionPluginFactory；hub_context 检索当前 HubSession 已保存的跨 branch/engine 可见 user/assistant/tool/error，按 Turn/Attempt/engine/status 分组，含失败原因/时间/ID 筛选。五来源固定高水位、跨 message 片段/tool 块、源 SHA、有界 UTF-8 读取；128 逻辑来源/64 MiB scan、3 命中、512-byte excerpt、4 KiB response。文件列表只读索引，文字附件须显式查询原 BLOB；无全局/其他产品会话/Worker/父 scope、hidden reasoning 或外部 CLI 未落盘全文。
- schema 24 保存独立派生媒体与 receipt，原附件清单/字节不改。hub_media 接真实 SecurityGuard HITL；固定安装的 ffmpeg/ffprobe、清环境、限定协议/demuxer、no-follow owner-only 只读 FD 快照、2 job 并发/时间/输出预算、取消 kill/reap。图/帧 512px/256 KiB JPEG；1–4 均匀或指定帧保存请求/实际 pts_time/理由；静音视频前最多 60s/720px/5fps/8 MiB，明确 codec/帧率/范围/元数据/颜色损失。Mac 512 MiB RSS 每 50ms sampled 不是硬沙箱；Linux 2 GiB AS 仅实现未在本机验收。
- 只有显式选择的 1–2 派生 JPEG，在声明 images/vision 的 route 的一次请求副本中进入模型；原视频/音频/其他帧不发送，conversation 仍为引用。provenance 带源/产物 SHA、版本/尺寸/真实帧点与实际 model/backend；receipt 区分 prepared/attempted/response/unknown，正常终态及重启都不重放，响应不冒充视觉理解。字节上限不冒充 provider 精确多模态 token 上限。
- **475 passed：258 Desktop lib / 4 精确真实账号 filtered、4 bin、43 adapter、14 HTTP、156 llm-wire**，含 12 新回归；一个既有 llm-wire 文档例子 ignored。完整串行 Desktop/真实 catalog+AllowOnce/模型替身验证零自动图→只选中图/provenance→成功/错误 receipts→下一次零重放；本机 ffmpeg 9.0.2 合成图/视频实际解码验证时间点/codec/损失/原件/越权/取消/缺工具/损坏/23→24 迁移与重启。无真实账户/计费/用户媒体调用。
- all-target check（29 既有 warnings）、new-file rustfmt/diff check/public snapshots/依赖/bus/wasm32-unknown-unknown/full host link 全通过；wasip2 未安装显式 skipped。首轮 compile/fixture/Mac RLIMIT_AS EINVAL 修正按实际 API/pts_time/RSS 能力处理，不放宽 SDK/生产迁移闸门。新增代码/公开应用桥同步三层文档，依据 domain-modeling 更新已接受 ADR 的实际范围；未改 kernel/agent-core/CI allowlist，缓存依赖复用，无安装、用户 MCP 或 push。
- 当前未续验 UI/Chrome/原生 AX/视觉/性能；派生媒体专门预览 API/UI、真实 provider 媒体验收、音频转录/场景检测、schema JSON-path、精确多模态预算及外部 engine 工具循环策略仍待实施。用户 CONTEXT.md/不相关目录保持。下列记录是历史，不迁移为本批验收。

## 2026-10-03 继续 · 递归子 Agent 的引用策略和直接父权限

- app-core 新增 optional SpawnContextPolicy（初始化注册、bus lint max surface 2），产品 Plugin 按 repository 创建的实际 child ID 重绑自有 reader/首个 assembly hook/middleware/短提示；不依赖 engine、不给 kernel/agent-core 加政策。child/孙 scope ID 与 native session ID 不混用；空 host whitelist 仍可读自己的有界记录，不授予父历史/文件/Shell/MCP。
- assembly 先投影后传给通信 hook，重算 token metadata；来源失败不回灌 raw entries，锁存 child-local 错误，在 provider 前停止。自有记录/error/typed 父子 link 保留，可重启检索。通信 hook 是可信装配代码，仍可访问原 ContextHandle，不冒充它的沙箱；未新增 child 自动压缩器。
- 修复递归从 root catalog 拿回直接父被移除工具的漏洞：schema/模板 inherit-all/实际 grant 都受直接父工具集限制。默认模型也继承实际父模型而非 root scope 缓存模型；显式 override/模板优先级、security/HITL、锁、递归深度/取消/预算不变。
- 六条新增确定性回归；最终 **344 app-core（328 lib + 16 security/lock/workspace/swap/wiring integration）、43 adapter、246 Desktop lib / 4 精确真实账号 filtered、4 bin、14 HTTP = 651 tests passed**。父→子→孙真实 BaseAgent fixture 覆盖大日志/ToolContent 图片引用、通信 assemble 顺序、自有检索、父/其他 child 隔离、原始身份及 reopen；空授权/来源失效/递归工具 schema与执行/实际模型继承均验证。首轮新测试出现错误 event 变体名与 text_content 不含 ToolResult 的 fixture 断言错误，按真实 AgentEnd/ToolResult 修正，未改变生产闸门或弱化隔离断言。
- Desktop/server all-target check、public snapshots、依赖/bus/wasm32-unknown-unknown/host-link 闸 pass；wasip2 缺 target 显式 skipped。依据 coding/domain-modeling/git 同步三层文档及既有 ADR 的实际范围；没有 SDK/kernel/UI 修改，不重报历史 Chrome/AX/性能通过，没有真实账号/计费/外部 MCP/新增依赖或 push。用户 CONTEXT.md 和无关目录保留。
- 仍待实施：跨 branch/engine 的 Hub 产品历史分组/检索，真实媒体压缩/转码/抽帧/按需选择投递与 receipts，以及 schema JSON-path 提取。当前不会把媒体引用宣称为模型已看过视频。

## 2026-10-03 继续 · ALVA 通用结果引用、按需 MCP 与耐久历史检索

- 同一 native SessionRepository 作为 factory/adapter/Inspector/retrieval authority；Plugin/Middleware 只修改模型请求副本，>4 KiB 普通结果、ToolContent 图片、所有受管 MCP 调用结果返回记录引用，原始脱敏事件/记录/error/id 不改。投影在既有自动压缩之前；重复 call ID 不串源，来源 hash/content 校验，compaction 持久化引用不套成新引用链。blob 解压 64 MiB/压缩输入 65 MiB/hash-size fail-closed。
- 受管 ALVA MCP 改为一个 mcp_runtime + 4 KiB/32-server 短提示；search 默认 5/最多 20、有界说明/分页，describe 一个 schema、call 精确已连接身份/object/input ≤256 KiB。仍走原 HITL（含查询）与真实 stdio 清环境/取消/进程回收，旧 direct-tool caller 兼容。host 目录扩大到 8192 tools/128 pages，并加累计 32 MiB，单 frame 1 MiB 不变；真实 5000-tool/32-MiB 拒绝 fixture 通过。
- context_retrieval 的 search_history/read/search_record 固定当前 native ALVA session + execution context，literal keyword/time/type/high-water cursor；128 metadata/64 MiB 来源扫描、20 hits/100-byte preview、2 KiB UTF-8 read/16 KiB wire。排除 reasoning/内部 provider 事件与 typed MCP media/blob/data-URI；原结果仍可由 UI/Inspector 完整查看，不执行返回 HTML/脚本。既有权限和暴露范围不扩大。
- **40 adapter、325 app-core lib、86 MCP protocol、246 Desktop lib/4 精确真实账号 filtered、4 bin、14 HTTP** 通过；端到端真实 factory + stdio + 正常 AllowOnce + 自动压缩 + 模型历史 Tool 验证 600 kB 原结果只作为引用进入模型。初次并行 Desktop 回归 245 passed/1 既有 reopen lock failure，完整串行 246 pass；未擅自改锁机制、未宣称定位或修好了间歇性冲突。all-target check、diff/new-file rustfmt、public snapshot、依赖/bus/wasm32-unknown-unknown/host-link 闸 pass，wasip2 缺 target 显式 skipped。
- 依据 coding/domain-modeling/git 规范同步三层文档和 ADR-0011 的实际范围，保留用户 CONTEXT.md/不相关目录，不 push。无 UI 修改，233 UI/6 Chrome 是上一批证据而非重验；无真实账户/计费/外部 MCP/新 npm 包，原生 AX/视觉/性能继续暂缓。
- **仍待实施**：SDK agent tool 独立递归子装配继承（父 retrieval 被复制也因 session binding 拒绝）、跨 branch/engine 的 Hub 产品历史分组/检索、实际媒体压缩/转码/抽帧/选择投递及 receipts、专用 schema JSON-path 提取。Hub-owned ALVA Worker 走共同 factory 已覆盖；不将 native branch 检索误称为全产品历史，也不将引用返回误称为模型已理解图片/视频。

## 2026-10-03 功能补全 · 首页/消息基础与引用优先上传

- 首页真实最近会话与选择目录后的起步草稿；用户原文、助手安全 Markdown/代码/表格/引用/原生外链、失败原因与 partial 文本、工具/审批/fallback 不混淆。显式复制成功/失败反馈；IME Enter/Shift Enter 不发送，busy Stop 不被失效下一轮 route 禁掉。搜索/复制覆盖已载入消息及工具预览（含窗口外条目），明确不是分页全文或模型历史检索。
- 可信 message-module registry/provider 有表格、正负柱状/折线图、节点图真实 renderer；助手 alva-display fence 与完整单页 ToolOutput 均消费，未知/非法/错误模块保留原文；没有远程 JS/HTML 安装器。协议记录 docs/ALVA-HUB-MESSAGE-MODULES.md，不改 RuntimeEvent/kernel。
- 用户补充引用优先后，schema 23 原字节 BLOB + no-follow/create-new 会话目录副本替代直接正文喂入。单文件 20 MiB、每 Turn 40 MiB/8 文件；真实 HTTP 64 MiB 专用 body cap 测试成功上传恰好 20 MiB 二进制，越权预览 404，signature-only 不伪造解码。新会话已被 live event 认领后失败仍恢复附件-only 草稿。媒体 Blob 本地预览关闭回收；清单原始哈希/大小、明确未额外压缩/转码/抽帧/分析与 upload_prompt 投递范围，保存清单不是模型成功接收回执。
- 最终 **233 UI / 24 files、246 Desktop lib / 4 精确真实账号 filtered、4 bin、14 HTTP、6 Chrome 滚动回归**（100/1000/10000 初始底部与 resize、混合行有界 DOM、流式阅读锚点、窗口外搜索）通过。TypeScript/Vite build、Desktop/server all-target check、diff/rustfmt、新公开符号快照、依赖/bus/wasm32-unknown-unknown/host link 闸通过；wasm32-wasip2 缺 target 明确跳过。现有 Rust unused warnings/React act/Collapsible 与 Vite >500 kB chunk warning 未改验收阈值，不冒充性能绿标。
- 依据 coding/domain-modeling skills 将验收与术语/ADR-0011 同步；新增文件/接口同步分形三层文档。base64/sha2 仅显式复用已缓存/锁定 Rust 依赖，offline 构建，无新增 npm 安装。未调用真实账户/付费模型/用户 MCP 服务，原生 AX/视觉/性能继续按用户授权暂缓；保留 CONTEXT.md 和不相关目录。
- **不宣称所有基础能力已完**：通用工具大结果策略、MCP 单入口渐进加载、AI 历史检索、实际媒体压缩/抽帧/模型投递仍待实现；已接受方向、实现顺序和反例预算见 docs/ALVA-HUB-CONTEXT-RETRIEVAL.md。本批附件不自动读取原件，文件副本在共享 cwd 不宣称 engine 间 OS 隔离，失败不隐式删除用户目录。

## 2026-10-02 修复 · H09/H15 ALVA 启动期 Stop 与真实终态

- 每轮 ALVA 使用独立取消 token；尚无 BaseAgent 时也接受 cancel，factory/MCP 组装与取消/consumer 关闭竞争，丢弃 pending build 后不再调用模型。运行期 stream 被丢弃也会取消永远 pending 的模型，drain/flush 后释放 running；同一会话 resume 使用新 token，正常 SessionStarted 仍在真实组装完成后产生。
- Hub 原生 SessionStarted 前 Stop 会 abandon claim；此前无 Completed EOF 被误结算成功，现明确 Failed/Other 且不启动已授权 fallback。使用真实私有 SQLite 重开验证终态与一次取消事实；未改 store schema、kernel/agent-core 或 runtime event 合约。
- 当前窗口提交的 ALVA 对话收到匹配的 user_message 后即可 Stop，外来 worker/run_token 不抢身份；ack 不清掉 Stop，实际 terminal 才清理。其他 engine 仍等 running；未扩展恢复的启动期会话控制，不改布局/外观/动效。
- **5 条新回归先红后绿**：adapter 3、产品 run seam 1、App 1。完整复跑 **341 Rust passed**（51 Hub core、35 ALVA adapter、239 Desktop lib / 4 精确真实账号 filtered、4 bin、12 HTTP）与 **210 UI / 20 files**；TypeScript/Vite build、Desktop/server all-target check、diff check、公开符号快照、依赖/bus/wasm32-unknown-unknown/full host link 全通过。wasm32-wasip2 缺 target 明确跳过；85 protocol/323 app-core 是前批证据，本轮未复跑。
- 无真实账号、付费模型、用户 MCP 服务调用或新增依赖；原生 AX/视觉/性能验收保持暂缓。同步三层文档与 ACCEPTANCE，保留用户 CONTEXT.md/不相关文件，仅本地提交，不 push。

## 2026-10-02 修复 · H17 MCP 生命周期与模板工具限制

- 用户“修复”后复查已有 MCP 管线：protocol client 与 app harness 的同 config 重复注册不再清掉连接；实际配置改变仍替换。protocol 握手/目录发现共用 timeout，目录失败进入 Failed；两层 connect 被取消均回到无连接状态，不残留 Connecting。
- protocol 查询依据 transport 存活过滤失效目录并可重新连接；正在执行 RPC 的 mutex 不阻塞状态/目录快照。两层 call_tool 在 transport 失效时直接拒绝；既有 harness 重连反例追加直接调用拒绝断言。AgentTemplate 声明 MCP set 但服务离线时保留 Some([])，不再用 None 放宽为继承全部父工具；显式普通工具和旧 exact MCP alias 兼容不变。
- **8 条新回归先红后绿**：protocol 5 条、app-core 3 条；最终完整复跑 **694 Rust passed**（85 MCP protocol、323 app-core lib、32 ALVA adapter、238 Desktop lib / 4 精确真实账号 filtered、4 bin、12 HTTP）。Desktop/server all-target check、rustfmt、diff check、public symbol snapshot 与依赖/bus/wasm32-unknown-unknown/full host link 全通过；wasm32-wasip2 缺 target 仍明确跳过，现有 unused-item/Result warnings 未在本轮扩范围处理。
- 没有 UI 代码改动，209 UI 与 TypeScript/Vite build 仍为前批证据，本轮不冒充重验；无真实账户、付费模型或用户 MCP 服务调用。更新分形目录约束与正式 ACCEPTANCE；kernel/agent-core 零实现修改，用户 CONTEXT.md/不相关文件保留，仅本地提交不 push。原生 AX/视觉验收及远程 MCP/auth、第三方插件安装等未完成项保持原状态。

## 2026-10-02 功能补全 · H17 ALVA 能力与真实 stdio MCP

- 按用户“先把底层逻辑、业务逻辑、该有的 UI UX 补上，不管美观”继续：schema 22 私有持久化真实 catalog 的 10 组能力开关，安全/审批/超时/循环保护不可关闭；每轮 Desktop/HTTP 捕获 snapshot 经 blueprint 注入 ALVA，包含 fallback，其他 engine 不受影响，不热改当前 Agent。受管空 MCP 列表不回退外部配置，AEP ambient loader 关闭；工具开关不是安全沙箱，子 Agent 工具可能独立。
- stdio MCP 不再 stub：实际初始化、版本协商、initialized、分页工具发现/调用；直接 absolute executable + argv，清环境、不继承凭据、不安装程序。1 MiB frame、256 tools/16 pages、有界 RPC/取消、Unix owned process group 回收；单服务 timeout + 60 秒总 startup budget，失效连接重连与失败清目录。所有 direct MCP 和 mcp_runtime 接既有 HITL，不信 server readOnly 注解。
- 模型工具名原 `mcp:server:tool` 不符合 provider 字符约束，改为统一稳定 64-byte `mcp_` hash alias，description 保留 logical identity；模板/派生路径兼容 exact 旧名，不展开 wildcard 权限。参考 [OpenAI function name 契约](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create) 与 [Anthropic tool name 契约](https://platform.claude.com/docs/en/agents-and-tools/tool-use/define-tools)，未修改 kernel wire helper。
- MCP create/edit 禁用、enable 显式宿主授权、revision/capacity/字段限额、delete 只删配置；一次性诊断绑定已存 session cwd，最多 2 并发，实际执行握手/发现后回收，无模型/计费/enable 写入。Desktop/HTTP 共享 store 与 probe，isolated QA 禁启用/测试。Settings/Plugins 接真实 switch/CRUD、确认、错误、保留草稿、focus restore、代际隔离与搜索；移除样例安装，不冒充外部 AEP registry。
- 最终 **238 Desktop lib passed / 4 精确真实账号 filtered、4 bin、12 HTTP、80 MCP protocol、320 app-core lib、32 ALVA adapter、209 UI / 20 files**；TypeScript/Vite build、all-target check、依赖/bus/wasm32-unknown-unknown/host link、public symbol snapshot 全通过。wasm32-wasip2 缺 target 明确跳过。收尾发现并修正新增名称转换的 Vec 类型推导与删除样例后的 unused import；重跑完成，不删除测试或放宽生产迁移。
- 自编 Python stdio fixture 验证真实传输/调用/cwd/无 HOME/字面 argv/超时取消与错误正文不泄漏；真实 store/迁移/HTTP/ALVA assembly+guard 回归验证功能，不接用户 MCP 服务、不运行真实账户、付费模型或下载第三方代码。原生 AX/视觉 QA 按用户授权继续暂缓；远程 MCP/auth、第三方插件安装、Worker 调优、SSH/IM 未完成。

## 2026-10-02 功能补全 · H17 本地自动化

- schema 21 持久化真实 interval 配置；创建/编辑暂停，启用前显示模型/provider/账号/账单/固定权限并显式确认，revision/route 改变拒绝旧授权。普通会话专用，不经 TeamRun 自治或自动 fallback，不建立 kernel 开关。
- Desktop 与 HTTP 沿现有执行/时间线/审批/Stop；schedule+Turn 同事务认领，每 10 秒 poll，最多 4 个并行。busy 和跨完整间隔的漏过 tick 跳过；成功后等完整间隔，失败/Stop/route change 暂停，重启不补跑。仅应用/服务运行时执行，无 daemon/cron。Pause 不取消当前 Turn，用户去 session Stop。
- **10 后端专项、8 UI 专项 + 1 App 接线、1 HTTP CRUD、1 隔离 QA gate** 新回归。真实引擎替身证明结果/日志落盘与 duplicate claim 不执行；崩溃/fallback double 不调用第二账户。隔离模式只允许暂停配置，禁启用与 pump；未运行真实账户或付费 inference。目录请求按代际隔离，不让迟到轮询覆盖新启用状态；加载失败与保存失败分开。
- 最终 Desktop lib **233 passed / 4 精确真实账号 filtered**、bin **3 passed**、HTTP **11 passed**、UI **202 passed / 19 files**；all-target check 与 TypeScript/Vite build 通过。旧 schema-18 fixture 残留新版 automations 表导致一次整包失败，修正 disposable fixture 后完整复跑通过；没有用 IF NOT EXISTS 放宽生产迁移。
- 原生外观/自动化交互验收仍暂缓；插件安装 registry、MCP 管理、全局工具/Worker 调优、SSH/IM、真实账号及 WKWebView 性能仍未完成。核心 SDK 未改，用户 CONTEXT.md/不相关文件保留；本批只本地提交，不 push。

## 2026-10-02 功能补全 · H17 工作区 Skills

- Skills 标签接 session 已存 cwd 的真实文件目录，新建/编辑仅受管项目路径，生态来源只读；复用 canonical SKILL.md parser，capability-relative no-follow/限额/敏感路径防护，源 revision 冲突拒绝覆盖。保留已有 enable-list 的全局名字及额外字段，不读取全局 Skills，不冒充完整 registry。
- 归档/恢复在 macOS/Linux 使用原子 no-replace 目录移动，资源保留；其他平台不展示此动作。下一轮 ALVA 读取，不热改正在执行的 Agent；Plugins 仍为不可安装样例，不联网拉取或执行脚本。
- 后端新增 **6 项**，包含真正 FsSkillRepository 发现与归档恢复；UI 新增 **7 项 + 1 App 接线**；真实 HTTP 创建/编辑/归档/恢复新增 **1 项**，引擎调用计数不增加。HTTP fixture 显式独立 catalog，不走真实共享/legacy 配置构造器。
- Desktop lib **223 passed / 4 真实账号精确 filtered**，HTTP **10 passed**，UI **193 passed / 18 files**；all-target check 与生产 web 构建通过。原生视觉/交互未冒充验收；无 kernel/agent-core 实现改动，无新外部包，协议 parser 的限定直接依赖已登记。
- Skills 完成后的依赖闸复跑 **exit 0**：全部边界、bus、wasm32-unknown-unknown 与 host 链接通过；wasm32-wasip2 target 未安装，明确跳过。

## 2026-10-02 功能补全 · H17 真实外观

- 应用级 AppearanceProvider 接通浅色/深色/实时系统主题、全局 default/compact Tokens、应用减弱动效；OS 无障碍偏好不能被关闭。偏好有版本校验、限额、跨窗口同步、重启恢复与卸载清理；保存失败仍可立即应用，但显示失败，不冒充持久化成功。
- 浅色语义 palette 与 tooltip/badge/品牌文字适配；密度不缩文字；去除位移动画时保留弹窗居中、开关/折叠状态和 spinner。没有修改 kernel/agent-core，没有新依赖或账号调用。
- UI **185 passed / 17 files**，其中外观独立 **13 项**；TypeScript/Vite 生产构建通过。浅色前景对正常/hover/active/inset 背景 AA 契约通过。真实 WKWebView 主题/视觉验收仍暂缓，源码和 jsdom 不冒充原生截图。

## 2026-10-02 当前交付与暂缓汇总

- 本轮自主迭代提交：Files `405e1309`、Git `457ce28c`、消息日志 `a71144f8`、PTY `a9d422c6`、浏览器 `5d10d7e8`、隔离原生 fixture `a0839db1`、Worktree IPC `f179468f`、次级动作/快捷键 `d08d297c`、安全依赖闸 `fdc5a0e8`、耐久 Inspector 补验 `7f0cffea`。全部本地未 push，单命令关闭签名；不改 Git 持久配置。
- 最终 lib 改用四个真实 CLI/账号测试的精确 skip，不误过滤合成回归：**217 passed / 4 filtered**；bin **2 passed**；HTTP **9 passed**；UI **172 passed / 16 files**；all-target check/生产 web build/依赖及 bus/wasm32 闸门通过。wasm32-wasip2 未安装，明确跳过。
- target 精度：当前 Cargo workspace/Rule 18 实际存在并检查的是 `alva-app-cli`/`alva-app-gpui`，未列 `alva-app-tauri`；不将脚本绿标冒充不存在 target 的检查，也不在 Hub 任务中迁移独立原生入口。
- 正式 `crates/alva-hub-desktop/ACCEPTANCE.md` 首节记录当下范围、命令、环境、account-free 原生入口与暂缓列表。原生截图另存 `native-before-worktree-fix.png`，只证明部分读取，不冒充修复后全流程验收。
- 用户暂不方便处理 AX 权限，本轮未再请求或绕过；真实 SDK/账号和计费仍未授权执行。完整 WKWebView 主流程、窄窗口/多语言/键盘、性能预算与每片段 SQLite latency 未验，不标 H18/H21 完成。历史 Chromium 证据不迁移成 WKWebView 成绩。
- 仅关闭本轮显式启动的 QA app 与本地 Vite/fixture-page 服务（1421/1437 已无监听）；不删除 fixture 或用户工作树/目录。已有 CONTEXT.md 和不相关 untracked 改动保留。

## 2026-10-02 自主迭代 · H14 耐久 Inspector 补验

- 使用真实临时 HubStore + Alva SessionRepository，不调用模型；保存 typed LLM/Tool 与 ChildSessionLinked，flush 后销毁 repository 并重开，经正式 `HubInspectorService` 读取根/子/孙 Agent。
- 精确比对 durable record_id/seq/payload、shared LLM 指标、父子关系和 branch 归属；另一个 Hub 会话不能读取该子树。缺失 product、缺失 native record 与 Claude unsupported 明确报错，并验证不会查询时创建不存在的记录。
- `cargo test -p alva-hub-desktop --lib inspection::tests -- --test-threads=1` **3 passed / 218 filtered**；两项为新增真实 store 桥接回归，一项为已有投影回归。生产行为未改，不把这些测试替代原生 Inspector 页面/焦点/性能验收。

## 2026-10-02 最终依赖闸检查 · 安全规则复用

- `ci-check-deps.sh` 首次最终复跑失败：H10 引入的 `alva-agent-security` 用于 Files/Text 和 Git 的统一 `SensitivePathFilter`，但漏同步 Desktop 的明确依赖 allowlist。Rule 17/18/19 均通过；失败是这个单一未登记直接依赖，不是反向 SDK 边界违规。
- 将此依赖及产品文件边界用途显式登记；不移除规则、不放宽其他 crate、不复制敏感路径实现。Desktop 约束同步记录这一限定用途。
- 复跑 `bash scripts/ci-check-deps.sh` **exit 0**：全部依赖边界、bus cap surface、`wasm32-unknown-unknown` clean 集合和 `alva-host-wasm` 完整链接通过；`wasm32-wasip2` 因 target 未安装明确 SKIPPED，不安装新工具链。
- `--skip real_` 还匹配了一个仅用 engine double 的旧 Alva resume 文本回归；已单独执行 **1 passed / 218 filtered**，没有账号访问。最终 lib 证据是 214 项批次 + 此 1 项单跑，4 个真实 CLI/账号测试未运行。

## 2026-10-02 自主迭代 · H17 真实动作与次级页收口

- Settings 的无效 Inspector 本地开关改为打开真实 Workbench Inspector；⌘/Ctrl K/N/B 接通搜索/新会话/工作台。Esc 仅停止无覆盖层的中央当前对话，不抢 Inspector 返回、工作台输入、菜单或对话框键盘行为。
- Stop 在请求期间按会话去重；迟到失败只显示到原会话，取消 ack 不冒充终态、不关闭新会话或同会话续跑的 Stop。新增 A→B 迟到 ack、同 session resumed run 与键盘范围回归。
- 未接通的独立 Workbench 窗口 disabled 并说明原因；插件/技能样例明确为 preview/not available，安装/注册仍 disabled，不以 `Available` 伪装 registry。Files/Text 按容器宽度排列，浏览器按钮可换行；没有改 Tokens/primitive 状态或新增页面断点。
- 验证：前端 **172 passed / 16 files**；TypeScript/Vite 生产 build pass。首次新增 Inspector 测试暴露完整 bridge mock 缺导出，补齐；插件按钮测试的文案断言写错，修正为实际 accessible name。以上为测试夹具修正，不删除或弱化产品断言。
- 最终跨端回归：Desktop lib **214 passed / 5 filtered**（显式不运行 real_/system_probe 账号/机器测试），Desktop bin 隔离模式 **2 passed**，Hub HTTP **9 passed**；Desktop + Server `cargo check --all-targets` pass。保留已有 Rust/React 警告与 Vite 大 chunk 警告，不为清警告修改无关模块。
- 原生 GUI 受 AX 权限错误阻塞后，用户于 2026-10-02 明确“暂时不方便，做你能做的”；本批不再请求权限或绕过权限，也不宣称 WKWebView 窄窗口/键盘/性能验收已通过。

## 2026-10-02 原生发现的缺陷 · Worktree IPC

- 真实 Tauri 项目页将已有 Git 项目误报为 `Git 发现失败：undefined`。原因是 command 返回嵌套 `Result<Result<Vec<_>, State>, String>`，Tauri 成功 body 实为 `{"Ok":[...]}`，与 web 数组/state 契约不符；mock 直接返回数组所以未检出。
- 改为 `WorktreeDiscoveryReply` untagged DTO；command 外层错误保留、产品 state 为带 code 对象，正常结果为数组。新增实际 serde JSON 边界回归，7 项 discovery 测试通过，Desktop/example build 通过。
- 后续原生重验当前被 Orca Computer Use 的 macOS AX 读取错误阻塞（权限报告 granted，但无法读取窗口）。已请用户重新切换授权，不绕过权限、不声称全原生验收通过。

## 2026-10-02 自主迭代 · H18 安全原生验收入口

- 新增 debug-only `--isolated-ui-data <absolute directory>`；主 WebView 使用 incognito，Hub store/provider/history/workspace 仅用指定目录。不读取共享/legacy Provider 配置，不探测真实账号，不恢复 worker；engine 执行、安装、账号操作与付费/发现请求在 invoke 边界拒绝。正常启动仍使用原 app-data。
- `examples/native_ui_fixture.rs` 在显式空目录构造双 Git worktree 和真实产品 store（注入确定性 engine double，保存 assistant 与工具全文），附浏览器测试页；Git 不使用用户/系统配置、hook、远端或 push。
- 已在真实 Tauri/WKWebView 读取完成会话、Markdown 代码、耐久 assistant 与 5 页工具正文。发现项目 worktree 的嵌套 Result 传输缺陷，待下一独立修复。此记录不是全桌面验收通过声明。

## 2026-10-02 自主迭代 · H12 独立用户浏览器

- Desktop 显式打开独立 incognito 原生窗口，地址/后退/前进/刷新/显示/关闭控制连接真实 WebView；session+随机 handle 绑定，最多 4 个，关标签/切会话/主窗销毁清理，迟到 Open 回收。不是 Agent 自动化浏览器，HTTP 模式明确不支持。
- 仅允许 HTTP(S)，拒绝凭证 URL、file/data/javascript/tauri、产品资源及开发主窗口来源；拒绝弹窗与下载。普通应用命令额外在全局 invoke 边界校验 main 标签（Tauri 普通应用命令默认不限 capability），core/plugin capability 仍只赋予 main；没有远端 capability。
- 按原生导航/加载回调显示地址与 loading，20 秒慢加载明确提示；实际网络错误由原生页面显示，不以超时冒充加载失败或成功。未声称禁止系统级浏览器权限提示或所有远端页面行为。
- 验证：URL/IPC 身份及 handle 归属 2 项通过；前端全套 164 项、Hub lib 213 项/5 filtered、Desktop all-target check 与生产 build 通过。真实原生窗口交互验收随后执行。

## 2026-10-02 自主迭代 · H13 原生逐行 PTY

- macOS/Linux Desktop 显式 Open 后创建真实 `/bin/sh +m -i` PTY，cwd 仅来自已存 session；清空继承环境，不加载 login profile。逐行输入、Ctrl-C、行列 resize、退出码、关闭/重开可用，不宣称支持 vim 等全屏模拟器。HTTP/非 Unix 明示不支持。
- session + 随机 handle 防迟到 close 杀新终端；最多 4 实例，输出环形缓存 256 KiB，输入 16 KiB；切会话/关标签/主窗销毁清理。清理先 kill owned process group 再 reap（waitid WNOWAIT 保留 PID 身份），回收 reader thread；终端是用户直接授权的 shell，不是安全沙箱或 Agent 审批通道，用户主动脱离进程组不在管理范围。
- 验证：真实 PTY 2 项覆盖 cwd、输入、resize、Ctrl-C、exit7、重开、跨会话拒绝、后台 sleep 进程组回收及输出上界；前端 161 项通过，生产 build 和 Desktop all-target check 通过。首次编译发现 macOS openpty 参数需要 mutable pointer，已修复并实测。

## 2026-10-02 自主迭代 · H20 消息耐久补页

- schema 20 新建可见 assistant 文本日志；沿现有 attempt/消息/UTF-16 offset 协议，在 live 发送前事务保存，16 KiB UTF-8 分块，不再只依赖 Turn 结果摘要。仅有 Completed(result) 的 provider 同样完整保存。写失败使 attempt 失败，不假装耐久成功。
- Tauri/HTTP 提供会话隔离的 128 条游标页，固定 through 高水位，一条游标边界重放用于观察终态。前端初次打开及每 2 秒补页，实时重叠去重、旧会话迟到结果丢弃、崩溃后已读消息也同步 interrupted；只替换有日志 Turn 的摘要，旧记录不能补造丢失片段。
- 验证：Hub lib 串行 207 项通过/6 filtered（当时 real_ 过滤误含新合成 worktree 测试，已改名；该测试此前独立通过）；随后新增发送前持久化回归 1 项通过。前端 159 项通过，HTTP 9 项通过，双端 all-target check、生产 build 通过。4MB Unicode 多页重开、缺口补齐、重复回放、terminal 先到与切会话均覆盖。
- 这是可见 assistant 消息的耐久恢复，不是所有 raw adapter 事件日志；工具、审批、协作继续使用各自已有记录。私有消息不自动加入跨引擎 handoff；尚未声称持久化每片段的真实桌面延迟预算通过。

## 2026-10-02 自主迭代 · H11 真实 Git

- Git 页绑定已存 session cwd，读取真实 Git branch（非 EngineBranch）、staged/unstaged/untracked、选中文件 diff；Review 单独保留工具执行记录。终态 detail revision 与手动 Refresh 都触发重读。
- 只读命令禁用 optional locks、外部 diff/textconv/fsmonitor，清除继承 Git 环境及用户级配置；10 秒超时、status 1 MiB/2000 路径、diff 256 KiB 限额。NUL 解析保留特殊文件名，literal pathspec 防通配，rename 双端过滤敏感路径。没有新增 stage/commit/discard。
- 验证：真实临时主仓库+linked worktree、staged/unstaged/untracked、rename、删除、二进制、嵌套 cwd、中文/空格/方括号路径、大 diff，4 项通过；HTTP 全套 9 项通过（含真实 Git 接口），前端 154 项通过，Desktop all-target check 和生产 build 通过。真实 Tauri 视觉验收后续执行。

## 2026-10-02 自主迭代 · H10 真实 Files/Text

- 已保存会话的 cwd 是唯一根，Tauri/HTTP 共用只读服务；Files/Text 从真实目录导航并读取 UTF-8 文本，不再以工具记录推测文件。尚未创建会话时明确提示先打开会话。
- capability-relative 逐层 no-follow，复用 SensitivePathFilter 并隐藏元数据/认证目录；特殊文件、二进制、失效路径均明确拒绝或标注。列表 2000 项、预览 256 KiB，超限提示而非伪装完整；原文件不修改。
- 验证：文件服务 4 项、HTTP 文件集成 1 项通过；前端全套 151 项通过；Desktop/Server all-target check 与 TypeScript/Vite build 通过。首次 TS 检查暴露 Notice 缺 heading 与多余 import，已修复并复测。
- UI 覆盖目录进入/返回、惰性 HTML 文本、限额提示、失败刷新、迟到旧会话隔离。未声称真实 Tauri 视觉验收；后续与 Git/主流程一起验收。

## 2026-10-01 继续实施 · 工具全文分块

- schema 19 新增不可变工具正文引用与 UTF-8 安全分块（每块至多 16KiB）；完整正文先脱敏，再与 side-effect evidence 在同一事务落盘，timeline/history 仅携带摘要和引用。单份原文上限 64MiB，超限明确失败，不静默截断。
- Tauri 与 HTTP 提供会话隔离的逐块读取；错误会话、缺失引用和越界页不泄漏正文。前端仅保留当前页，支持前后翻页、加载状态、失败重试，并丢弃切换后的迟到响应；旧记录明确标为摘要，不能恢复从未保存的全文。
- 验证：Hub lib **199 passed / 5 filtered**；Hub server **8 passed**；前端 **148 passed**；Desktop + Server `cargo check --all-targets` 与 TypeScript/Vite build pass。覆盖 Unicode 全文重组、重开数据库、旧版迁移、脱敏跨块边界、事务回滚、HTTP 会话隔离与 UI 分页/重试。
- 本轮没有真实模型计费调用，也未进行真实 Tauri 视觉/性能验收。完整耐久消息日志、断线补页及 Files/Git/浏览器/终端仍未完成。
- 消息协议已单独提交 `579fbe46`；1Password 签名失败，本批使用单命令关闭签名的本地提交，不修改持久配置，不 push。

## 2026-10-01 继续实施 · 消息片段协议

- 移除“正文相等就是重复”的启发式；新 live DTO 在共享产品 emit 层携带 attempt-scoped stream_id + UTF-16 offset，Tauri/SSE 不各自生成序号。相同正文不同位置保留，同位置重发去重，乱序片段等缺口补齐，终态不因迟到片段复活；跨 attempt 的相同 native message ID 分开。
- 旧事件无 position 时按到达顺序追加，不能猜测是否重发。普通 native ID 的 assistant_message 正确归类消息，不落入 control 卡。
- 验证：Hub lib 串行 **195 passed / 5 filtered**；前端 **143 passed**；Desktop + Server `cargo check --all-targets` pass；TypeScript/Vite build pass。无真实账号/计费调用。
- 边界：完整耐久消息日志、断线补页与全产品验收仍未完成；工具全文分块另一个提交实施。

## 2026-09-22 Review 剩余两项修复

- 本轮关闭原 5 项 review 中剩余的工具顺序和长列表定位，不代表下方产品 backlog 全完成。
- `a029555d`：工具首次观测时间/序号写入 side-effect evidence，ToolEnd 不覆盖；live DTO 与历史共用元数据并以 attempt 隔离工具 ID。反向完成、同毫秒、逆字典 ID、跨 attempt 重复 ID、旧 JSON 和 SQLite reopen 均有回归。旧数据只能保留已存数组次序，不能恢复从未保存的时间。
- `0de6fe52`：有界列表统一负责 follow 与测量补偿；初始化先定位 scroll owner，尺寸变化导致的 scroll clamp 不取消跟随，避免同步重复布局循环。所有事件共享窗口，工具摘要默认展示。
- `ea44cadd`：将前批已通过测试的项目/工作树选择独立提交；可用工作树用于新任务 cwd，不改写历史会话工作区。
- 验证：Hub lib 串行 **194 passed / 5 filtered**（跳过真实机器/凭证探测），前端 **140 passed**，TypeScript + Vite build pass。
- 真实浏览器通过内置浏览器控制验证：100/1000/10000 条初始底部误差不超过 0.5px；10000 条挂载 11 行；resize、jump、session switch、stream 完成继续跟随；向上阅读同一 key 的 offset 从 -686.890625px 到 -686.890625px，stream 完成不漂移。不是 Tauri WKWebView 性能验收。
- Playwright runner 的 `npm exec` 被一次性包执行保护拦截，未重试/绕过该命令；改用已提供的内置浏览器 UI 验证。`windowing.spec.ts` 已补精确底部、resize/jump 与真实行锚点断言，但本轮不声称该 runner 执行通过。
- 仍不在本轮关闭范围：全文分块、相同文本 delta 协议、真实文件/Git/终端/浏览器工具、完整桌面性能和真实订阅验收。Kernel/agent-core 零改动。

## 2026-09-22 Review 后半批修复（本批仅 3/5 项）

- 范围：SDK cwd 信任边界、配置 argv 泄漏、SDK readiness 误报；未动滚动定位和历史排序。
- SDK 只从用户级 Hub runtime 安装目录解析，校验 manifest 身份/版本结构与绝对入口、拒绝 symlink/Unix 不安全权限/与任务目录交叠；探测不 import 包。装配与 readiness/veto 使用同一解析器。
- SDK 缺失或非法以 `dependency_unavailable` 明确阻止启动；修复入口给出受管安装位置，不自动安装或退回项目/全局 SDK。真实供应商 SDK 兼容性及账号仍未验收。
- 配置从 argv 改走 version=1 的私有 stdin 首帧，写入完成才允许控制消息；8MiB 上限、宿主 2s 写入超时、bridge 10s 初始化超时。
- 提交：`864b5b36`（私有 stdin 初始化）与 `9ad16d32`（受管 SDK pin/readiness）。均仅本地提交，先前 UI 改动不在其中。
- 验证：Claude adapter 原有 102 项通过，新增私有初始化测试单独 1/1 通过；Hub lib 串行 **193 passed / 5 filtered**，SDK assembly 专项 **7 passed**，readiness 纯测试 **15 passed / 6 filtered**（不运行机器凭证探测）；前端关联测试 **50 passed**，`tsc -b` pass。假 SDK 无网络调用，假 key + 30 万字符正文完整到达且 argv 只有 Node/脚本。
- 不隐瞒失败：Hub lib 并行曾 **192 passed / 1 failed**，失败为 `session_workspace_kind_persists_across_reopen` 数据库重开锁占用；随后串行 193/193 通过。本批未改 DB 锁实现，不宣称解决该并行不稳定性。

## 2026-09-22 接手修正（优先于下方历史完成声明）

用户确认由当前 Agent 接手未完成实现。基线 `3fd7ed08`，以下变更正在工作区中，尚未完成全产品验收：

- H09/H21：修复消息列表与工具列表分离、工具默认折叠、工具 DOM 不受窗口约束的问题。消息/工具/审批/状态共享一个有界列表；工具参数与结果默认展示。动态高度使用 ResizeObserver，支持锚点补偿与卸载清理。
- H05/H08：首页与左栏显示项目的真实工作树，可将有效工作树选为下一会话 cwd；缺失/bare/prunable 工作树禁用，历史会话目录不被改写。尚未完成项目下会话分组的最终信息架构验收。
- H06：产品 Claude 装配由 Light print 改为既有 Full SDK adapter；Default 权限回调、持久化 session、默认不加载项目 setting sources，unrestricted 才 bypass。补装配权限测试；安装修复指引不再将 Node 缺失误修成安装 Light CLI。SDK package readiness 与真实账号调用仍未验收。
- 回归：Hub lib `cargo test --offline -p alva-hub-desktop --lib -- --skip real_` **188 passed / 5 filtered**；前端 `tsc -b` pass，vitest **137 passed**；Chromium `tests/windowing.spec.ts` **2 passed**（新增统计全部行，不只消息）。不是 macOS WKWebView 性能验收，没有真实计费调用。
- Kernel/agent-core 本批零改动，不扩大 K01 的历史验证结论。
- 补充验证：工作树选择独立测试 **1 passed**（前端累计 138）；Claude adapter 的 fake SDK / permission / cancel / session-cleanup 测试 **34 passed**，没有真实账号调用。

### 明确未收口，不得标完成

1. H09 全文：2026-10-01 已实现新记录的耐久全文分块与分页读取；旧记录仍只有已存摘要，真实 Tauri 全流程验收未完成。
2. H20 事件协议：工具身份/顺序、stream/offset、schema-20 可见 assistant 日志与断线补页均已实施并有回归；真实桌面持久化延迟与长会话性能仍待验。
3. H06 SDK readiness：受管依赖探测与 fake-SDK 产品链路已在 `9ad16d32` 修复；真实供应商 SDK/账号端到端仍未验收。
4. H10/H11：真实文件树/预览与 Git status/diff 已接线并通过后端/HTTP/UI 回归；原生 UI 全流程验收待完成。
5. H12/H13：隔离用户浏览器与真实逐行 PTY 已接线，安全/lifecycle 回归通过；原生整段窗口交互仍待验，HTTP 明示 unsupported。
6. H14/H17/H18：Inspector 耐久桥接/递归和次级页动作已补验/修复；原生端到端、焦点/窄窗/键盘、生产 WKWebView 性能及真实账号验收仍待完成。

下方勾选项属于先前执行者的历史记录，不构成本轮全量验收。

- 目录：`.scratch/alva-hub-workbench-reset/`；执行 Agent 会话，2026-09-20。
- 基线：`764864da`（TASKS.md 规划基线）。分支 `refactor/kernel-boundary`。
- 签名说明：1Password 签名代理在本会话中途持续返回 "failed to fill whole buffer"，自 `9079abfe` 起的提交以 `--no-gpg-sign` 落库；此前 `a250cb9d`/`b0a649e0` 为签名提交。

## 状态总览

| 卡 | 状态 | 交付 commit / 文件 |
|---|---|---|
| H01 | ✅ 交付 | `b0a649e0` INVENTORY.md + LEGACY-MAP.md |
| H02 | ✅ 交付（工具区位置保持右侧可逆） | `a250cb9d` glossary 增量 + `b0a649e0` SPEC.md |
| H19 | ✅ 基线+预算交付（两轮方法修正，见 PERF §2） | `9079abfe`（夹具/runner）+ `98387ad5`（runner 修正）+ PERFORMANCE.md |
| H20 | ✅ 交付 | `9079abfe` session/timeline-model.ts |
| H21 | ✅ 交付并达预算 | `98387ad5` bounded-list + 行级 memo |
| H04 | ✅ 后端+bridge 交付（迁移 18） | `e4a91a8c` |
| H05 | ✅ 只读发现交付 | `e4a91a8c` worktree_discovery.rs |
| H08 | ✅ 两模式入口交付 | `5d6ac7d3` StartupGate + App 接线 |
| H09 | ✅ 核心交付 | `4b946a79` R42 迁移：工具调用/结果、审批、状态与消息同轴展示；HubTimeline 键改 H20 稳定 id；条件挂载取消（终态后执行历史仍在轴上）。**未竟部分**：滚动跟随/回底与审批的实测在真实 Tauri 内待 H18；工具输出的全文分块（>限额 blob 全文可达）未做 |
| H15 | ✅ 9/9 交付 | R78 `16fb6c75`、R77+R86-http `65d58265`、R91 `f5fdf081`、R86-desktop `73289ba9`、R85 `2dd8edea`、R84 `467064d4`（+lib re-export `1a50ce95`）、R88 `5c658718`、R89 `e6fc3a13`、R87 `638d0d54` |
| H16 | ✅ R96/R97 修复 | `0c98f529` |
| K01 | ✅ 检查执行（pass，见下） | 本次运行记录 |
| H06 | ✅ 创建/删除连接闭环交付（2026-09-21 会话） | `09d56d41` |
| H07 | ✅ 路由身份证据测试交付（2026-09-21 会话） | `a11583de` |
| H03 | ⏸ 方案已交付，**待用户确认决策点**（2026-09-21 会话） | `14228492` IA-PLAN.md |
| H10-H14/H17/H18 | ⏸ 未开始（H10-H14/H17 依赖 H03 确认后批次 B1-B5；H18 依赖其后） | 见"未完成与原因" |

## 2026-09-21 会话（接力：验证既有交付 + H06/H07/H03）

- **基线复验（当前 HEAD）**：vitest 131/131（修复 tsc 后 134/134）；hub-desktop lib 181+4 已知 real-CLI 环境失败（与 H15 记录一致，非回归）；K01 结论无需重跑（本会话 kernel/agent-core 零改动）。
- **tsc 回归修复**（`5adc60f0`）：R89 测试（e6fc3a13）引入 4 个 tsc 错误（未用变量、mock 参数类型、null 收窄），vitest 用 esbuild 转译不查类型所以运行时全绿掩盖了回归。修复纯类型卫生，运行时语义不变。**教训：EXECUTION.md 此前"H08 时 tsc 干净"的结论在后续 commit 后失效——tsc 检查应列入每批验证命令。**

### H06 — Provider 创建/删除闭环（`09d56d41`）

- 探查结论（子代理全量盘点）：兼容矩阵（route_planner 单一权威+契约测试）、订阅/API 分离呈现、凭证闭环（0600+DTO 只出 bool+红acted 诊断）、保存/编辑/重启测试**已达标**；缺口是 create 不存在（前端 Add modal 用假 id 调 save 必然报错——假入口）、delete 完全不存在（H06 验收明确要求删除测试）。
- 交付：`ProviderCatalog::create`（后端分配 id `{kind}`/`{kind}-2` 与内置无冲突；订阅 kind 拒绝；kind 白名单与 planner 矩阵契约测试互锁）+ `ProviderCatalog::delete`（内置 starter/managed 拒删；default_target 指向被删连接时显式清除；会话历史保留 provider_id、路由投影不可用）+ DTO `deletable` 由后端裁定 + Tauri 命令 + hub-server HTTP（POST /api/providers/create、DELETE /api/providers/:id）+ bridge + Add modal 走真实 create + 协议选择 + 删除确认弹窗 + preset 过期预置模型表删除（脱节问题）+ 创建失败弹窗内如实报错。
- 测试：Rust providers 19/19（4 新增）；vitest 134/134（3 新增：真实 seam 创建、失败不假成功、删除确认+内置无删除入口）；hub-server 7/7；tsc 干净。
- **H06 未竟（如实列出，需用户决策或后续卡）**：
  1. **Claude 产品路径是 CLI print 适配器（engine_wiring.rs:342-417），不是卡面写的"Claude Code SDK"**。切换到 Node Agent SDK 桥是架构级变更（运行时依赖、认证面、装配全变），未获授权不擅自重写——需用户决策：接受 CLI print 路径（修改 SPEC 措辞）还是立项 SDK 桥卡。
  2. typed AuthSource/Subscription 枚举未做（现仍字符串 kind + managed/target_uses_api 接缝；UI 分开呈现已达标）。若 H02 术语要求 typed 字段，另立小卡。
  3. 同一协议多连接（多账号）仍不支持（catalog 每 kind 一连接）；V1 未要求，登记为已知限制。

### H07 — 发任务选择执行组合并锁定实际路由（`a11583de`）

- 现状核查：链路保证已存在——run seam 用持久化 `prepared.target` 装配（run.rs:243-247）、`run_hub_prompt` 强制新会话显式目标/显式默认路由且 `canonical_available_route` 拒不可用（main.rs:168-206）、恢复会话禁止覆盖钉定路由、同名模型按 provider 区分与未知模型不发明控件已有 picker 测试。**未发现需要修的缺陷**；缺的是把验收矩阵钉死的证据测试。
- 交付两个证据测试：
  - `selected_route_identity_reaches_the_engine_request_with_cwd`：同一请求内断言 build_engine 收到逐字段选中目标 + 假引擎 RuntimeRequest 的 canonical cwd/prompt + branch 快照完整身份（provider/account_source/billing_route/reasoning_effort）。
  - `later_route_changes_never_rewrite_recorded_attempts`：A 运行后新会话选 B，重启后 A 的 branch 仍 A；带 B 恢复 A 仍按 A 装配（observed 序列 A,B,A）——当前选择状态永远不能改写历史。
- 测试：lib 187 通过 + 4 已知环境失败。
- 未竟：真实 Tauri 内 composer→请求 的端到端复测归 H18（本卡证据层为 run seam 假引擎捕获，与 H18 桌面壳验收互补不重复）。

### H03 — IA 结构方案（`14228492`，待确认）

- 交付 `IA-PLAN.md`：终态页面清单 P0-P8、中央轴结构确认保留、两模式补强项、搜索重定义、Workbench 左右 token 方案（默认保持右侧+可逆开关）、设置/次级页诚实化规则、B1-B6 落地批次、§10 事实索引（全部 文件:行号）。
- **4 个决策点待用户确认**（IA-PLAN.md §9）：①工具区默认位置；②⌘K 实现还是删假徽标；③"当前项目"行做否；④Automations/Extensions 保留结构+诚实化还是整页降级。
- 按卡验收要求，确认前不实施大面积改动；H10-H14/H17 在 B1-B5 批次内跟随。

每卡证据（命令 + 结果摘录）：

## H01/H02 — 现状与规范

- 证据：三路并行勘察（前端 / 后端 / .scratch 旧卡）产出 INVENTORY.md 逐项文件:行登记；LEGACY-MAP.md 核实 R45/R77-R91/R96/R97 在全 git 历史无修复 commit，旧 UI 总表"00-12 全完成"被 round5 HISTORICAL-UI 判伪。
- 验证命令：`git log --all --grep "R77"` 等逐编号（无修复记录）；结论登记于 LEGACY-MAP.md。
- 词汇表增量：Project / Conversation mode / Project mode / Engine+Pi（`docs/ALVA-HUB-GLOSSARY.md`，commit `a250cb9d`）。

## H19 — 性能基线

- 夹具：`web/src/perf/`（`?perf=1` URL 门控，生产构建可用，产品面无入口）。确定性种子 mulberry32(20260920)，n=1000/10000，含 ~1MB 单条输出、失败/中断/审批卡。
- runner：`web/perf/run-perf.mjs`。帧证据 = CDP Tracing `DrawFrame`（**非 rAF 采样**）；长任务 = PerformanceObserver('longtask')；输入延迟 = wheel 派发 vs 页面事件 timeStamp；DOM/堆 = DOM 计数 + performance.memory。
- **方法修正（诚实记录）**：初版用 wheel burst 驱动，DrawFrame 间隔混入空闲等待，滚动 p95≈155ms 是采集假象；修正为 CDP `Input.synthesizeScrollGesture` 连续手势后，基线滚动 p95 实为 ~10ms（滚动由合成器线程承担），**真实代价在长任务与堆**（n=10000 流式 16s 内 227–242 次 >50ms 主线程阻塞、堆峰值 270–536MB、切会话 ~1s）。废弃数据保留在 results/ 目录。
- 预算判定（新方法、同机同种子）：基线 n=10000 时 TTI 951–978ms（贴线）、长任务/堆/切换超预算 → H21 必要性成立。

## H20 — 数据解耦

- `session/timeline-model.ts`：`TimelineEntry`（规范 id=`session::item`、turn/attempt/tool 归属、确定性排序）；`mergeEntry` 为历史回放与实时事件**唯一**合并语义（R29 正文保留 / R40 控制条目新体覆盖 / 重复全文投递不再拼接 / 迟到事件按序插入）。App.tsx 历史与实时均走该模型；旧 `mergeLiveTimeline` 变薄包装。
- 测试：`timeline-model.test.ts` 13 项（含"回放与实时收敛为同一列表"契约）；全量 vitest 129/129。修复过程中发现并修正分类缺陷：消息性由 role 优先判定（item_id 形态仅作回退），与 R29 语义一致。

## H21 — 长会话滚动与流式性能

- `session/bounded-list.tsx`：BoundedList（按测高+估高前缀和二分窗口、pad spacer 保持总高、layout-effect 锚定校正）；`chat-transcript.tsx` 行级 `memo`（流式 delta 只重渲染尾条 Markdown）。
- 真实浏览器证据：`tests/windowing.spec.ts`（Playwright，dev server）2/2 —— 1000 条会话挂载 <120 行、滚动可达顶部、流式追加下锚点位移 <4px。
- 前后对照（同夹具同 runner 同机，n=10000，3 次中位）：

| 指标 | 改造前 | H21 后 |
|---|---|---|
| S1 首开 TTI | 951–978ms | **285–325ms** |
| DOM 节点 | 142,567 | **20,514** |
| S2 滚动 p95 | 10.1ms | 10.6ms（均达标 ≤20ms） |
| S3 快滚长任务 | 39–43 次 | **7–10 次** |
| S3 堆峰值 | 478MB | **28MB** |
| S4 流式长任务 | 227–242 次/16s | **0–1 次** |
| S4 堆峰值 | 270–536MB | **62–66MB** |
| S5 切会话 p50 | 997–1008ms | **105–106ms** |
| S5 20 次后堆 | 242MB | 65–66MB（稳定，无线性增长） |

- 原始 JSON：`web/perf/results/perf-results-1789837394299.json`（前）、`…8133852.json`（后）。

## H04/H05 — 项目与工作树

- 迁移：schema 18（projects 表 + `hub_sessions.project_id` 可空列），纯增量；pre-18 构建经版本闸拒绝、不损数据（回滚 = 旧版本 refuse + 新表 drop，无旧列改写）。
- 语义：canonical 路径唯一（别名/symlink 收敛；用户目录允许 symlink，与 Hub 受管目录的拒 symlink 规则区分）；项目可无会话存在、重启保留；delete_project 仅解绑不触盘；set_session_project 双端校验。
- H05：`git worktree list --porcelain` 纯只读；porcelain 路径逐字解析（空格/中文）；区分 Hub 受管（`hub/worker-*` / 受管根下）与用户工作树；非 Git = 状态而非错误；缺失目录显式 MissingDirectory。**不创建/prune/删除任何工作树。**
- 测试：store 9 项（含重启持久、别名收敛、解绑保留历史、legacy NULL）+ discovery 6 项（含真实 git 仓库夹具：linked/detached/locked/prunable/受管标记/只读不变）。命令：`cargo test -p alva-hub-desktop --lib worktree_discovery` / `project` 全绿。
- 未验证：hub-server HTTP 侧未加对应端点（Tauri 优先）；4 个 real-codex/claude CLI 测试因本机 PATH/凭证缺失按测试内建守卫失败（非本次改动引入）。

## H08 — 两模式入口

- StartupGate（`components/startup-gate.tsx`）：对话模式（一键受管目录会话，无需仓库）与项目模式（项目列表 + 添加 + 刷新 + 删除）均为正式入口；入口选择持久化（localStorage `alva-hub.entry-mode`）。
- 聚合：项目行显示会话数 + 工作树状态行（用户/受管/缺失目录/非 Git）；打开项目 → 绑定其工作目录 + 归属过滤；新建会话创建后一次 `setSessionProject` 归属（不改 cwd）；打开历史会话按其**自身存储** project 恢复上下文（不按模式猜）。
- 模式切换只改列表范围，不搬迁会话/不扩权限；删除项目仅解绑。
- 验证：vitest 129/129（集成测试更新到新入口文案）；`npx tsc -b` 干净。Playwright 真实浏览器冒烟夹具路径可用（?perf=1 与产品面共壳）。
- 未验证（诚实项）：真实 Tauri 桌面壳内的 H08 主流程（加项目→选目录→新任务→切历史→重启恢复）待 H18；浏览器模式（hub-server）项目面显式声明不可用（`startup.unsupportedTitle`）。

## H16 — R96/R97

- R96（Pi overflow 压缩恢复）：
  - 修复：`agent_end(willRetry:false)` 携带未决错误不再升 fatal——发可恢复提示（用户可见"等待结算"）并继续读流；`settle_at_eof` 在完整请求结算时把仍待定错误定性为唯一 fatal（原文保留）；`AutoRetryEnd{false}` 消费 pending 防止双 fatal（保住 R2）。
  - 动态反例（round10 原证据工程）：`cargo run --offline --manifest-path .scratch/kernel-boundary-review-round10/evidence/adapters/Cargo.toml` → `compaction_recovery=true, fatal=false, answer_preserved=true`，exit 0（修复前 false/true/false、exit 101）。
  - 适配器测试 23/23；conformance OutputAfterFatal 夹具改锚真实 fatal（重试耗尽），场景语义不变。
- R97（合成 fatal handoff 身份窗口）：
  - 修复：`settle_synthetic_failure` 把 Failed 发布与 handoff 身份恢复合并进**同一写锁临界区**，且先于合成终态可见；R93 观测 id 投影不变；显式 fatal 路径回归保留。
  - 新回归：`synthetic_fatal_eof_handoff_settles_identity_before_the_retry_window`（51/51 hub-core 全绿）。
  - **诚实记录**：红测验证表明旧序下该窗口无法用顺序式测试稳定复现（与 round10"动态未复现"一致）；本修复的价值是构造性消除窗口（同一临界区、发布前完成），测试作为契约回归底线。

## K01 — Kernel/SDK 独立检查

- 检查命令与结果（本次运行，全部通过，exit 0）：
  - `bash scripts/ci-check-deps.sh` → **PASSED**（含 wasm32-unknown-unknown-clean crate 集 + alva-host-wasm 完整链接；`wasm32-wasip2` 检查因 target 未安装 SKIPPED，已如实记录）。
  - `bash scripts/public-surface.sh` → `alva-kernel-abi OK` / `alva-agent-abi OK` / `alva-agent-core OK`。
  - `cargo test -p alva-kernel-abi -p alva-kernel-bus -p alva-agent-core -p alva-kernel-core` → 全部 target `ok`，0 failed（约 200 项定向测试）。
- 本会话对 kernel/agent-core 的**唯一**改动是 hub-core（L7 编排层，非 SDK 内核）的 R97 修复与 pi 适配器（L4.5）的 R96 修复；kernel-abi/kernel-core/bus/agent-core 零改动（public-surface 快照零漂移佐证）。
- **K01 结论：pass，未发现可复现内核缺陷 → K02 不激活（不适用）。**
- 未跑：`cargo check --target wasm32-unknown-unknown` 单独穿透本会话未执行（ci-check-deps 已覆盖 wasm-clean 集合判定）；`wasm32-wasip2` 编译检查因 target 未安装跳过。两项均如实记录，不冒充通过。

## H15 — 执行控制与异步串台缺陷（原卡逐卡）

全部沿 LEGACY-MAP 钉定的原编号执行；每个原卡独立 commit；先失败反例后修复（有测试夹具的卡）。

- **R78**（`16fb6c75`）：重复 Run 认领前删除活动轮 stopping marker。红测 `rejected_duplicate_run_preserves_the_active_stopping_marker` 在旧代码下零 Interrupted（wait_for 超时失败），修复（删除 pre-claim 清理）后绿；R65 全部 stop-settlement 回归保持绿。
- **R77**（`65d58265`，含 R86 HTTP 半）：迟到 cancel ack 把取消事实/marker/stopping 事件写到同 pid 下一轮。关键认知：同分支 resume 会铸造新 attempt，**branch 不是 Stop 身份，attempt 才是**。修复 = stop 在 await cancel 前捕获 `running_attempt_id`，经 `record_attempt_cancelled_bound`（事务内核验该 attempt 仍是运行中的那个）条件写事实；superseded ack 返回 `{stopping:false, superseded:true}`，不插 marker（键改 pid→attempt）、不发事件、不清审批（R86 HTTP 半：clear 后置）。marker 只被结算同一 attempt 的 run 消费。红测 `late_cancel_ack_never_marks_the_next_attempt` 旧代码 `{stopping:true}` + B 流上出现 "User stopped"，修复后绿。
- **R91**（`f5fdf081`）：Desktop Stop 在 await cancel 前写取消事实（cancel 失败留虚假取消），ack 后立即广播 Interrupted（与自然 Completed/Failed 冲突、UI 虚假"已退出"）。修复 = 身份预捕获 + `record_attempt_cancelled_bound` + 移除抢先广播，真实流结束时由 run seam 结算唯一终态（对齐 R65 后 HTTP 语义）。
- **R86 desktop**（`73289ba9`）：审批路由清理由 cancel 前移到"cancel 成功且事实已写入被停 attempt"之后——cancel 失败时权限请求仍可回答；superseded 不清（不毁 B 的待审批）。
- **R85**（`2dd8edea`）：hibernate_worker 不再吞掉一切 stop Err 后无条件删句柄——复用 `release_worker_runtime` 的保留语义（无运行=正常清理；真实 cancel 失败=保留可停句柄+告警）。失败语义：生命周期已 Hibernated（不认领新任务），在途运行仍可 Stop。
- **R84**（`467064d4`）：execute_session_run 对任意 outcome 无条件按 pid 清 active_hubs/审批——被拒的重复 Run 剥掉在跑 A 的控制面。修复 = 仅 outcome Ok（真正认领并流过）才做兜底清理；认领前拒绝不动现场。
- 附带修复（`1a50ce95`）：H05 提交漏了 lib 根 re-export，bin 目标自那时起未编过（--lib 测试不覆盖 bin）——本次 bin check 暴露后修复。
- **R88**（`5c658718`）：挂起的 loadSession 在更新导航意图后返回会重开旧会话。修复 = `sessionLoadSeqRef` 单调序号门控每次安装（会话/workspace/project/route/profile/timeline/detail），startNewSession 也推进序号；team fetch 返回后二次校验。红测用受控 Promise：过期响应重装 A（gate 消失）在旧代码下失败、新代码通过。教训记录：waitFor 断言不得咬初始态（第一版红测因此未咬住，用 act 冲刷后修正）；测试尾部恢复共享 mock 防止实现泄漏（泄漏曾打红 8 个 R29/R30 回归）。
- **R89**（`e6fc3a13`）：终态/详情刷新在事件到达时校验归属、响应落地只查组件存活——A 的迟到刷新把 B 的 sessionDetail（标题/memory/分支/Inspector）整体替换。修复 = 响应到达时对 `activeSessionRef` 二次校验；worker 驱动的 team 刷新同样钉住 lead。红测：受控 A 刷新在切到 B 后落地，旧代码覆盖 B 标题（红），新代码保持（绿）。
- **R87**（`638d0d54`）：claude cancel-grace 强杀文案被分类 Crash 并触发自动 fallback。修复 = fallback 循环在继续连续性链之前查询本 attempt 的取消事实（`attempt_has_cancel_fact`，经 attempt 的 started_at 绑定）；有事实即中断链——用户要它停，后继引擎绝不能被启动；无事实的真实 crash 仍照常 fallback（既有 fallback-chain 测试保持绿）。红证：禁用守卫时 `user_stop_grace_kill_never_falls_back` 失败（builds==2，第二个引擎被真实构建）；启用后 builds==1、单 attempt。
- 附带修复（`1a50ce95`）：H05 提交漏了 lib 根 re-export，bin 目标自那时起未编过（--lib 测试不覆盖 bin）——本次 bin check 暴露后修复。
- 回归基线：hub-core 51/51、hub-server 7/7、hub-desktop lib 185 项中仅 4 个 real-codex/claude CLI 环境测试失败（需真实二进制与凭证，改动前即失败，按测试内建守卫报错）；vitest 131/131。
## 未完成与原因（不掩盖，2026-09-21 更新）

1. **H03 方案待确认（当前闸门）**：IA-PLAN.md 已交付（`14228492`），4 个决策点等用户选择（§9）；确认前不实施大面积 UI 落地。H10-H14/H17 按 B1-B5 批次跟随。
2. **H09 收尾（小尾巴）**：中央轴合并已交付（`4b946a79`）；剩余为真实 Tauri 内的滚动跟随/审批实测（归 H18）与大输出的分块全文可达（现由窗口化+滚动保证可达，分块未做）。
3. **H06 未竟（需用户决策）**：① Claude 产品路径是 CLI print 而非卡面"Claude Code SDK"——SDK 桥是架构级变更，未授权不擅自重写，需决策（接受 CLI 现状改 SPEC 措辞 / 立项 SDK 桥卡）；② typed AuthSource 枚举未做（登记）；③ 同协议多账号多连接不支持（V1 未要求，登记）。
4. **H07 未竟**：真实 Tauri 内 composer→请求端到端复测归 H18（本卡为 run seam 假引擎证据层，互补不重复）。
5. **H18 真实 Tauri 全流程验收未开始**：依赖 H03 确认 + B1-B5。性能与主流程证据目前均来自同引擎 headed Chromium + CDP；真实 webview 复测是其核心内容。
6. 4 个 real-codex/claude CLI 测试因缺真实二进制/凭证持续失败（改动前即如此，非回归）。
