# Alva 架构设计

## 身份

| 属性 | 值 |
|------|-----|
| 项目代号 | Alva |
| 三仓库 | alva-sandbox（沙箱）+ alva-agent（框架）+ alva-app（产品） |
| 技术栈 | 纯 Rust — GPUI (Zed GPU UI 框架) + 自研 Agent 引擎 |
| 目标平台 | macOS（首发）/ Windows / Linux |

## 核心理念

Alva 是一个**分层解耦的 AI Agent 平台**，三大组件完全独立：

- **alva-sandbox** — 通用沙箱基础设施，不知道谁跑在里面
- **alva-agent** — 通用 Agent 框架，不知道自己跑在哪
- **alva-app** — 产品应用，把 sandbox + agent 组合成桌面工具

与 Claude Code / Codex 的核心区别：**Skill 和 MCP 按 Agent 模板定义加载，不是全局的。** 不同的 Agent 有不同的能力集。

## 三仓库架构

```
┌──────────────────────────────────────────────────────────────────┐
│  alva-app（产品层）                                               │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │  alva-app          — GPUI 桌面 GUI                       │    │
│  │  alva-app-core     — 应用编排层（Agent 生命周期 + Skills + 持久化）│    │
│  │  alva-app-debug    — 调试 HTTP API + traced! 宏          │    │
│  │  alva-app-devtools-mcp — MCP 开发工具服务器              │    │
│  └──────────────────────────────────────────────────────────┘    │
│          依赖 ↓                           依赖 ↓                 │
│  ┌───────────────────────┐  ┌───────────────────────────────┐    │
│  │  alva-agent（框架层）  │  │  alva-sandbox（沙箱层）       │    │
│  └───────────────────────┘  └───────────────────────────────┘    │
└──────────────────────────────────────────────────────────────────┘
```

## alva-agent 框架层

```
┌─ alva-types ─────────────────────────────────────────────────────┐
│  Message, ContentBlock, Tool, LanguageModel, ToolContext, ToolFs  │
│  AgentMessage, StreamEvent, ToolCall, ToolResult                  │
│  （所有 crate 的共享词汇表，零外部依赖）                            │
└──────────────────────────────────────────────────────────────────┘
              ↑ 依赖
┌─ 功能层（并行，互不依赖）─────────────────────────────────────────┐
│  alva-agent-context  — 上下文管理 Hooks + Session + 四层 Store     │
│  alva-agent-core     — Agent 循环引擎 + Middleware 洋葱模型       │
│  alva-agent-tools    — 16 内置工具（通过 ToolFs 抽象）            │
│  alva-agent-security — SecurityGuard + PermissionManager          │
│  alva-agent-memory   — FTS + 向量搜索 + MemoryBackend trait       │
│  alva-agent-graph    — StateGraph + Pregel + Channel + SubAgent   │
└──────────────────────────────────────────────────────────────────┘
              ↑ 依赖
┌─ 组装层 ─────────────────────────────────────────────────────────┐
│  alva-agent-runtime  — AgentRuntimeBuilder（组合所有功能层）       │
└──────────────────────────────────────────────────────────────────┘
              ↑ 依赖
┌─ 引擎层 ─────────────────────────────────────────────────────────┐
│  alva-engine-runtime       — EngineRuntime trait（统一引擎接口）   │
│  alva-engine-adapter-alva  — 本地 Agent 适配器（直接 Rust 调用）  │
│  alva-engine-adapter-claude — Claude SDK 适配器（Node.js bridge） │
└──────────────────────────────────────────────────────────────────┘
              ↑ 依赖
┌─ 协议层（独立，不依赖 alva-app）─────────────────────────────────┐
│  alva-protocol-skill — Skill 三级加载：metadata → body → resource │
│  alva-protocol-mcp   — MCP 客户端：连接、工具发现、McpToolAdapter  │
│  alva-protocol-acp   — Agent Client Protocol：消息、会话、进程     │
└──────────────────────────────────────────────────────────────────┘
```

## alva-sandbox 沙箱层

```
┌─ alva-sandbox（core）────────────────────────────────────────────┐
│  Sandbox trait        — exec / read_file / write_file / list_dir │
│  SandboxProvider      — create / get / destroy                    │
│  SandboxAdapter       — 后端实现者接口                             │
│  SandboxCapability    — 可选能力（Stream / Sleep / Snapshot 等）   │
│  EnvPolicy            — 环境变量隔离（Inherit / Clean / Whitelist）│
└──────────────────────────────────────────────────────────────────┘
              ↑ impl SandboxAdapter
┌─ Adapter 实现 ───────────────────────────────────────────────────┐
│  alva-sandbox-local       — 本地 macOS/Linux（已实现）            │
│  alva-sandbox-docker      — Docker 容器（bollard）               │
│  alva-sandbox-e2b         — E2B 云沙箱（REST API）               │
│  alva-sandbox-v8          — V8 isolate（类 Cloudflare Workers）  │
│  alva-sandbox-cloudflare  — Cloudflare Workers                   │
│  (future)                 — iOS sandbox / WASM                   │
└──────────────────────────────────────────────────────────────────┘
```

