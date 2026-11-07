# 横向支撑 · Native process engine adapters
> Claude/Codex/Pi完整外部harness的原生子进程适配；Codex app-server与Pi受控RPC各自保留协议owner，不是Provider包装。
## 地位
原生外部 Agent 适配器：消费 integration/engine 根契约、程序必传的storage.BindingStore、owned platform/system/process 与私有文件能力；不依赖 Hub 产品状态或 Rust 桥。Claude/Codex/Pi 当前在本 package 按 Vendor 分派共享实现，不宣称一对一迁成三个独立 adapter 包，也不是 SDK 层。
## 逻辑
显式批准宿主/账户访问后启动指定程序；Claude/Pi/Codex exec使用私有HOME与显式env，Codex app-server继承本机环境。stdout/stderr/stdin经完整行framing、有效环境秘密及共享known-field脱敏落私有有界journal，产品只收事件/引用。stderr单行超1MiB正文省略并失败，不能保存截断秘密。原生result与真实进程结束共同决定终态，取消kill/reap，Unknown vendor帧不触发执行。Pi只恢复匹配header的确切文件，无关启动中断的空/残缺header不阻断其他已绑定会话；目标缺失仍失败。
Codex/Claude/Pi可显式选择absolute已存在的CLIConfigDirectory，经canonical目录验证后只传对应CODEX_HOME/CLAUDE_CONFIG_DIR/PI_CODING_AGENT_DIR给CLI；完整配置由CLI读取，不宣称仅复用登录。Alva不读取、复制或链接凭据，Env不能覆盖profile变量。Codex app-server按Cindy保留本机HOME、工具链和hook环境，再单独设置CODEX_HOME；未选择配置时该目录仍为产品私有，默认AccountSource明确区分native环境与私有配置，显式目录仍绑定canonical SHA256。其他vendor/exec保留私有HOME；Pi的显式全局配置仍受既有禁extension/MCP/Skill/context等启动选项约束。持久非空request来源不匹配时在进程IO前拒绝；Codex exec fresh/resume都在父层固定read-only sandbox。
app-server环境合并对照makecindy/cindy固定修订5f9a219d8d789ac96de779e17c61b8126845f1bd的env-builder.ts、shared/terminal-output.ts及desktop/runtime-configs.ts：继承后叠加显式Env和CODEX_HOME，补缺Python/纯文本终端默认值，合并NO_PROXY/no_proxy及本机地址；不写信任文件、不添加PATH包装或工具链资源设置。仅已知敏感键的有效值与显式Env值进入脱敏集合，公开HOME/PATH不被当成秘密。环境秘密使用run-local有界增量过滤，原始delta/未知片段和stderr正文省略；完整message/tool/result/request/reply保留经脱敏的可恢复事实及大结果引用。人类秘密答案继续使用后述更严格的整段省略政策。
Codex仅在Transport=app-server时启动双向JSON-RPC stdio，依次initialize/initialized、thread/start或exact resume、turn/start；不因协议失败退回exec或重放prompt。空值/exec保留原打印协议。CodexSandbox配置仅readOnly默认或显式workspaceWrite；thread sandbox分别映射read-only/workspace-write，turn sandboxPolicy使用readOnly/workspaceWrite（canonical cwd为唯一写根，networkAccess=false且排除tmpdir与/tmp默认写根）。CodexApprovalPolicy配置仅onRequest默认/unlessTrusted/never，分别映射on-request/untrusted/never，均非Alva权限profile。此transport声明permission/user-input callback、typed FileInputs与SkillInputs；Codex图片走localImage，其他文件只传独立原件/文本派生path index供native read/search。显式选中Skill仅核对所给canonical SKILL.md的regular/SameFile/1MiB/SHA256，再走native skill{name,path}输入；不扫描全局Skill目录。Codex exec与Claude print明确拒绝typed files/Skills；Pi print仅支持typed files。执行审批与人类问题分别关联native request ID，string/number身份不混用。未知server request明确拒绝，未知notification仅有界记录；resume中同bound thread的旧turn usage通知只记journal，既不失败新turn也不覆盖新usage，server request/tool等身份检查不放宽；秘密问题答案在发送前加入有锁/有界的显式脱敏值，stdin答案正文省略；之后stdout/stderr原始body整体省略（保留省略字节与处理事实），MessageDelta/ReasoningDelta不进入产品事实，防止跨帧重组。完整Message/Tool Input/后续问答与审批payload经共享literal脱敏再投递，保留已完成安全正文；问题仅投影字符串字段，保留is_secret/is_other等typed控制，支持连续秘密问答；涉及秘密的question ID/choice拒绝并收尾原进程，不改写native路由或答案。pending响应一经消费，任何发送失败/取消都使原RPC失败并关闭真实进程，不留下无法回答的native等待、不自动重放。取消先在已知turn上interrupt，再关闭输入、kill/join真实进程，不以EOF或退订宣称完成。cleanup失败保留实际process owner与所选binding store供显式Close重试。
已验证owner/cwd的resume身份可独立限定header到达前的journal引用；它不预填mapper原生session、不产生SessionStarted。实际CLI header仍经原有精确绑定检查，取消须真实join且保留cancelled和强日志失败的区别。
Pi JSON以agent_settled为本轮原生完成事实；agent_end仅结束底层run，即使willRetry=false仍可能继续overflow recovery，不冒充成功。assistant error/aborted/length保留失败，后续真实恢复的完整message可清除临时失败；settled后的执行帧拒绝。Pi空/exec仍为只读print，审批/问答unsupported；Transport=rpc才启用可编辑配方，保留私有HOME与所有自动发现禁用项，仅显式注入应用自带固定SHA私有extension。先get_state与每run nonce ready握手，核对exact private session文件/header/cwd后才prompt；未ready/身份失败零模型请求。原生tool_call在真实execute前等待本应用confirm，read/grep/find/ls保持原生读取，bash/edit/write每次独立请求实际批准，不以Ask/Auto/Full header自动放行。ask_user仅提供单个自由文本问题；问答/审批分别走既有Hub owner，native UI ID加run前缀隔离迟到响应。pending消费后发送失败收尾原进程，不重放；Stop撤销dialog、先abort再真实kill/join，未完成保留owner。所有自由回答视为可能含秘密，stdin正文省略，之后native原始输出整体省略/产品delta抑制，完整安全message继续脱敏投递。typed Skill仍明确unsupported。
Pi两种transport均支持typed FileInputs：选中原件/文本派生只发独立path index；print图片走官方@absolute-path，RPC走原生images字段（最多两张/每张20MiB，总计40MiB）。启动前复用流式SHA/size/SameFile重验，图片实际读取再核对有界字节/hash，不扫描或解析Prompt中的@。RPC以显式64MiB framing容纳native重复图片记录，journal仅保留media形状/省略字节数；选中原件与native session保留实际内容，不复制多份base64到日志。所有路径须实际cleanup complete后才发Completed；失败保留原process与binding owner供Close重试。
Pi RPC完成正文以单个有界aggregate先整体脱敏；≤64KiB发真正完整Message，超过时才发诚实MessageDelta分块并由Completed收尾。已完成安全分块可穿过秘密回答后的raw-delta抑制；原始native delta继续抑制，不能先切块再逐块脱敏。普通已投递delta用当前message的字节数/增量SHA核对安全前缀，仅补未投递后缀，不重复旧正文。Pi message_end（包括error/aborted/length）释放累计正文与投影前缀；替换message_start也释放旧缓存，活跃累计正文保持1MiB上限。print/Codex/Claude的既有完成映射不因此改变。
Claude result同时检查subtype与is_error；零退出不能覆盖native失败。已知session不得在result或尾随帧中变化，result后的执行输出拒绝，纯system通知可继续读至真实进程结束。此逻辑仅用项目fixture子进程测试；不启动用户Claude程序或账号。
## 约束
测试不隐式发现用户CLI或读取账户凭据；显式opt-in的已安装Pi仅使用全新private配置和项目localhost固定HTTP响应，不能冒充真实模型/账号验收。Claude默认safe-mode/dontAsk，Codex只读sandbox，Pi禁用extension/MCP/skills/templates/theme/contextfiles自动发现、offline启动；CLI安全标志和tool_call门控均不是OS沙箱。没有permission callback就明确unsupported。三vendor native身份均绑定product owner/canonical cwd；未知/foreign resume拒绝。Pi仅读取私有目录JSONL精确header/cwd，传确切文件路径，不让session-id缺失时新建冒充恢复。日志编码/脱敏失败锁存，后续帧或取消不能把不完整journal恢复成成功。日志/帧/总时限超限明确失败，不截断成成功。
Adapter.New不创建engine.sqlite，缺失Bindings在IO前拒绝；上层成功装配后移交Close所有权。原SQL实现已归platform/storage/bindings，stock程序显式选择；测试全部经opaque接口句柄跑真实子进程和重开。供应者Close失败保留原owner供显式重试。
已知观察边界：process Descriptor目前没有Inspector，engine-journal引用仅定位其私有原生日志，不是已接通的通用UI/CLI原件reader；Codex大结果也不因此宣称可经Inspector读回。CLI的精确native record reader仅用于Alva；Pi大型安全答复通过已提交MessageDelta全文聚合读取，不伪造精确Reference或把片段标作完整Message。

