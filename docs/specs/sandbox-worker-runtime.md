# 沙箱化 Worker 运行时——wasm/OS 两档隔离 + 统一审批升级通道

> Status: Accepted（2026-07-14 grilling 定案，测试缝已确认）
> Date: 2026-07-14
> Related: [sandbox-runtime-abstraction.md](./sandbox-runtime-abstraction.md)（L3 Sandbox trait 抽象，本 spec 的执行后端接口可复用其方向）

## Problem Statement

alva 的产品形态是 orchestrator（强模型规划）→ worker（廉价模型执行）。今天派活给 worker 时，worker 拿到全套工具（含 shell）、零隔离地跑在真实文件系统上。worker 模型的能力与对齐质量参差不齐，且干活时会接触不可信内容（网页、第三方代码中的提示注入）。**安全性不能委托给一个问号**——不能等出事再挽救，隔离必须是事前设计。此外，现有子 agent 以空 middleware 运行、绕过 HITL 审批，这是一个今天就存在的洞，与沙箱项目无关也必须修。

## Solution

给 worker 提供两档隔离运行时，orchestrator 派活时按任务形态选档：

- **wasm 档**：纯文件加工类任务（调研、文档/代码分析、批量编辑生成）。worker 跑在 wasm 沙箱（wasmtime + WASI）里，能力全部由宿主按 job 显式授予：文件=preopen 目录映射（能力模型，授权外路径"不存在"）、网络=域名白名单（默认全拒）、LLM 调用=宿主代理（API key 永不进沙箱）。批量操作靠内嵌 QuickJS 的 `run_script` 工具，一次调用完成 N 文件修改，token 成本与 shell 同量级。
- **OS 沙箱档**：确需本机工具链（shell/git/构建）的全流程任务。macOS Seatbelt / Linux Landlock 按 job 生成只含授权路径的 profile，shell 存在于隔离边界之内。

沙箱 worker 遇到能力缺口时走**升级通道**：升级请求是一个普通工具调用，复用现有 `PermissionMode` 审批体系（Interactive 人批 / Auto 分类器审 / Bypass 信任直干），不发明第二套平行审批机制。

## User Stories

1. As an orchestrator 用户, I want 派给问号模型的任务默认跑在沙箱里, so that 我不必把安全性建立在对模型质量的猜测上
2. As an orchestrator（强模型）, I want 派活时按任务形态声明隔离档位与授权目录, so that 纯文件任务拿到最小世界、重任务拿到受控工具链
3. As an orchestrator 用户, I want 用一个旗标临时授权若干目录给 worker, so that 授权随 job 生灭、不留常驻权限
4. As a worker（廉价模型）, I want 沙箱内有完整的文件增删改查能力, so that 我能独立完成分析、重构、生成类任务
5. As a worker, I want 一个 `run_script` 工具执行我写的 JS, so that 批量修改 200 个文件只花一次工具调用的 token 而不是 400 次往返
6. As a worker, I want 环境说明（挂载点、可用工具、无 shell、结果交付方式）在会话开始就常驻上下文, so that 我不浪费轮次探测环境或重试不存在的路径
7. As a worker, I want 授权外访问返回明确的"不存在/被拒"错误, so that 我能立刻调整策略而不是盲目重试
8. As a worker, I want 遇到必须跑测试/构建的环节时发出升级请求, so that 我不因缺 shell 而卡死，重活由宿主代跑并把结果喂回给我
9. As an orchestrator 用户, I want 升级请求在 Interactive 模式下弹给我逐条批准, so that 长尾命令永远有人把关
10. As an orchestrator 用户, I want 升级请求在 Auto 模式下由分类器自动放行安全命令, so that 深夜批量派活不被我卡住
11. As an orchestrator 用户, I want Bypass（信任）模式仅在沙箱兜底时可用, so that "直接开干"的前提永远是隔离已就位
12. As an orchestrator 用户, I want 子 agent 的每一次工具调用都经过审批中间件, so that HITL 不再被空 middleware 静默绕过（现存洞）
13. As an orchestrator 用户, I want worker 的 LLM 调用由宿主代理、API key 不进沙箱, so that worker 被提示注入也偷不走我的凭证
14. As an orchestrator 用户, I want 按 job 给 worker 开域名白名单, so that 联网任务可做、SSRF 与数据外流不可做
15. As a worker, I want 在 `run_script` 的 JS 里用 fetch 访问白名单域名, so that 抓取参考资料这类任务能在沙箱内闭环
16. As an orchestrator 用户, I want job 日志按工具调用粒度记录 worker 行为, so that 批任务不需要 token 流也有进度与审计可见性
17. As an orchestrator 用户, I want 沙箱档位与 jobs 体系（submit/wait/status/result）无缝组合, so that 已有的派活工作流不需要重学
18. As a 未来的远程部署者, I want 文件后端是接口而非写死的本地目录, so that 同一个 worker 将来可跑在别的主机上、文件走 WebDAV/云盘而 worker 代码零改动
19. As a skill 作者, I want 沙箱环境说明作为 bundled skill 维护, so that 环境用法随运行时版本演进而不散落在代码字符串里
20. As a 项目维护者, I want QuickJS 集成锁死为"无模块系统、无 npm"的冻结面, so that 这笔集成税是一次买断而不是追着上游跑的活边界
21. As a 项目维护者, I want wasm 相关 crate 的编译完整性由 CI 防火墙持续钉住, so that 沙箱地基不被无关改动悄悄破坏

