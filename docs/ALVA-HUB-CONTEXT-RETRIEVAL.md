# Hub 引用优先、按需检索

2026-10-02 用户明确要求。架构原则见 ADR-0011；这里记录实现与验收，不将规划当作已上线。

统一链路：内容落盘 → 小索引/引用 → 按任务搜索 → 有界提取 → 相关片段进上下文。
模型上下文是工作集，不是附件仓库、MCP 工具目录或完整历史数据库。

| 内容 | 索引/入口 | 正文读取 | 当前状态 |
|---|---|---|---|
| 上传日志/文本 | 会话内文件名、字节数、相对路径 | `hub_context` 列表/文字定位/有界读取；既有工作区工具仍可用 | 已实现；不会把正文拼到 prompt |
| 图片/视频/二进制 | 文件引用 + 原始哈希/大小/处理清单 | `hub_media` 显式探测、压缩/转码/抽帧、选择与 receipt | Hub 根 ALVA 已实现受支持格式的有界处理/选中图单请求投递；不是自动语义分析/全视频或音频输入 |
| 所有工具的大结果 | 调用身份、成功/失败、耐久记录引用 | `context_retrieval` 搜索/分段读取 | Hub Desktop/server ALVA 与其 app-core 递归子树已实现；其他 engine 自有循环不受该策略控制 |
| MCP | 简短 server 索引 + 一个 `mcp_runtime` | 有界搜索、一个选中方法契约、精确调用；结果先落盘再引用 | Hub 受管 ALVA 已实现；legacy direct-tool caller 保持兼容 |
| 历史消息/工具记录 | native record ID；Hub Turn/Attempt/branch 分组和来源 SHA | native `context_retrieval` 与 root `hub_context` 关键词/时间/类型/来源筛选 | ALVA 可检索当前 HubSession 已存的跨 branch/engine 可见记录；不扫其他会话/Worker/全局或外部 CLI 内部记录 |

## 附件（当前）

输入只接受用户选定的文件字节，不接受服务端宿主路径。每 Turn 最多 8 文件，
每文件 20 MiB、总计 40 MiB；二进制以 base64 传输、后端恢复原字节，HTTP run
请求限额 64 MiB。图片/视频可上传，本地支持的格式可预览，但不冒充模型理解。
附件有私有 SQLite BLOB
权威副本，并以唯一文件 ID 写入 session cwd 下
`.alva/sessions/<HubSession>/attachments/<id>/<filename>`，不覆盖项目现有文件。
逐级 no-follow、create-new、Unix 新目录 0700/文件 0600，拒绝路径穿越和受保护文件名。

模型只拿文件名/大小/相对路径、处理清单和“按任务搜索、限量读取”的提示，不读正文生成
prompt。大日志、同名上传、Unicode、符号链接绕过、重启/跨会话预览需有回归。
项目模式会在用户选择的工作目录新建这个会话子目录；其他项目源码不改。
这不是同一用户/共享工作区内多个 engine 之间的 OS 隔离，不能宣称共享 cwd
的文件对其他工作区 Agent 不可见。UI 预览始终读会话归属校验后的耐久副本。
当前失败可能留下只由本次上传创建的副本/空目录；不隐式清理用户目录。

## 所有工具与媒体处理

所有工具共享“原文保留、引用优先”规则，不只 MCP。工具的成功/失败、权限请求和
副作用证据不能被引用层吞掉；短状态和用户主动检索的有界片段允许直接返回。
否则读取工具的结果再次变引用，会形成永远读不到内容的循环。大结果进入 artifact，
模型收到调用身份/有界摘要/引用，只有需要的片段或已选择媒体产物进入工作集。
在 Plugin/Middleware/产品 adapter 处实施，不给 kernel 添加结果存储政策。

当前 `ContextRetrievalPlugin` 复用同一个 native `SessionRepository`，不另建 artifact
日志。发模型前仅修改请求副本：普通 ToolResult 文本超过 4096 UTF-8 字节或含
`ToolContent::Image` 就转引用；`mcp_runtime.call_tool` 即使短结果也转引用。事件和
权威记录不因投影变短，error/id 保持。引用包含 record ID、调用身份、原始文本字节数、
有界媒体类型及来源哈希；重复 call ID 按实际消息身份定位，压缩保存的引用再次投影
时验证来源，避免引用链。原记录已按现有规则脱敏，并非未经脱敏的私人原件。

投影 priority 在现有 CompactionMiddleware 之前，压缩计数和 summarizer 只看引用；
当前 Hub BaseAgent 没有另装会在此之前执行的自定义 ContextSystem。只豁免本插件
注册的 `context_retrieval` 的最多 16 KiB 有界提取，不豁免普通 read_file/MCP。
源不可定位就停止该模型请求；引用读取遇到失效/损坏 blob 明确失败，不回灌原文。
blob 解压限 64 MiB、压缩输入限 65 MiB，校验大小/hash，SQL 页先读 metadata 再按预算
解压；媒体类型索引最多 16 项，每项最多 128 字节，并明确截断。

