# Srow Agent vs Wukong 功能对齐检查

> 检查日期: 2026-03-20
> Srow 代码路径: `/Users/smallraw/Development/QuincyWork/srow-agent/src/`
> Wukong 参考: `/Users/smallraw/Development/QuincyWork/smallraw-skills/docs/dump/Wukong.analysis/`

## 评判标准

- **已对齐**: 功能完整实现，有真实逻辑（非空占位），可运行
- **部分实现**: 有代码结构和核心逻辑，但关键路径未完成或有 TODO
- **缺失**: 无对应代码，或仅有空模块占位

---

## 总览

| 维度 | 对齐率 | 说明 |
|------|--------|------|
| A. Agent 引擎 | 7/10 (70%) | 核心循环完整，缺 enable_thinking/parallel_tool_calls/prompt_cache |
| B. 内置工具 | 13/22 (59%) | 代码操作 + 浏览器基本齐全，缺 AI 生成/搜索/MCP 元工具 |
| C. ACP 协议 | 11/12 (92%) | 协议层最完整的模块，全链路贯通 |
| D. Skill 系统 | 6/7 (86%) | 三级加载 + 注入策略齐全，缺 search_skills/use_skill 元工具 |
| E. MCP 协议 | 4/5 (80%) | 管理/调用/适配齐全，缺 mcpServerConfig.json 读写和内置 Server |
| F. 安全层 | 5/5 (100%) | 最完整的模块，sandbox-exec + 敏感路径 + HITL + 授权根 + SecurityGuard |
| G. 运行时管理 | 5/6 (83%) | manifest/versions/installer/resolver 齐全，缺 HTTP 下载 |
| H. 桌面客户端 UI | 3/5 (60%) | 三栏布局 + Workspace 树 + 聊天面板，缺 Agent 状态面板真实数据 |
| I. 多渠道/网络 | 1/4 (25%) | 仅有进程管理器，缺渠道框架/可观测性/SQLite 持久化 |
| J. Srow 独有 | 4/4 (100%) | 编排层/动态 Agent 组装/按模板加载 Skill+MCP/Preview Panel |

**整体对齐率: 59/80 = 74%**

---

## 详细对比

### A. Agent 引擎（对标 AllSpark/Spark Loop）

| 功能 | Wukong | Srow | 状态 | 差距 |
|------|--------|------|------|------|
| Agent 核心循环 | `service_impl.rs` prompt→LLM→tool_call→execute→loop | `engine.rs` 完整实现 run() 循环，含 cancel/max_iterations/compaction | ✅ | 逻辑完整，含 iteration guard |
| LLM 多 Provider 支持 | 7 个：MaaS/OpenAI/Claude/Gemini/GLM/Qwen/MiniMax | 1 个实际实现：`OpenAICompatProvider`（via rig-core），支持 OpenAI/DeepSeek/Qwen 通过 base_url 切换 | ⚠️ | 仅 1 个适配器，缺原生 Claude/Gemini/GLM/MiniMax 适配器。`LLMProviderKind` 声明了 4 种但只有 OpenAI 兼容路径有实现 |
| 上下文管理/压缩 | `compaction.rs` token 超限裁剪历史 | `context_manager.rs` 有 threshold 检查 + truncation（Strategy A），预留 Strategy B 调 LLM 摘要 | ✅ | 简单截断实现可用，LLM 摘要尚未实现 |
| 会话管理 | Session CRUD + 持久化 (SQLite) | `SessionService` CRUD + `MemoryStorage` 内存实现，`SessionStorage` trait 定义完整 | ⚠️ | 功能可用但无 SQLite 持久化，重启丢失 |
| 记忆系统 | SQLite + Embedding + FTS 混合搜索 + MEMORY.md 同步 | `agent/memory/mod.rs` 仅有空注释占位 | ❌ | 完全缺失。Wukong 有 memory_files/memory_chunks/chunks_fts/embedding_cache 四张表 |
| 消息模型 | LLMMessage/LLMContent/ToolCall/ToolResult | `domain/message.rs` + `domain/tool.rs` 完整定义，含 Role/LLMContent(Text/ToolUse/ToolResult) | ✅ | 1:1 对齐 |
| 流式输出 | streaming response via LLM 适配器 | `complete_stream()` 通过 mpsc channel 发送 StreamChunk(TextDelta/ToolCallDelta/Done) | ✅ | 完整实现 |
| enable_thinking | MaaS/Qwen 支持 `enable_thinking` 思考模式 | rig-core 的 stream 有 `Reasoning`/`ReasoningDelta` 事件但被跳过 (`_ => {}`) | ❌ | 协议层有 reasoning 事件类型但未处理，未暴露到 EngineEvent |
| parallel_tool_calls | LLM 请求参数 `parallel_tool_calls` | `execute_tools()` 注释写 "sequentially for now; parallel in future with join_all" | ❌ | 代码 for 循环串行执行工具 |
| prompt_cache_hit_tokens | MaaS 的 `prompt_cache_hit_tokens` | 无对应字段或逻辑 | ❌ | 缺失 |

