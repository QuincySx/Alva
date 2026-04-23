---
name: amp-mcp
description: Amp 的 MCP（Model Context Protocol）客户端完整实现 —— 发现、连接、OAuth、状态机、tool 过滤、resource 读取、error 分类。当需要理解 Amp 怎么做 MCP server 管理、怎么把 MCP tools 接到主 tool registry、怎么做 OAuth callback 时加载。
trigger_words:
  - mcp
  - mcpService
  - mcpServers
  - amp mcp
  - mcp oauth
  - mcp doctor
  - mcp approve
  - includeTools
  - read_mcp_resource
  - StdioClientTransport
  - mcp.json
  - MCP tool filtering
---

# Amp MCP Client

Amp 把 MCP servers 当作**一等 tool source**，和 builtin / toolbox / plugin 并列。但做了远比"照搬 SDK"多的事：三处配置来源、五种状态、OAuth 回调 server、tool glob 过滤、resource 读取工具、显式 workspace trust 审批。

整个流程对 Alva 非常有参考价值，因为 `alva-protocol-mcp::McpToolAdapter` 现在只覆盖了"tool 包装"这一层，而 Amp 暴露的 lifecycle / OAuth / trust / filtering 才是把 MCP 真正用起来的关键。

## 文件索引

| 文件 | 内容 | 何时读 |
|---|---|---|
| `./discovery-config.md` | `.amp/settings.json` + 全局 `~/.config/amp/settings.json` 的 `mcpServers` 格式，workspace vs global 语义 | 想懂配置的三处来源（workspace / global / `--mcp-config` flag）和 skill bundled mcp.json |
| `./lifecycle-states.md` | 状态机 `loading / connecting / connected / reconnecting / failed / denied / awaiting-approval / blocked-by-registry` 的完整转换 + 观察途径 | 想懂一个 MCP server 从配置到可用之间经历什么 |
| `./oauth-flow.md` | Discovery (`/.well-known/oauth-authorization-server`) + 回调 server (`127.0.0.1:8976`) + `secretStorage` 保存 token + `amp mcp oauth login/logout/status` 三件套 | 想给 Alva 加 MCP OAuth 支持 |
| `./tool-filtering.md` | `includeTools` glob 过滤 + chrome-devtools 26 tools = 17700 tokens 的实测 + name prefix `mcp__<server>__<tool>` | 想懂为什么必须过滤，怎么在 skill 里只暴露子集 |
| `./resource-reading.md` | `read_mcp_resource` builtin tool + `$z` 截断阈值 + `@server:uri` 引用语法 | 想懂 MCP resources（不是 tools）怎么接入 |
| `./error-handling.md` | `vCT()` 错误分类函数：`timeout / auth-failed / network / server-error` + MCP image error 处理 + `MCP tool ... error response without details` 兜底 | 想做错误归一化 |

## 常见快速问答

**Q：怎么给 Amp 加一个 MCP server？**
A：三个地方任选一个。
1. **CLI 写入 global**：`amp mcp add <name> -- <command> [args...]`，写到 `~/.config/amp/settings.json` 的 `amp.mcpServers`。
2. **CLI 写入 workspace**：加 `--workspace` flag，写到 `.amp/settings.json`。**但 workspace servers 需要 `amp mcp approve <name>` 显式审批才会跑**（trust check），见 `lifecycle-states.md`。
3. **Skill bundle**：`.agents/skills/<name>/mcp.json`，跟 skill 绑定，懒暴露 tools（skill 没 load 时 tools hidden 但 MCP 已经起了）。
完整支持 URL 自动检测（`amp mcp add sg https://...` 会自动选 transport），header / env 注入，详见 `discovery-config.md`。

**Q：tool filtering 怎么工作？**
A：每个 MCP server config 可选 `includeTools: string[]`（glob pattern）。Amp 文档里明确写：
> chrome-devtools has 26 tools = 17,700 tokens. Always use `includeTools` to expose only what the skill needs. This reduces token cost by 90%+.

当一个 MCP tool 在 prompt 里呈现时，name 是 `mcp__<server>__<tool>`（双下划线），在 permissions 规则里这样匹配：`allow mcp__atlassian__jira_fetch_issue --issue_key "TEST*"`。详见 `tool-filtering.md`。

**Q：Amp 怎么处理 MCP OAuth？**
A：每个 MCP server 启动之前检测它是 URL-based 且没有 `headers` / `command` → 走 OAuth 路径。三件套：
- `amp mcp oauth login <name> --server-url <url> --client-id <id>`：discovery + 注册 client info 到 `secretStorage`
- `amp mcp oauth logout <name>`：清 token
- `amp mcp oauth status <name>`：查 token / expiry

实际回调在 `127.0.0.1:8976/oauth/callback`，是**一个 shared callback server**（多个 MCP server 复用一个端口）。详见 `oauth-flow.md`。

**Q：MCP resources 呢？**
A：单独的 builtin tool `read_mcp_resource(server, uri)`。用户在 prompt 里可以写 `@filesystem-server:file:///path/to/document.txt`，模型就会自动调。content 超过 `$z`（未知确切值，推断 ~128KB）会截断。详见 `resource-reading.md`。

**Q：一个 MCP server 连不上怎么办？**
A：Amp 有 `amp mcp doctor [name]` 专门的诊断命令，等 `mcpService.initialized`，然后遍历所有 servers 打印状态。状态里包含完整错误分类（timeout / auth-failed / network / server-error），错误信息里包含补救建议（"Try registering OAuth credentials manually" 等）。详见 `error-handling.md`。

**Q：workspace 里有 `.amp/settings.json` 带 MCP server，安全吗？**
A：**默认不跑**。需要 `amp mcp approve <name>` 或在 global settings 里加 `amp.mcpTrustedWorkspaces: ["<workspace-root>"]`。状态会显示为 `awaiting-approval`。这是 Amp 专门防"trojan repo"的设计。

## 对 Alva 的启发（浓缩版）

当前 `alva-protocol-mcp` 已有：`McpClient` 配置/连接/工具枚举、`McpToolAdapter` 把每个 tool 包成 `alva_kernel_abi::Tool`（name 格式 `mcp:<server>:<tool>`）、SSE/Stdio transport、config 文件读写。

缺的（Amp 都有、按优先级排）：

1. **状态机可观察化** —— Alva 现在只有 `Connected / Disconnected / Connecting` 三态，没有 `awaiting-approval / failed / reconnecting / blocked-by-registry`。UI 层 / `mcp doctor` 命令都依赖这个。
2. **`includeTools` glob 过滤** —— 这是省钱的关键，**17K tokens** 不是假的。应该在 `McpServerConfig` 加 `include_tools: Option<Vec<String>>` 字段，`build_mcp_tools` 时 glob-match 过滤。
3. **Workspace trust / approval** —— 同一套 `.amp/settings.json` trust 模型，避免 repo 里的 MCP server 自动跑。
4. **OAuth callback server** —— 要接任何 remote MCP（Sourcegraph / HuggingFace / Monday），必须要。Amp 的 shared port `8976` 可以照抄。
5. **`read_mcp_resource` builtin tool** —— MCP 不只有 tools，resources 也是协议核心。Alva `resources.rs` 已有 MCP resource 读取实现，但没暴露成 engine tool。

子文件会给每一点的详细建议。