## 业务域清单
| 文件 | 职责 |
|---|---|
| adapter.go | 显式程序/argv/env、owned I/O生命周期、私有日志与终态 |
| descriptor.go | 独立保守能力声明；不把CLI安全flag冒充等价权限profile，Claude真实append-system-prompt/max-turns，Codex/Pi明确拒绝，不声明native Inspector/provider headers/fast mode |
| codex_app_server.go | 显式app-server握手/thread/turn与owner/cwd绑定；启动前有界流式重验原件/派生大小/SHA/文件身份及选中Skill，不把路径或Prompt当授权；配置隔离与interrupt/真实join |
| codex_rpc.go | 双向JSON-RPC stdio、有界framing/backlog/请求关联、失败清理及禁止自动重放 |
| codex_events.go | 原生item/tool/result与approval/question独立映射、pending/resolved关联和native policy校验；人类答案正文不入journal |
| codex_environment.go / codex_environment_test.go | app-server专用本机环境合并、终端默认值、代理去重及已知敏感键快照；不读取真实配置/认证/信任文件 |
| codex_output.go | run-local有界跨片literal脱敏和完整native日志可恢复性；与人类秘密答案的严格省略区分，回归见codex_environment_test.go |
| codex_app_server_test.go | 仅owned helper process验证双向审批/连续秘密问题/迟到响应、native身份、帧错误、真实interrupt/exit、日志及有界native政策 |
| mapping.go | 各vendor真实JSONL方言、tool配对、可恢复错误与结果证据；已验证resume引用来源独立于CLI session确认 |
| selected_files.go | Codex/Pi共用的明确选中文件流式SHA/大小/SameFile重验，不新增文件权限 |
| pi_events_test.go / pi_files_test.go | Pi1.4 settled/overflow恢复、abort/length及零退出失败、已结束消息缓存零累计/活跃1MiB上限、typed图片argv/原件派生path index与真实工具事实回归；只用自有helper进程 |
| pi_bridge.go | 固定内容SHA私有Pi extension；tool_call执行前独立批准、自由文本ask_user与run nonce ready，不开启其他extension发现 |
| pi_connection.go / pi_connection_test.go | 独立Pi RPC命令关联；队列只持media省略后的投影，最多64条/合计8MiB，receive释放准确字节，response同受8MiB约束；显式64MiB framing与失败零重放 |
| pi_rpc.go / pi_events.go | 精确native文件身份、ready前零prompt、typed输入、逐工具审批与自由问答、run绑定响应、Stop撤销/abort/真实join和安全产品投影 |
| pi_rpc_test.go | 自有真实子进程验证批准前零写/拒绝零写、问答脱敏、20KiB/80KiB完整安全答复和跨16/64KiB秘密、已发布delta不重复、missing bridge/foreign身份、迟到响应、Stop/重开恢复、大图片及大行读取消 |
| pi_native_test.go | 仅显式ALVA_PI_TEST_NODE/SCRIPT启用真实已安装Pi+应用extension+localhost SSE fixture；真实agent-loop验证write拒绝/允许及ask_user回答回到下一模型请求，无真实账户/远程模型 |
| sessions.go | 经共享BindingStore核对owner/cwd，Pi私有原生header与真实join后关闭所传组件 |
| private_other.go / private_windows.go | Pi文件的Unix权限/Windows DACL与SameFile检查；Windows USERPROFILE/AppData/TEMP显式绑定私有home，拒绝大小写profile override |
| adapter_test.go | 三vendor真实本地fake子进程、配对/错误/取消、日志、header前取消的引用来源、精确resume授权与跨实例Pi恢复/foreign拒绝 |
| profile_test.go | 明确配置目录验证/来源拒绝、真实本地child环境隔离及Codex fresh/resume只读参数回归；不读取账号 |
| claude_result_test.go | 仅本地fixture子进程验证success+is_error、冲突session与result后输出不能伪报成功；允许尾随纯通知 |