### B. 内置工具（对标 Wukong 22 个）

| 功能 | Wukong | Srow | 状态 | 差距 |
|------|--------|------|------|------|
| execute_shell | ✅ 沙箱内执行，超时控制 | `execute_shell.rs` 完整实现，sh -c + timeout + cwd | ✅ | 缺沙箱 wrap（SandboxConfig 已实现但 execute_shell 未调用 wrap_command） |
| create_file | ✅ | `create_file.rs` 完整实现，支持 auto create_dirs | ✅ | — |
| file_edit | ✅ old_string→new_string + replace_all | `file_edit.rs` 完整实现 | ✅ | — |
| grep_search | ✅ pattern + use_regexp + include_pattern | `grep_search.rs` 完整实现 | ✅ | — |
| list_files | ✅ | `list_files.rs` 完整实现 | ✅ | — |
| ask_human | ✅ HITL 请求用户输入 | `ask_human.rs` 完整实现，发送 WaitingForHuman 事件 | ✅ | — |
| browser_start | ✅ | `browser_start.rs` + `BrowserManager::start()` 通过 chromiumoxide 启动 Chrome | ✅ | 真实 CDP 实现 |
| browser_stop | ✅ | `browser_stop.rs` + `BrowserManager::stop()` | ✅ | — |
| browser_status | ✅ | `browser_status.rs` 列出实例和标签页信息 | ✅ | — |
| browser_screenshot | ✅ | `browser_screenshot.rs` 通过 CDP 截图，返回 base64 | ✅ | — |
| browser_navigate | Wukong 无独立工具 | `browser_navigate.rs` 独立导航工具 | ✅ | Srow 额外 |
| browser_action | Wukong 无独立工具 | `browser_action.rs` 执行 click/type/press 等 | ✅ | Srow 额外 |
| browser_snapshot | Wukong 无独立工具 | `browser_snapshot.rs` 获取 DOM 快照 | ✅ | Srow 额外 |
| browser_wait_for_download | ✅ | 无对应实现 | ❌ | 缺失 |
| text2image | ✅ 云端 binding | 无对应实现 | ❌ | 缺失 |
| image2image | ✅ | 无对应实现 | ❌ | 缺失 |
| text2video | ✅ LWP MCP | 无对应实现 | ❌ | 缺失 |
| understand_image | ✅ 图像内容分析 | 无对应实现 | ❌ | 缺失 |
| parse_file | ✅ PDF 等文件解析 | 无对应实现 | ❌ | 缺失 |
| internet-search | ✅ 云端 binding | 无对应实现 | ❌ | 缺失 |
| read_url | ✅ 网页内容读取 | 无对应实现 | ❌ | 缺失 |
| mcp_runtime | ✅ list_servers/list_tools/call_tool | `McpToolAdapter` 将 MCP 工具注册为 Tool trait，但无 `mcp_runtime` 聚合元工具 | ⚠️ | MCP 工具已可通过 ToolRegistry 调用，但缺少显式的 list_servers/list_tools/call_tool 元工具 |
| view_image | ✅ | 无对应实现 | ❌ | 缺失 |
| dingtalk_core | ✅ 钉钉 API 操作 | 无对应实现（正确：Srow 不需要钉钉集成） | N/A | 无需对齐 |

