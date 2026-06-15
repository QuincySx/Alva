# CLI 增量组件加回清单(从 mini mode → 标准 agent)

> 背景:CLI `build_agent` 已砍到 mini mode = 裸 `BaseAgentBuilder`(自动装 memory/security/system_context)+ `CorePlugin`(增改查搜)+ approval/checkpoint substrate。
> 方法:**一次加一个组件,加完编译 + 实测能跑 + 行为符合预期,再加下一个**。不搞统一 Preset 框架,手动一点点加。
> 日期:2026-06-15。每加一项打勾 + 记 commit。

## 现状(mini mode 基线)
- [x] CorePlugin(read/create/edit/list/find/grep)
- [x] (自动)memory / security / system_context
- [x] (substrate)approval channel + checkpoint_mgr

## 优先级清单(从上往下加)

### P1 — 让它"能干活" ✅(commit 见下)
- [x] **ShellPlugin**(execute_shell)—— 跑命令/build/test/删文件。编码 agent 最高价值。
- [x] **卫生中间件**(LoopDetection + DanglingToolCall + ToolTimeout)—— 近乎零成本,防跑飞/死循环/工具卡死。
  - 编译✅。运行验证(需 API key):`cargo run -p alva-app-cli` → 让 agent 跑个 shell 命令(如 `ls`)、删个临时文件,确认 execute_shell 可用。

### P2 — 安全与长会话 ✅
- [x] **PermissionPlugin**(PlanMode + 审批)—— 配 approval substrate,HITL/plan 模式。验证:mock 回归 7/7 仍过(P2 没破坏工具);plan-mode 拦写已由 `e2e_tool_coverage::stage1_enter_plan_mode...blocks_writes` 项目级覆盖。
- [x] **CompactionMiddleware** —— 长会话上下文压缩(中间件,长会话时激活;上下文压缩逻辑由 context 测试覆盖)。

### P3 — 知识与检索
- [ ] **SkillsPlugin** —— 渐进式 skill 加载(复用 bundled_skills + _paths.skills_dir)。测:加载一个 skill。
- [ ] **WebPlugin**(internet_search + read_url)—— 联网检索。测:搜一次、读一个 URL。

### P4 — 协作与多 agent
- [ ] **TaskPlugin** / **TeamPlugin** —— 任务/团队管理工具。
- [ ] **基础设施 + SubAgentPlugin** —— SubAgent 依赖 ProviderRegistry + SpawnCommRegistry + BlackboardComm,这一档一起加(SubAgentPlugin::new(3) + 三个 infra plugin)。测:spawn 一个子 agent。

### P5 — 扩展与外挂
- [ ] **McpPlugin**(global + project mcp config,复用 _paths)—— MCP 服务器工具。
- [ ] **HooksPlugin**(HooksSettings)—— 用户 hook。
- [ ] **CheckpointMiddleware** —— 自动 checkpoint(手动 checkpoint_mgr 已可用)。
- [ ] **SubprocessLoaderPlugin**(复用 _paths.extensions_dir)—— AEP 第三方子进程插件。

### P6 — 长尾/小众(最后)
- [ ] **InteractionPlugin**(ask_human)
- [ ] **PlanningPlugin** / **UtilityPlugin**
- [ ] **AnalyticsPlugin**
- [ ] **BrowserPlugin**(重依赖 chromiumoxide)

## 完成判据
全部加完后,CLI 装配集合应与重构前(24 项)等价 = "标准 agent"。届时把 `_paths` 改回 `paths`、移除 `bundled_skills` 的 `#[allow(dead_code)]`。

> 注:加回每项时,只动 `agent_setup.rs` 的 builder 链(+ 必要的 substrate 接线如 SubAgent 的 infra)。Tauri 侧暂不动(等 CLI 这条链跑顺了再说,或后续再考虑要不要共用)。
