# 横向支撑 · L4.5 Engine 根契约与产品适配
> Engine 是完整 Agent harness，不是模型 Provider；Hub 只消费此层的执行、独立检查与声明契约。
## 地位
仅本目录根契约包属于 L4.5：不依赖 SDK、harness、platform/storage 或 apps。具体 adapter 在独立子包，不能反向成为 Hub core 的依赖；整个 integration/engine 容器不因此成为 SDK 层。
## 逻辑
Execute 的 context 属于执行所有者；Cancel 只是请求，真实 Completed 才收尾。Error fatal 后仅允许 Completed，工具必须配对，大结果用 qualified native reference。
Event.Truncated 只允许 Message/ContentBlock/ToolEnd：Text 仅是有界前缀，带 Reference 时完整内容经 Reference 读取，否则前缀即全部可得正文；截断事实以该字段为准，不靠文字后缀。
Event.Display（ToolDisplay）只属于 ToolStart/ToolEnd 及请求执行该调用的 PermissionRequest：按 ADR 0013 §3 由各 adapter 把自己的工具名/item 类型归一成 kind（command/read/edit/search/subagent/todo/other）及命令/退出码/状态/输出只在轨迹（OutputInTrace）、路径/增删行数/有界 diff、关键词/命中数/结果、任务/子会话/状态、待办项、Output 引用；各字段有上限且编码后≤4KiB，越界、kind 不符或格式错误由 StreamContract 拒绝；BoundToolDisplay 的输出对任何字符串（NUL、非法UTF-8、控制字符）都必能通过校验。adapter 须先对未截断的 Display 逐字段脱敏（MapToolDisplayStrings），再 Bound，避免截断点留下秘密前缀。消费者只按 kind 呈现，不认工具名或引擎名。
Usage为包含式计数（缓存读/写都计入InputTokens，同kernel wire.Usage）；CachedInputTokens/CacheWriteInputTokens用指针区分“未报告”与“报告了0”，Usage.Add累加时只要有一部分没报告，合计就视为未报告（避免混合求和压低命中率）；StreamContract拒绝缓存数（两者之和）超过输入数。InteractionResolved可带可选Resolution：适配器自己转发的审批决定（Allowed）或回答（只含用户从提供选项里选中的标签；自由文本与秘密回答只记为已回答、值为空列表，不存正文；空map=显式跳过），只属于InteractionResolved、按kind校验且有界（≤16题×16值、每值4KiB、编码≤16KiB），不授予任何权限；旧事件无Resolution。WaitCheckpoint.Calls列出让出到该检查点的工具调用（≤Requests、每个≤256字节），这些调用的工具结果只表示等待，不是失败。
ExecutionActivity独立表示native build/prepare启动前事实，必须先await耐久report再启动；ToolIntent.NoSideEffect不覆盖这些工作，Activity不冒充ToolStart/End或完成。
DurableWaits能力必须由实际factory声明；Completed(waiting)只在SessionStarted后携带合法revision/数量/到期receipt，结束本次执行但不声称任务成功。WaitRevision续跑不含新prompt，具体checkpoint/body仍由产品绑定原生存储读取。
## 约束
不制造可恢复 native ID，不静默忽略不支持的选项。PermissionCallback/Resume 能力如实声明。模型/账户配置由显式 assembly 注入，不读取用户旧配置或凭据。
## 业务域清单
| 文件/目录 | 职责 |
|---|---|
| contract.go | 统一 Request/Event/Runtime/能力与流契约；Truncated 有界前缀标记只属于最终消息、内容块与工具结果；Display 只属于工具事件与其PermissionRequest并经校验；Usage缓存计数（指针区分未报告/0、Add累加、Validate不超输入）；InteractionResolution只属于InteractionResolved并按kind有界校验（Validate导出供适配器降级用） |
| context.go / context_test.go | M4 上下文：ContextUpdated 事件只携带 ContextReport——ContextUsage 是 provider 实测的最近一次请求（含缓存输入+输出，WindowTokens=0 表示未声明，Parts 只是按估算占比分摊、必须加总等于实测）与 ContextCompaction 结果（compacted 需覆盖范围与摘要记录引用；failed/skipped/summary_corrupt）；Capabilities.ContextCompaction、Request.CompactContext/ReportContext 与之成组；拿不到数据的引擎不报告、不估算 |
| tool_display.go | stdlib-only ToolDisplay/ToolKind/ToolDiff/ToolMatch/TodoItem、MapToolDisplayStrings 逐字符串字段映射、BoundToolDisplay 裁剪（UTF-8安全、按kind清字段、超预算依次丢结果/待办/diff/plan）、Validate，以及 adapter 共用的 LineCount/LineChanges/UnifiedDiff/Checklist |
| tool_display_test.go | 只在工具事件与PermissionRequest、越界/错kind/坏状态拒绝、敌对字符串（NUL/非法UTF-8/控制字符/超长）进所有字段时Bound后必过校验与契约、字符串映射覆盖所有字段、行数/diff/清单辅助 |
| options.go | stdlib-only可选生成参数、数值校验和复制；adapter显式声明支持，不静默忽略 |
| wait.go | stdlib-only可选WaitingRunner及有界checkpoint收尾回执（含让出的工具调用Calls），不依赖harness/产品存储 |
| descriptor.go | 独立Runtime/Inspector装配、保守配置/权限关系/安装声明；不探测宿主或账户 |
| inspector.go | stdlib-only检查契约：bounded向前/向后metadata（Next/HasMore恒为向前含义，Before/HasEarlier只由向后页设置）、RequestID/RunID/CallID/Status与工具起止时间、BackwardPages能力声明（未声明者可能忽略Before，消费者不得向后翻页）、text/typed child/背压观察/显式JSONL，不含执行或删除 |
| contract_test.go | stream终态/配对、字段身份、typed event与Truncated归属边界回归；工具参数16KiB/文本64KiB事件上限；用量缓存未报告/报告0/累加/混合视为未报告/超过输入被拒；Resolution归属与按kind/上限校验；Wait.Calls边界 |
| alva | 消费完整 L5 harness/SDK 的 Alva 产品桥接，不是轻契约或 SDK 包 |
| process | 按 Vendor 分派 Claude/Codex/Pi 的共享原生 JSONL adapter；装配期ProductInstructions只走经源码确认为追加的原生通道（目前仅Claude），Codex/Pi不注入，不经Request；显式CLI配置与私有HOME分别保留；Claude结果失败/身份/尾随帧仅fixture验证，不调用真实账号；不冒充三个独立 package |
| input_files.go / input_skills.go | 显式已捕获文件及所选Skill的stdlib DTO/校验/复制；不读文件、不发现配置或授予目录权限 |

Request.RetainWorkspace is an optional nonserialized trusted host capability: native ToolControl adapters may extend the actual admitted workspace lease for explicitly application-owned background resources, releasing only after physical reap. It grants no tool permission; sealed attempts reject late retention. Opaque external engines do not receive it.

Descriptor仅声明实际Go配方：显式option-aware Alva factory与Claude print已支持bounded SystemPrompt/MaxTurns；Codex/Pi不把system role降为user文字，fast-mode未接线就不声明。未注册Inspector时明确unsupported，不借用Alva存储。Scope由可信产品branch/typed chain派生，owner/native ID不是UI授权输入。

M2 增量：Request.TakeInterjections/ReportDelivered 由 Alva 适配器绑定（同 ReportActivity），ForkPoint 增 OwnerID（原生会话按产品会话分属时用其打开来源）。

M2 分叉锚点：Event.NativeTurn（Completed上引擎自己的turn id）、ForkPoint.AnchorTurn/AnchorKeep/BeforeSequence/ThroughSequence 都是记录意图时固定的绝对锚点；ErrForkUnreachable 表示切点已不可达且未报告任何事件，产品据此就地降级。Codex 按 turn id 在只读 thread/turns/list 中查找，找不到即不可达。