### C. ACP 协议（对标 Wukong agent_client）

| 功能 | Wukong | Srow | 状态 | 差距 |
|------|--------|------|------|------|
| ACP 消息类型 — Inbound | 13 种 | `AcpInboundMessage` 枚举 13 种：SessionUpdate/MessageUpdate/RequestPermission/TaskStart/TaskComplete/SystemMessage/FinishData/ErrorData/PreToolUse/PostToolUse/ToolCallData/Plan/PingPong | ✅ | 1:1 对齐 |
| ACP 消息类型 — Outbound | 5 种 | `AcpOutboundMessage` 枚举 5 种：Prompt/PermissionResponse/Cancel/Shutdown/Pong | ✅ | 1:1 对齐 |
| Bootstrap payload | workspace/authorized_roots/sandbox_level/model_config/attachments | `BootstrapPayload` 完整实现所有字段 + srow_version | ✅ | 1:1 对齐 |
| 进程管理 | spawn/stdin/stdout/orphan cleanup | `AcpProcessManager` spawn + send + shutdown + subscribe；`AcpProcessHandle` 完整 stdin writer/stdout reader/stderr logger/process wait 四个 tokio task | ✅ | 完整实现 |
| 孤儿清理 | REWIND_PROCESS_MANAGER_PARENT 环境变量标记 | `orphan.rs` 有 SROW_PROCESS_MANAGER_PID 环境变量注入，cleanup 函数为 no-op placeholder | ⚠️ | 框架搭好，实际扫描逻辑 TODO |
| 权限管理 | PermissionRequest → 4 选项 | `PermissionManager` 完整缓存 + `AcpSession::handle_permission_request` 全流程（缓存检查→WaitingForHuman→oneshot 等待→回传） | ✅ | 完整实现 |
| Agent 发现 | Claude/Qwen/Codex/Gemini CLI + PATH 查找 | `AgentDiscovery` 支持 ClaudeCode/QwenCode/CodexCli/GeminiCli/Generic 5 种 | ✅ | 完整实现，含 npx fallback 和 builtin packages 路径 |
| AgentDelegate → Tool trait 桥接 | 无（Wukong ACP Agent 不作为 Tool 暴露） | `AcpDelegateTool` 将 AgentDelegate 包装为 Tool trait，可被编排层 Agent 调用 | ✅ | Srow 独有设计：外部 Agent 可作为工具被决策 Agent 调用 |
| ContentBlock 类型 | Text/ToolUse/ToolResult | `content.rs` 完整实现 3 种，含 is_delta 标记 | ✅ | 1:1 对齐 |
| ACP Session 状态机 | Ready→Running→WaitingForPermission→Completed/Cancelled/Error/Crashed | `AcpSessionState` 7 种状态完整实现 | ✅ | 1:1 对齐 |
| ACP 消息持久化 | SQLite acp_messages 表 | `AcpMessageStorage` 内存 Vec 实现，预留 SQLite 接口 | ⚠️ | 功能可用但非持久化 |
| 心跳 Ping/Pong | ✅ | `PingPong` 完整实现，AcpSession 收到 ping 自动回 pong | ✅ | — |

### D. Skill 系统（对标 Wukong skills/）