Pi1.4.0对照[官方固定revision](https://github.com/earendil-works/pi/tree/4a080252f06caf63a525d7873c5d94d3c23887e0/packages/coding-agent)的`src/cli/args.ts`、`src/main.ts`、`src/config.ts`、`src/modes/print-mode.ts`、`docs/json.md`（2026-11-05）；getAgentDir优先显式PI_CODING_AGENT_DIR，否则为homedir/.pi/agent；Alva本身不读其内容。仓库已由badlogic/pi-mono迁至earendil-works/pi。fixture不冒充用户CLI/真实账号验收，后者须单独报告。
Pi RPC门控另核对同revision的`src/core/agent-session.ts:_beforeToolCall`、`packages/agent/src/agent-loop.ts:prepareToolCall/executePreparedToolCall`、`src/core/resource-loader.ts`与`src/modes/rpc/rpc-mode.ts`：异步tool_call先于真实execute；noExtensions只加载明确extension；ctx.ui.confirm/input经extension_ui_request/response等待；abort等实际idle。2026-11-05真实本机Pi1.4.0离线get_state握手及双分支本地HTTP fixture/race通过，不读取已有账号目录、不发远程模型请求。

Claude选项对照[官方CLI reference](https://code.claude.com/docs/en/cli-reference)（2026-10-17）：--append-system-prompt与--max-turns仅作为显式argv；原safe-mode/dontAsk保持，helper子进程验证真实argv和error_max_turns失败终态。无付费账户或已安装用户CLI验收。
Claude结果字段与尾随通知对照[官方TypeScript参考](https://code.claude.com/docs/en/agent-sdk/typescript#sdkresultmessage)和[agent loop](https://code.claude.com/docs/en/agent-sdk/agent-loop#message-types)（2026-10-24）；不以SDK文档宣称已完成双向控制协议适配。

Codex app-server对照[官方协议文档](https://learn.chatgpt.com/docs/app-server)（2026-10-26）：JSONL stdio/initialize、thread start/resume、turn start/interrupt、item事件、requestApproval与requestUserInput、serverRequest/resolved。仅项目自有helper子进程与race检查，不声称已完成用户CLI/登录账户、外部模型、真实平台或GUI业务验收；不读取账户文件、不调用login/auth接口、不启动真实Claude。
实际wire枚举与必填字段以[openai/codex固定revision的generated v2 schema](https://github.com/openai/codex/tree/970111f6cb62cc02fe36a82ea7d71b37a2889c97/codex-rs/app-server-protocol/schema/typescript/v2)核对（2026-10-26）：AskForApproval、SandboxMode、SandboxPolicy、ThreadStartParams、ThreadResumeParams、TurnStartParams、UserInput与ToolRequestUserInputQuestion/Response；不把文档示例中的camelCase policy示意直接当wire枚举，不做失败后换拼写重试。
2026-11-05复核本机codex-cli0.165.0对应[官方固定schema](https://github.com/openai/codex/tree/360319d8dee62d87e2d59fc0c21805d79519cfed/codex-rs/app-server-protocol/schema/typescript/v2)的ThreadStartParams、ThreadResumeParams与TurnStartParams，当前thread/turn请求字段仍适用；版本/源码核对不冒充已完成真实模型调用。
app-server可能多路复用native child thread；非root通知仅进有界原生日志，不能改root身份/tool配对/终态，非root server request明确拒绝。此处不从native字符串推断产品typed child ancestry或审批授权；真实child交互中介尚未实现。