覆盖当前 Desktop/server 经共同 factory 构造的 ALVA native session（包括 Hub-owned
ALVA Worker）与 app-core `agent` tool 的递归子树；不是其他 engine 的工具循环政策。
Plugin 初始化发布 app-level `SpawnContextPolicy`，在 repository 创建实际 child session
后重绑 reader、middleware、首个 assembly hook 与短 prompt，而不是复制父 reader。
ContextHooks 先于通信 assemble 投影引用并重算 token metadata；来源失败不把 raw entries
传给下一个 assembler，锁存错误，provider middleware 明确停止请求。通信 hook 是可信
产品代码，仍可访问 ContextHandle；不是其沙箱。未新增 child 自动压缩器或替换默认 SDK。

各递归层的 schema、模板 inherit-all 与实际选择都以直接父 Agent 的工具子集为准，
不能经再次 spawn 拿回被移除的 root host 工具；孙 Agent 默认模型继承实际父模型，
不重新落回 root scope 保存的模型（显式 override/模板优先级不变）。空 host whitelist 下仍提供当前 child
的有界 reader（无文件/Shell/MCP/父历史权限）；孙会话使用新的自有绑定。独立 native
记录/typed 父子链接保留，child 可读自己的引用、压缩前历史并在重启后分段读取，父/
兄弟/其他 child 的 record ID 均拒绝。未注册该 optional Plugin 的 caller 不被加政策。

上传处理清单当前记录 `source_sha256`、原始/保存字节数、类型（签名探测，非成功
解码声明）、`compression_applied=false`、`transcoding_applied=false`、空的
`extracted_frames` 和 `delivered_to_model=false`；`delivery_scope=upload_prompt`
只表示本次上传未自动注入模型，不保证后续工具永远没读过它。UI 本地预览不是模型输入。

媒体处理 Tool 创建关联原始 artifact 的新产物与处理记录：

- 压缩/转码：工具和版本、输入/输出哈希与大小、格式/codec、分辨率、帧率、
  质量参数、是否丢音轨/裁剪等信息损失；原件不覆盖。
- 抽帧：视频时长、策略（时间均匀/场景切换/用户指定）、指定时间与真实时间点、
  每帧相对路径/尺寸/哈希及挑选原因；不能只说“抽了几张图”。
- 模型投递：明确本次读取/发送了哪些片段、派生产物或哪几个时间点的帧，
  哪些原件/范围未读取；不把本地预览或工具落盘当作模型已经看过。
- 失败/超时/取消：保留真实状态，不把部分产物假记成完整处理。ffmpeg 即使
  存在也不能说明当前产品已提供该功能；必须以实际解码、产物与接线回归作证。

验收：20 MiB 精确边界/总量限制、二进制字节/hash 重启一致；20 MiB 上传不增大
模型正文；视频帧策略/真实时间点、压缩损失说明、派生身份/原件保留、取消、
缺工具与模型媒体能力不支持均可观测。执行成本与媒体投递须遵循既有权限/用户任务。

### 当前 root 产品媒体链路（schema 24）

`SessionPluginFactory` 按实际 native root 创建 `HubContextPlugin`，生产 Desktop/server 在
Turn 认领后绑定同一个 HubStore/产品会话。`hub_media` 只接受 artifact/attachment ID，
不接受宿主路径或 URL；不自动扫描/处理/投递。全部动作注册进既有 SecurityGuard
HITL（包括元数据动作），PermissionProfile/Team 自治的既有授权规则不被绕开。

已实现动作：probe、process_image、extract_frames、transcode_video、list_artifacts、
read_manifest、select、receipt、cancel_selection。探测仅视频/图像 metadata，不声称理解。
支持 PNG/JPEG/WebP/GIF、MP4/MOV/WebM；其他格式、缺解码器/encoder、错误输入明确失败。
只从固定系统安装目录寻找 ffmpeg/ffprobe，不安装软件；当前 native 实现为 macOS/Linux。
解码输入来自校验过的私有原始 BLOB，新建 owner-only no-follow 快照并以只读 FD 传入；
快照在持有 FD 后 unlink，不动原始附件。只允许 file/pipe 协议、固定 demuxer，MOV 外部
data references 禁用；不继承宿主环境。不是 OS 沙箱，不宣称抵御同用户恶意进程。

最多 2 个处理 job 并发，输入仍 20 MiB；每 decoder 35 秒 wall/30 秒 CPU，单次分配
参数 64 MiB，来源 16 megapixels/8192 edge，输出/日志有硬字节限额；取消/超预算 kill/reap
本次子进程，future 丢弃 kill-on-drop。Linux AS 硬限 2 GiB；macOS 因原硬限制调用实际返回
EINVAL，改为每 50ms 监测 owned/unreaped PID 的 512 MiB RSS 守卫，可能短暂超出，不称硬上限。
原有单工具超时闸仍保留，长视频可能因此失败而非完成。