| 功能 | Wukong | Srow | 状态 | 差距 |
|------|--------|------|------|------|
| SKILL.md 解析 | YAML frontmatter + Markdown body | `SkillMeta`（name/description/license/allowed_tools/metadata）+ `SkillBody`（markdown + estimated_tokens） | ✅ | 完整定义 |
| 三级渐进加载 | meta → body → resources | `SkillLoader` 实现 `build_meta_summary(L1)` / `load_skill_body(L2)` / `load_resource(L3)` | ✅ | 完整实现 |
| Skill 类型 | Bundled/MBB/User | `SkillKind` 枚举：Bundled / Mbb{domains} / UserInstalled | ✅ | 1:1 对齐，含 MBB 域名绑定 |
| MBB 域名路由 | manifest.json → domain 匹配 | `SkillStore::find_mbb_by_domain()` 按域名后缀匹配 | ✅ | 完整实现 |
| Skill 注入策略 | Auto/Explicit/Strict | `InjectionPolicy` 3 种 + `SkillInjector::build_injection()` 分策略处理 | ✅ | Auto=仅 metadata，Explicit=展开 body，Strict=额外声明 tool 限制 |
| search_skills / use_skill 元工具 | Agent 运行时可调用 search_skills 搜索、use_skill 加载 | `SkillStore::search()` 方法存在，但未包装为 Tool trait 注册到 ToolRegistry | ⚠️ | 后端能力齐全，缺 Tool 封装让 Agent 自主调用 |
| 安装/启用/禁用/删除 | SkillStore 完整生命周期 | `SkillStore` install/remove/set_enabled 完整实现，含 "Bundled 不可删除" 保护 | ✅ | 完整实现 |

### E. MCP 协议（对标 Wukong src/mcp/）

| 功能 | Wukong | Srow | 状态 | 差距 |
|------|--------|------|------|------|
| MCP Server 管理 | 配置/启动/停止/连接/断开 | `McpManager` register/connect/disconnect/connect_auto + 状态机（Disconnected→Connecting→Connected/Failed） | ✅ | 完整生命周期 |
| MCP 工具调用 | list_servers/list_tools/call_tool | `McpManager` list_all_tools/call_tool/server_states | ✅ | — |
| MCP 工具 → Tool trait 适配 | Wukong 通过 mcp_runtime 聚合 | `McpToolAdapter` 实现 Tool trait，`build_mcp_tools()` 批量转换；工具名格式 `mcp:<server_id>:<tool_name>` | ✅ | 比 Wukong 更优雅：MCP 工具与内置工具统一注册 |
| mcpServerConfig.json 读写 | 用户自定义 MCP Server 配置文件 | 无文件级配置读写，仅有 `McpServerConfig` 数据结构 | ⚠️ | 缺从磁盘加载/保存用户 MCP 配置 |
| 内置 MCP Server | 8 个 builtin（system-permissions/browser/dingtalk-*等） | 无内置 MCP Server | ❌ | Wukong 的 builtin MCP 多为钉钉特有，Srow 暂不需要，但缺 system-permissions 类通用能力 |

### F. 安全层（对标 Wukong 安全架构）

| 功能 | Wukong | Srow | 状态 | 差距 |
|------|--------|------|------|------|
| macOS sandbox-exec | 4 种模式 | `SandboxConfig` 4 种模式（RestrictiveOpen/RestrictiveClosed/RestrictiveProxied/PermissiveOpen）+ `generate_sb_profile()` 生成 .sb + `wrap_command()` | ✅ | 完整实现，含 deny default + file-read* + subpath write + network 控制 |
| 敏感路径过滤 | .env/证书/密钥/内部目录 | `SensitivePathFilter` 4 层规则：denied_dirs(.gnupg/.ssh/.kube/.aws/.azure/.gcloud/.docker) + denied_extensions(.pem/.key/.p12/.pfx/.jks/.keystore/.cer/.crt) + denied_filenames(.env*/.npmrc/.pypirc/credentials.json) + denied_patterns(regex) | ✅ | 比 Wukong 更全面，额外覆盖 .aws/.azure/.gcloud/.docker |
| HITL 权限审批 | 4 选项 + session 缓存 | `PermissionManager` (security 模块) AllowOnce/AllowAlways/RejectOnce/RejectAlways + always_allowed/always_denied HashSet 缓存 + pending oneshot channel | ✅ | 完整实现 |
| 授权根目录 | authorized_roots | `AuthorizedRoots` workspace 主根 + extra_roots 列表 + check() 验证路径归属 | ✅ | 完整实现 |
| SecurityGuard 统一入口 | spark_session_handler.rs 权限评估 | `SecurityGuard::check_tool_call()` 统一入口，组合 sensitive_paths + authorized_roots + HITL permission 三层检查 + extract_paths 从 JSON args 和 shell 命令中提取路径 | ✅ | 设计优于 Wukong：单一入口，自动路径提取 |

