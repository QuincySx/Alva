# Srow Agent 架构设计

> 基于 Wukong (com.dingtalk.real) 逆向分析的完整架构复刻

## 身份

| 属性 | 值 |
|------|-----|
| 项目代号 | Srow Agent |
| Bundle ID | `com.smallraw.app.srow-agent` |
| 技术栈 | Tauri 2.x (Rust + WebView) |
| LLM 框架 | rig (Rust Agent 框架, 6.6k star) |
| 目标平台 | macOS / Windows |

## 核心理念

Srow Agent 是一个 **AI Agent 编排系统**，核心能力是：选人 → 派活 → 监控 → 反思 → 换人/调整 → 重试。

与 Claude Code / Codex 的核心区别：**Skill 和 MCP 按 Agent 定义加载，不是全局的。** 不同的 Agent 有不同的能力集。

## 架构分层

```
┌─────────────────────────────────────────────────────┐
│                    Srow Agent                        │
│                                                      │
│  ┌────────────────────────────────────────────────┐  │
│  │  UI 层 (Tauri WebView)                          │  │
│  │  左: Workspace 列表 / Session 列表               │  │
│  │  中: 聊天记录                                    │  │
│  │  右: Agent 状态面板 (🟢🟡⚫🔴)                   │  │
│  └────────────────────────────────────────────────┘  │
│                                                      │
│  ┌────────────────────────────────────────────────┐  │
│  │  编排层 (Agent Orchestrator)                     │  │
│  │                                                 │  │
│  │  核心 Agent:                                     │  │
│  │  ├── 决策 Agent — 分析任务、选人、派活             │  │
│  │  ├── 验收 Agent — 检查结果、判断质量               │  │
│  │  └── 发散 Agent — 头脑风暴、探索方案               │  │
│  │                                                 │  │
│  │  执行 Agent 实例池:                               │  │
│  │  ├── 浏览器 Agent 实例 A (session-1)              │  │
│  │  ├── 浏览器 Agent 实例 B (session-2)              │  │
│  │  ├── 编码 Agent 实例 C (session-3)                │  │
│  │  └── ...                                        │  │
│  │                                                 │  │
│  │  Agent 模板库:                                    │  │
│  │  ├── 预定义模板 (浏览器/编码/系统/文件...)          │  │
│  │  └── 动态组装 (大脑根据任务临时创建)                │  │
│  └────────────────────────────────────────────────┘  │
│                                                      │
│  ┌────────────────────────────────────────────────┐  │
│  │  引擎层 (Agent Engine)                           │  │
│  │                                                 │  │
│  │  自研引擎 (基于 rig):                             │  │
│  │  ├── LLM 调用 (OpenAI/Claude/Gemini/DeepSeek)   │  │
│  │  ├── 上下文管理 (token 裁剪/压缩)                 │  │
│  │  ├── 工具执行 (shell/file/browser/MCP)           │  │
│  │  └── 记忆系统 (SQLite + 向量搜索)                 │  │
│  │                                                 │  │
│  │  ACP 协议 (外部 Agent):                           │  │
│  │  ├── Claude Code (stdin/stdout JSON)             │  │
│  │  ├── Qwen Code                                  │  │
│  │  ├── Codex CLI                                  │  │
│  │  └── Gemini CLI                                 │  │
│  └────────────────────────────────────────────────┘  │
│                                                      │
│  ┌────────────────────────────────────────────────┐  │
│  │  能力层 (Capabilities)                           │  │
│  │                                                 │  │
│  │  Skill 系统:                                     │  │
│  │  ├── 按 Agent 定义加载（不是全局）                 │  │
│  │  ├── 预定义 Skill + 用户自定义 Skill              │  │
│  │  └── 浏览器 Skill (按域名路由, 类 Wukong MBB)     │  │
│  │                                                 │  │
│  │  MCP 系统:                                       │  │
│  │  ├── 按 Agent 定义加载 MCP Server                │  │
│  │  └── 内置 MCP + 外部 MCP                        │  │
│  │                                                 │  │
│  │  浏览器自动化:                                    │  │
│  │  ├── browser-runtime (Playwright 服务)           │  │
│  │  └── 反检测能力 (代理/指纹/stealth)               │  │
│  └────────────────────────────────────────────────┘  │
│                                                      │
│  ┌────────────────────────────────────────────────┐  │
│  │  安全层 (Security)                               │  │
│  │  ├── 沙箱隔离 (sandbox-exec)                     │  │
│  │  ├── HITL 权限审批 (allow/reject once/always)    │  │
│  │  ├── 敏感路径过滤 (.env/证书/密钥)                │  │
│  │  └── Agent 实例隔离 (独立 session/上下文)          │  │
│  └────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
```

## Agent 通信模型

三种通信方式并存：

```
1. 消息传递（轻量级）
   决策 Agent → "小王，查一下最便宜的机票"
   浏览器 Agent → "老板，580 元"

2. 共享工作区（重量级）
   浏览器 Agent 把抓取数据写到 workspace/output/flights.json
   编码 Agent 读取并写爬虫

3. 项目目录可见（协作级）
   同一个 Workspace 下的 Agent 都能看到项目文件
```

## Agent 生命周期

```
用户输入
  ↓
决策 Agent 分析任务
  ↓
选择/创建子 Agent
  ├── 优先匹配预定义模板
  └── 模板不够 → 动态组装（选 Skill + MCP + LLM）
  ↓
派发任务给子 Agent
  ↓
子 Agent 执行（可能调 LLM、工具、外部 Agent）
  ↓
验收 Agent 检查结果
  ├── 通过 → 返回用户
  └── 不通过 → 反思：
      ├── 选人错了？ → 换 Agent 模板
      ├── 技能不够？ → 调整 Skill 配置
      └── 方案不对？ → 发散 Agent 重新探索
  ↓
重试（直到通过或用户手动 cancel）
```

## 子项目拆解

| Phase | Sub | 名称 | 依赖 | 说明 |
|-------|-----|------|------|------|
| 1 | Sub-1 | Tauri 骨架 + UI 框架 | 无 | 项目初始化，基础窗口布局 |
| 1 | Sub-2 | Agent 引擎 | 无 | 基于 rig 的核心循环 + 工具执行 |
| 2 | Sub-3 | ACP 协议 | Sub-2 | 接入 Claude Code / Qwen Code / Codex |
| 2 | Sub-4 | Skill 系统 | Sub-2 | 按 Agent 定义加载 Skill + MCP |
| 3 | Sub-5 | Agent 编排层 | Sub-2, Sub-4 | 决策/验收/发散 + 子 Agent 管理 |
| 3 | Sub-6 | 浏览器自动化 | Sub-4 | browser-runtime + 域名路由 |
| 4 | Sub-7 | 安全层 | Sub-2, Sub-5 | 沙箱 + HITL + 敏感路径 |
| 4 | Sub-8 | 内嵌运行时 | Sub-6 | Bun/Python/Chromium 打包 |

## 参考资料

- Wukong 逆向分析主报告: `../smallraw-skills/docs/dump/Wukong.analysis/REPORT.md`
- Wukong Spark + ACP 协议分析: `../smallraw-skills/docs/dump/Wukong.analysis/REPORT-spark-acp.md`
- Wukong 技能包分析: `../smallraw-skills/docs/dump/Wukong.analysis/REPORT-skills.md`
- Alma 分析报告: `../smallraw-skills/docs/dump/Alma.analysis/REPORT.md`
- AdsPower 分析 + 反检测浏览器方案: `../smallraw-skills/docs/dump/AdsPower.analysis/`