- 图片/帧：JPEG 最大边 512px、每张 ≤256 KiB、mjpeg q=4、有损/无裁剪；保留实际尺寸、
  source/output hash 和版本。只保留首个 decoded image frame，不假称检测了源动画；默认
  scaler/color/metadata 行为，不保证色彩/元数据保真，也不宣称隐私 scrub 或字节一定减小。
- 抽帧：一次 1–4 张；均匀策略在 duration×i/(n+1)，或显式时间点，最长 3600 秒来源。
  选择首个 decoded frame at/after request，并从 showinfo 记录真实 pts_time（解码器归一化
  时间线，不伪装容器绝对时间）；指定时间/实际时间/策略/选择理由均保存。无场景检测。
- 视频：前最多 60 秒、720px 最大边、5fps（可能丢/补帧）、H.264 target 400k，移除任何
  音轨、≤8 MiB；真实输出 probe 与范围/截短/损失明示。视频产物仍是文件，不作为视频
  原件/音频直接传给模型。每原附件最多 64 派生产物，新副本/BLOB 独立保存。
- 投递：固定产品 route 必须明确声明 images/vision，未知模型不猜。选 1–2 个派生 JPEG
  与理由，receipt 绑定产品/native root/tool call，快照来源/产物 hash、真实帧点与模型 route；
  请求尝试时追加实际 model_id/provider backend，配置路由不冒充实际调用身份。
  middleware 在普通引用投影/压缩完成后，向请求副本附选中的图片和 ≤16 KiB provenance，
  最多 512 KiB 图像字节，不回灌原始视频/文本/其他帧；conversation 仍仅保存选择引用。
  prepared 不是已发送；submitted 表示请求尝试；response_received 不证明视觉理解。
  provider 错误/中断明确保留 acceptance unknown，终态/重启不自动重放选择。每 native
  session 最多 128 receipts。未提交选择可取消；失败可留本次新产物文件/空目录，不隐式清理。
  上述字节预算不等同于每个 provider 的精确多模态 token 预算；图片在压缩后注入，极小
  模型窗口仍可能拒绝，receipt 按真实错误保存，不宣称所有 route 都能接收。

真实本地 ffmpeg fixture 验证图压缩、指定/均匀时间/实际时间、视频 codec/fps 损失、原件
保留/损坏/迁移/重启/越权/取消/输出限额。真实 catalog + AllowOnce + 模型替身验证零自动
输入→只选中图及 provenance→成功/错误 receipt→下一轮零重放。尚无派生预览的专门
Tauri/HTTP endpoint/UI、真实付费 provider 验收、音频转录/场景检测或外部 engine 循环适配。

## MCP（当前 Hub 受管 ALVA）

1. 以 Plugin 注册一个常量 schema 的 MCP 入口；初始 prompt 仅列已授权 server
   的简短能力索引，不将所有 tool schemas 注册进模型。保留旧产品配置的暂停/启用、
   固定 snapshot、连接预算、取消回收以及 HITL，不改稳定 SDK。
2. `search_tools` 按 server/query 返回有限命中（建议最多 20 条简短描述）；
   `describe_tool` 只取选定方法，不返回全库。超大 schema 返回契约引用，按路径
   提取必要字段。当前契约引用由通用策略处理，按文字范围读取；专用 JSON-path
   schema 提取尚未实现。列表具备分页/截断指示，不靠模型猜遗漏方法。
3. `call_tool` 精确核对已启用 server/method/arguments，并走原审批路径。
   请求、响应、错误和调用关联落在同一会话 artifact 中；输出给模型的是成功/失败、
   有界摘要与结果引用，不把大量结果或调用日志回灌到上下文。
4. Agent 可搜索/分段读取调用过程与结果，UI 可按引用读取全文和消息模块。
   原始记录保持权威；不执行返回的脚本/HTML，不把描述中的指令视为授权。
5. 当前 gateway 的固定 schema 只含 `list_servers/search_tools/describe_tool/call_tool`。
   目录按稳定 server/tool 排序，literal case-insensitive query ≤200 字符，默认 5 项、
   最多 20 项并有 offset/has_more；描述预览最多 96 字符，不返回整库 schema。
   调用只允许本轮已连接目录的精确身份，arguments 必须 object、入口 input ≤256 KiB。
   当前包括搜索/描述在内的 meta-tool 都遵循既有 HITL，不自动信任远端 read-only 标签。
   初始 server 索引最多 4 KiB/32 个 server，不含命令/env 或 tool schema。连接仍有原
   60 秒总预算和回收机制；初始化失败未注册 gateway 时不提示其可用。