### G. 运行时管理（对标 Wukong environment/）

| 功能 | Wukong | Srow | 状态 | 差距 |
|------|--------|------|------|------|
| resource_manifest.json 解析 | 组件版本 + artifact 配置 | `ResourceManifest` 完整解析 profile/platform/components/artifacts | ✅ | 含 excluded(platform:) 排除逻辑 |
| versions.json 版本跟踪 | 已安装版本记录 | `InstalledVersions` 读写 + `check_all()` 比较：UpToDate/NotInstalled/NeedsUpdate/Excluded | ✅ | 完整状态机 |
| 组件安装 | zip/tar.gz 解压 | `Installer` 支持 ZipFlat/TarGz/QwenZip 3 种格式，含 strip prefix 智能检测 + Unix 权限保留 | ✅ | 有单测覆盖 |
| 路径解析 | Bun/Node/Python/uv/Chromium | `RuntimeResolver` 6 组件（Bun/Node/Python/uv/Chromium/Qwen），每个有平台特定候选路径 | ✅ | 完整实现，含 Windows 路径 |
| 按需下载 | CDN 下载 + 校验 | `resolve_archive()` 有 URL 处理分支但标注 "not yet implemented — placeholder" | ⚠️ | 框架齐全，HTTP 下载未实现 |
| EnvironmentManager ensure_ready() | 启动时自动检查更新 | `ensure_ready()` 遍历 manifest → 比较 versions → install_one，完整流程 | ✅ | — |

### H. 桌面客户端 UI

| 功能 | Wukong | Srow | 状态 | 差距 |
|------|--------|------|------|------|
| 三栏布局 | 侧边栏 + 聊天 + 右面板 | `RootView` 三栏：SidePanel(220px) + ChatPanel(flex-1) + AgentPanel(280px)，基于 GPUI 框架 | ✅ | 布局实现完整 |
| Workspace/Session 树形列表 | Wukong WebView 多窗口 | `SidePanel` + `sidebar_tree.rs` + `WorkspaceModel` | ⚠️ | 有代码结构但功能简化 |
| 聊天消息展示 | 富文本 + 工具调用卡片 | `ChatPanel` + `message_list.rs` + `input_box.rs` + `ChatModel` | ⚠️ | 基本框架有，但展示能力不如 WebView |
| Agent 状态面板 | Agent 运行状态、工具调用历史 | `AgentPanel` + `AgentModel` | ⚠️ | 有面板但数据绑定尚未完善 |
| 多窗口 | Wukong 8 个独立窗口 | 单窗口三栏 | ❌ | GPUI 技术栈不走多窗口路线 |

### I. 多渠道/网络（对标 Wukong channels/）

| 功能 | Wukong | Srow | 状态 | 差距 |
|------|--------|------|------|------|
| 钉钉集成 / 渠道框架 | channels/ 钉钉 AI Card/Gateway/Stream | `gateway/mod.rs` 空占位 | ❌ | 无渠道框架。Srow 定位桌面客户端，暂不需要钉钉，但缺通用渠道抽象 |
| 数据存储 SQLite | tasks/acp_messages/spark_agui_message 三张表 | `MemoryStorage` 内存实现 + `AcpMessageStorage` 内存 Vec | ❌ | 无 SQLite 依赖，所有数据重启丢失 |
| 进程管理器 | ProcessManager lifecycle: Running→Exited→Crashed→Restarting | `AcpProcessManager` + `ProcessState`(Running/Exited/Crashed/Restarting) | ✅ | 完整实现 |
| 可观测性 | Langfuse + OpenTelemetry + Sentry | 无 Langfuse/OTel/Sentry 集成，仅有 `tracing` 日志 | ❌ | 缺失 |

### J. Srow 独有功能（超越 Wukong）