**Sandbox 不知道谁跑在里面**——可以是我们的 Agent、Claude Code、任何 CLI 工具、Node.js 应用。

## ToolFs：连接 Agent 和 Sandbox 的桥

Agent 工具通过 `ToolFs` trait 操作文件和执行命令，不直接调用系统 API：

```
工具代码 → ToolFs trait (alva-types 中定义)
                ↓
        ┌───────┴───────┐
    LocalToolFs      SandboxToolFs (alva-app-core 桥接)
    (tokio::fs)      (dyn Sandbox → exec/read/write)
```

- **Agent 框架不依赖 Sandbox 框架**——ToolFs 是纯抽象
- **Sandbox 框架不依赖 Agent 框架**——它只提供执行环境
- **alva-app-core 做桥接**——SandboxToolFs 实现 ToolFs，委托给 dyn Sandbox

## 引擎系统

EngineRuntime trait 统一不同的 Agent 引擎后端：

```
EngineRuntime trait（execute / cancel / respond_permission / capabilities）
        ↓ impl
┌───────┴──────────────────────┐
│  AlvaAdapter                  │  ClaudeAdapter
│  直接 Rust 调用               │  Node.js bridge + JSON-line
│  AgentEvent → RuntimeEvent    │  SDK message → RuntimeEvent
│  本地工具执行                  │  SDK 内部管理工具
│  CancellationToken 取消       │  stdin 信号取消
└───────────────────────────────┘
```

## Skill 系统

三级渐进加载，按 Agent 模板定义：

| Level | 内容 | 何时加载 | Token 开销 |
|:---:|------|---------|-----------|
| 1 | Metadata（name + description） | 始终驻留 system prompt | ~50-150 |
| 2 | Body（SKILL.md 完整内容） | 用户 prompt 触发后 | ~500-2000 |
| 3 | Resources（scripts / references） | Agent 按需调用 | 可变 |

注入策略：
- **Auto** — 只注入 metadata，Agent 用 `use_skill` 工具按需拉取
- **Explicit** — 直接注入完整 body 到 system prompt
- **Strict** — 同 Explicit + 限制只能用该 Skill 允许的工具

SkillInjectionMiddleware 可根据用户消息动态搜索并注入相关 Skill。

## 安全模型

```
┌─ 权限层 ─────────────────────────────────────────┐
│  SecurityGuard     — 工具调用前检查（allow/block）  │
│  PermissionManager — HITL 四选项审批               │
│  SensitivePathFilter — .env/证书/密钥路径过滤      │
│  AuthorizedRoots   — 允许的工作区根目录             │
└──────────────────────────────────────────────────┘

┌─ 沙箱层 ─────────────────────────────────────────┐
│  SandboxConfig     — macOS Seatbelt profile 生成   │
│  EnvPolicy         — 环境变量隔离策略              │
│  NetworkPolicy     — 网络访问控制                  │
│  (Docker/E2B 天然隔离)                             │
└──────────────────────────────────────────────────┘

Secrets 管理 — 独立 CLI 工具：
├── 以 npm 包 / 纯 JS / WASM 形式部署到沙箱
├── 通过网络 API 请求临时授权获取密钥
├── 每次访问有审计日志
└── 授权有 TTL，沙箱销毁后失效
```

## 依赖防火墙

`scripts/ci-check-deps.sh` 自动检查 12 条边界规则：

```
Rule 1:  alva-types 零 workspace 依赖
Rule 2:  alva-agent-context → alva-types only
Rule 3:  alva-agent-core → alva-types + alva-agent-context
Rule 4:  alva-agent-tools → alva-types only
Rule 5:  alva-agent-security → alva-types only
Rule 6:  alva-agent-memory → alva-types only
Rule 7:  alva-agent-runtime → foundation agent-* crates
Rule 8:  alva-agent-graph → alva-types + alva-agent-core
Rule 9:  alva-engine-runtime → alva-types only
Rule 10: alva-engine-adapter-claude → alva-types + engine-runtime
Rule 11: alva-engine-adapter-alva → alva-types + engine-runtime + agent-core
Rule 12: protocol crates 不依赖 alva-app-*
Rule 13: alva-app 不直接依赖 agent-* 内部 crate（通过 facade）
```

## 参考设计

| 参考 | 我们学了什么 |
|------|------------|
| [Sandbank](https://github.com/chekusu/sandbank) | Sandbox Provider/Adapter 三层分离 + Capability 协商 |
| Claude Code | Skill 系统 + ACP 协议 + HITL 权限模型 |
| LangGraph | StateGraph + Pregel 图编排 |