6. stdio 保留 1 MiB 单 frame、单 schema 64 KiB、description 16 KiB 限额；目录扩为
   最多 8192 tools/128 pages，另加累计 32 MiB 上限，不以按需加载为由放开宿主内存。
   单 RPC 的大于 1 MiB 结果仍显式拒绝，20 MiB 文件上传不意味着放开 MCP frame。
7. 已用真实本地 stdio fixture 验证 5000 工具分页、一个 model-visible gateway、单工具
   schema、32 MiB 总目录拒绝、正常审批后的 600 kB 结果→引用→模型历史搜索，以及
   >1 MiB 通用工具原结果保留。typed MCP image/audio/video/blob 在文字检索中剥除字节，
   保留类型/URI/排除标识与结构化表格；不冒充已经分析媒体。任意普通工具文本中的
   未标类型编码不是媒体解码能力。未调用真实付费服务。

## 历史（ALVA native 与固定 HubSession 产品记录）

native 检索复用 SessionRepository 权威记录，不复制新的历史库。产品 root `hub_context`
复用 Hub Turn/Attempt、schema-20 可见消息日志、schema-19 工具全文及已保存的失败原因。
`context_retrieval` 提供当前 native session 固定作用域：关键词、
时间范围、消息/工具/错误类型；返回稳定引用、时间、类型、简短命中片段，再按
引用读取附近上下文。先实现确定性文字搜索，不以向量检索代替准确性。

当前预算：literal case-sensitive query ≤200 字符、每页 ≤20 命中、预览 ≤100 UTF-8 字节，
每次最多扫描 128 条 metadata/64 MiB 来源 JSON，读取 ≤2048 UTF-8 字节、完整工具响应
≤16 KiB。搜索 continuation 携带固定 `through` high-water，后来的追加不混入该快照。
`%_*` 等字符按字面匹配，不作为 SQL 模式；UTF-8 边界非法或预算放不下一个字符明确拒绝。
片段携带 source ID/范围/是否还有后文，绝不静默截断成完整答案。压缩/窗口溢出后
仍可检索，重启身份保持；跨会话/Worker/项目检索须明确授权，默认不扫全局历史。
检索结果是历史资料，不是当前指令。上传正文和历史日志不能自动进入 handoff
全文；handoff 应带经过现有脱敏闸门的相关引用/有界摘要。

只提供 user/assistant/tool 文本与 tool intent 参数；跳过重复 execution outcome 和内部
provider/LLM/hook 事件，剥除 hidden reasoning/typed 媒体字节。scope 同时绑定工具实例
和 execution context，模型不能传 session_id 越权。append-only 日志里的 compaction
投影副本仍是独立记录，尚未以 Hub Turn/Attempt 做跨 branch 去重/分组。

验收包含重复文本的不同身份、并行/失败 Attempt、超过模型窗口的历史、重启、
跨会话拒绝、全文可达、关键词特殊字符/Unicode、错误重试及明确检索预算。

`hub_context` 固定当前 HubSession 与实际 native root execution context，不接收 session_id；
root 工具复制到子 Agent 也会拒绝，Hub-owned Worker 有自己的产品绑定。跨 branch/engine
只搜索此会话已保存的 visible data，不恢复外部 CLI 未保存全文，不读取 reasoning/内部
provider 轨迹，不自动进 handoff。以 Turn/Attempt/branch/engine/status 分组，可用时间/类型
和各 ID 筛选；工具时间是 attempt_start，旧摘要是 turn_created，不伪造精确调用时间。

预算区别于 native reader：每次最多 128 逻辑来源/64 MiB hydrate、3 命中/100-byte previews、
512-byte UTF-8 read 和 4 KiB 完整 response。按 user/assistant/tool/legacy_assistant/error
来源再按 sequence 分页，五个固定高水位；快照固定行，不固定可变终态字段。assistant
按 attempt/原 item 重组至多 65536 片段并检查 UTF-16 连续位置，跨片段关键词不漏；tool
全文跨 16 KiB 块匹配并核对大小/数量。旧无 journal 的 final_result 明示 legacy summary，
不冒充丢失全文。引用带 source SHA，文字 view 排除 typed media 并整体脱敏；来源改变/
损坏/越权明确失败。文件列表只读索引，read_file 仅显式读取已验证原 BLOB 的脱敏片段。

## 不变约束

- UI 展示完整记录与模型是否读取记录是两件事；不能用“节约上下文”隐藏事实。
- 每个引用可追溯、可按权限读取；丢失/不可用显式失败，不能把引用当作已读正文。
- 不为了方便把私有数据复制进全局 workspace，或给 engine 新增宽泛宿主授权。
- 上述原则覆盖将来的图像、图表、生成产物和外部工具；具体读取能力需单独实现。