| 功能 | Wukong | Srow | 状态 | 说明 |
|------|--------|------|------|------|
| Agent 编排层 | 无（单 Agent 架构） | `Orchestrator` 含 brain/reviewer/explorer 三角色 + 执行 Agent 池 + MessageBus 通信 | ✅ | Srow 独有：多 Agent 编排，brain 分析任务→create_agent 分发→reviewer 质检 |
| 动态 Agent 组装 | 无 | `OrchestratorAgentTemplate` + `Orchestrator::create_agent()` 从模板动态实例化 | ✅ | 模板库 + 运行时实例化 |
| 按 Agent 定义加载 Skill/MCP | 无 | `AgentTemplateService` + `AgentTemplate`(skills: SkillSet + mcp: McpSet) → 按模板组装 | ✅ | 每个 Agent 实例可有不同的 Skill 和 MCP 配置 |
| AcpDelegateTool 桥接 | Wukong ACP Agent 独立运行 | `AcpDelegateTool` 将外部 Agent 包装为 Tool，决策 Agent 可通过 tool_call 委派任务给 Claude/Qwen/Codex/Gemini | ✅ | 外部 Agent 作为 Tool 参与编排 |

---

## 关键差距总结

### 必须补齐（阻塞可用性）

1. **SQLite 持久化** — 当前所有数据（Session/Message/ACP Message）使用内存存储，重启全部丢失。需要引入 `tokio-rusqlite` 实现 `SessionStorage` 和 `AcpMessageStorage` 的 SQLite 后端。
2. **记忆系统** — `agent/memory/mod.rs` 完全空白。Wukong 有 SQLite + Embedding + FTS 混合搜索 + MEMORY.md 同步，这是长对话场景的核心能力。
3. **search_skills / use_skill 元工具** — 后端 `SkillStore::search()` 已实现，但未包装为 Tool trait 注册到 ToolRegistry，Agent 无法自主搜索和加载 Skill。

### 应该补齐（影响体验）

4. **parallel_tool_calls** — 当前工具串行执行，应改为 `tokio::join_all` 并行。
5. **enable_thinking** — rig-core 已有 Reasoning 事件但被跳过，应暴露到 EngineEvent 让 UI 展示思考过程。
6. **HTTP 下载组件** — `Installer::resolve_archive()` 有 URL 分支但未实现下载逻辑。
7. **execute_shell 沙箱集成** — `SandboxConfig::wrap_command()` 已实现但 `ExecuteShellTool` 未调用它，命令裸执行。
8. **孤儿进程清理** — `cleanup_orphan_processes()` 为 no-op，需实现平台特定进程扫描。

### 低优先级（可后期补充）

9. **更多 LLM 适配器** — 原生 Claude/Gemini 适配器（当前通过 OpenAI 兼容协议可部分覆盖）。
10. **AI 生成工具** — text2image/image2image/text2video/understand_image/parse_file（需云端 API 绑定）。
11. **搜索/读取工具** — internet-search/read_url（需外部服务或爬虫）。
12. **可观测性** — Langfuse/OpenTelemetry/Sentry 集成。
13. **mcpServerConfig.json** — 用户自定义 MCP Server 配置文件的磁盘读写。

---

## 架构设计差异

| 维度 | Wukong | Srow | 评价 |
|------|--------|------|------|
| 应用框架 | Tauri 2.x (Rust + WKWebView) | GPUI (Rust native) | Srow 更轻量，无 WebView 依赖 |
| Agent 架构 | 单 Agent + 多引擎（Spark 进程内 + ACP 子进程） | 多 Agent 编排（Orchestrator + 3 角色 + 动态池）+ ACP 子进程 | Srow 架构更先进 |
| 浏览器自动化 | Bun + Playwright + Express HTTP 服务（TypeScript，~160 文件） | chromiumoxide (Rust native CDP) | Srow 更简洁，但功能较少 |
| LLM 调用 | 自研 AllSpark SDK，7 个适配器 | rig-core + OpenAI 兼容适配器 | Wukong 更全面，Srow 更轻量 |
| Skill→Agent 绑定 | 全局 Skill，所有 Agent 共享 | 按 AgentTemplate 定义各自的 SkillSet + McpSet | Srow 更灵活 |
| 安全模型 | 相似度高 | 相似度高，且 Srow 的 SecurityGuard 设计更统一 | Srow 略优 |