## Implementation Decisions

- **升级通道复用 `PermissionMode`**（alva-agent-security 的 Interactive/Auto/Plan/Bypass），升级请求建模为普通工具调用进入审批流。不做专用白名单机制。Bypass 的既有语义"requires sandbox"保持并成为档位自洽的一部分
- **共享地基先行且串行**：修复子 agent 空 middleware 绕过 HITL 的缺陷 + 升级请求接入审批体系。这是安全关键路径，先于两档任何一档
- **两档并行建设，手册模式执行**：主会话产出实施手册 → fresh-context subagent 分头执行 → 主会话独立复核（本项目 PR-11/PR-13a 已验证的模式）。OS 档动手前可能需要一轮 Seatbelt spike 探明 profile 生成
- **wasm 档目标为 wasm32-wasip2 + wasmtime**，宿主内嵌于 CLI。现有 19 crate 的 wasm32 编译不变量保持，防火墙增加 wasip2 目标；文件工具的条件编译从"wasm 全砍"放宽为"wasm 且非 wasi 才砍"
- **文件访问 = preopen 能力模型**：宿主把授权目录映射进沙箱，授权外路径在 guest 世界不存在（无检查点可绕）。已用本机实验证实 CRUD 全套可用、绝对路径与 dotdot 越狱均被拒
- **文件后端留在 `wasi:filesystem` 接口后面**：本地 preopen 只是一种实现，WebDAV/远程后端不现在做但设计不堵死
- **`run_script` 内嵌 QuickJS**（维护中的 Rust 绑定），scope 锁死：无模块系统、无 npm、无 Node API；ES 内置 + 十余个 fs 绑定函数 + fetch 绑定；带超时与内存上限。引擎可换可砍而不动 `run_script(source)` 工具契约
- **联网 = wasi-http + 按 job 域名白名单**，默认全拒，与 preopen 同构
- **LLM 调用由宿主代理**：wasm 通过 import 函数递 messages，宿主贴 key 转发。v1 阻塞式 ABI，不做流式；该形状兼容将来代理演进为远程 API 调用。进度可见性由工具调用事件落 job 日志承担
- **环境说明 = bundled skill（wasm-env），Explicit 注入常驻 worker system prompt**，依赖 skill 触发系统的注入机制（另一 spec 范畴）
- **CLI 面 = jobs/-p 体系扩展**：沙箱档位与目录授权作为派活参数（形如 `--sandbox wasm --grant <dir>`），与 submit/wait/status/result 组合
- **明确拒绝的方案**：宿主代跑模型编排的任意命令（闸门自拆）；WASIX/bash-in-wasm（单一供应商，能力与 run_script 等价）；OS 沙箱档单独先行（丧失位置透明愿景且升级通道已补足 wasm 档的 shell 缺口）

## Testing Decisions

好测试只断言外部行为：进程边界的输出（stdout/JSON/落盘 session）、真实发送给 provider 的请求体、文件系统的实际变化——不断言内部实现。

- **缝 1（现有）：CLI golden 缝**——recording-mock provider 从 `alva -p` 进程边界驱动。断言：子 agent 工具调用必经审批中间件（HITL 修复钉死）；升级请求在 Interactive/Auto/Bypass 三模式下的分别行为；授权外访问的失败对模型可见且任务带原因返回。先例：print_mode_golden、jobs 全生命周期 golden、递归闸门 golden
- **缝 2（新增，唯一新缝）：wasm 宿主 runner 边界**——喂 job 配置（授权目录、域白名单）+ 预编译 fixture wasm 模块，真 wasmtime + 临时目录，只观察文件系统效果与返回值。断言：CRUD 圈禁、run_script 批量修改、绝对路径/dotdot 越狱被拒、fetch 白名单默认拒。QuickJS 绑定与 preopen 接线不单独开缝，全部经此边界覆盖
- **CI 编译缝（现有延伸）**：wasm 防火墙增加 wasip2 目标检查
- 审批分类器、SkillInjector、QuickJS 绑定层不开独立集成缝；单元测试照常但不构成架构缝

## Out of Scope

- skill 触发系统四步（invocation 两档、目录常驻注入、SkillTool 接线、REPL fallback）——独立小项，是 wasm-env skill 的依赖，另行处理
- 流式 LLM 代理 ABI（v1 阻塞式已定案）
- 远程主机部署与 WebDAV/云盘后端的实现（仅保证接口不堵死）
- WASIX、Python/Rhai 等其他脚本引擎、npm/模块系统
- wasm worker 的交互式使用场景（当前全部为 headless 批处理）
- OS 沙箱档的图形界面/Tauri 集成

## Further Notes

- 排期定案：① HITL 洞修复 + 审批接线（按 P0 安全洞立即，主会话亲自做）→ ② skill 触发四步（小活，另 spec）→ ③ 两档手册并行分派。其余 backlog 顺延
- 驱动原则（设计北极星）："不能把安全性委托给一个问号"——隔离是事前设计，不是事后挽救；所有默认值向这条原则对齐
- wasm 档的差异化价值 = 位置透明（处处可部署、文件后端可虚拟化），不只是隔离硬度
- preopen 能力模型的可行性已有本机实验背书（CRUD 全通过、两种越狱全被拒），非纸面推演
