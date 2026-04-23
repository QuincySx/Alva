# MCP Server Discovery & Config

Amp 的 MCP server 从**三个地方**发现并合并：全局设置、workspace 设置、skill bundle。每一处有不同语义（trust / lifecycle）。

## 配置来源

### 1. Global settings (`~/.config/amp/settings.json`)

最常见的地方。结构大致：

```json
{
  "amp.mcpServers": {
    "context7": {
      "command": "npx",
      "args": ["-y", "@upstash/context7-mcp"]
    },
    "postgres": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-postgres", "postgresql://..."],
      "env": { "PGUSER": "orb" }
    },
    "sourcegraph": {
      "url": "https://sourcegraph.example.com/.api/mcp/v1",
      "headers": { "Authorization": "token <sg-token>" }
    }
  }
}
```

反编译里的 CLI 帮助把可能的形态说得很清楚：

```
amp mcp add <name> -- <command> [args...]                 (local MCP server, started with command)
amp mcp add <name> --env KEY=VAL -- <command> [args...]   (local MCP server, with env vars)
amp mcp add <name> <url>                                  (remote MCP server with auto-detected transport)
amp mcp add <name> --header KEY=VAL <url>                 (remote MCP server with HTTP headers)
amp mcp add hugging-face https://huggingface.co/mcp       (OAuth path — 后面 OAuth doc 会讲)
amp mcp add monday --header "Authorization=Bearer <token>" https://mcp.monday.com/sse
```

注意 `--header` **不能**和 `--env` / `command` 混用：
> HTTP headers cannot be used with command-based MCP servers. Use --env instead.

### 2. Workspace settings (`.amp/settings.json`)

同样的 `amp.mcpServers` 字段但在仓库根目录下。**安全关键**：

> MCP servers added to workspace settings (.amp/settings.json) require **explicit approval** before they can run. This is a security measure to prevent untrusted code execution.

反编译里看到状态机有 `awaiting-approval` 和 `denied`，以及：

```
To fix: Add "amp.mcpTrustedWorkspaces": ["<workspace-root>"] to <settings.json>
```

两种 approve 路径：
- `amp mcp approve <name>` → 调用 `mcpService.approveWorkspaceServer(name)`
- 在 global settings 里加 `amp.mcpTrustedWorkspaces: [...]`，整个 workspace 信任（workspace 里所有 MCP server 都放行）

默认 workspace MCP server 呈现为：
```
⊘ <name> awaiting-approval
  └─ Awaiting approval
```

### 3. Skill bundled (`.agents/skills/<name>/mcp.json`)

反编译原文：

> Skills can bundle MCP servers with an `mcp.json` file. The MCP starts at Amp startup but tools stay hidden until the skill loads.

```
.agents/skills/web-browser/
├── SKILL.md
└── mcp.json
```

`mcp.json` 结构跟 settings 里的 `mcpServers` 几乎一样，但**默认带 `includeTools`**：

```json
{
  "chrome-devtools": {
    "command": "npx",
    "args": ["-y", "chrome-devtools-mcp@latest"],
    "includeTools": ["navigate_page", "take_screenshot"]
  }
}
```

双重懒加载语义：
- MCP 进程在 Amp 启动时就拉起（省后续 cold start）
- Tools 对 LLM **hidden**，只有 skill 被 load（触发器匹配）之后才暴露

UI 标识：
```
○ web-browser  [skill-backed server]
  └─ skill: web-browser | deferred until skill load | includeTools: navigate_page, take_screenshot
```
（`○` = 未 load；`●` = connected and visible；`◌` = deferred）

### 4. Flag-passed（临时 / debug）

`amp mcp` 命令 + `mcp-flag` 源。反编译里看到的 source 枚举：

```js
let e = ["builtin", "mcp-workspace", "mcp-global", "mcp-flag", "mcp-other", "toolbox", "plugin", "other"];
```

`mcp-flag` 对应 `--mcp-config <file>` 或类似参数临时注入的（具体 flag 名没直接出现，但枚举值意图明确）。`mcp-other` 可能是 plugin / runtime 动态注入。

## 合并语义

`amp mcp list` 会分组显示：

```
Workspace .amp/settings.json
  project-mcp  command  npx -y @some/server

Global ~/.config/amp/settings.json
  context7   command  npx -y @upstash/context7-mcp
  postgres   command  npx -y @modelcontextprotocol/server-postgres
  sourcegraph url     https://sourcegraph.example.com/.api/mcp/v1
```

**同名时 workspace 优先**（`remove` 命令先查 workspace 再 fallback global 这个顺序暗示的）。

## 字段清单（反编译重构）

### Local (stdio) transport

| 字段 | 类型 | 说明 |
|---|---|---|
| `command` | string | 可执行命令（如 `npx`） |
| `args` | string[] | 参数数组 |
| `env` | { [k]: string } | 环境变量（**仅** local，URL 下用 headers） |
| `includeTools` | string[] | glob 过滤，只暴露匹配的 tool，见 `tool-filtering.md` |

### Remote (URL) transport

| 字段 | 类型 | 说明 |
|---|---|---|
| `url` | string | HTTP/SSE endpoint；transport 自动检测 |
| `headers` | { [k]: string } | HTTP headers（如 `Authorization`） |
| `includeTools` | string[] | 同上 |

### OAuth 特殊情形

当 config **只有 `url` 而没有 `headers`** 时，Amp 判断这是 OAuth server：

```js
// 逻辑（反编译推断）
if ("url" in A && !("headers" in A) && !("command" in A)) {
  // OAuth path - consult secretStorage, call getTokens(name), register client if missing
}
```

其他：

- `connect_timeout_secs`：反编译里没直接看到 schema，但状态机有 `"timeout"` 分类 → 有内部默认（推测 30s，与 Alva 一致）。
- 不支持 `auto_connect` flag；**所有** configured MCP server 都在 CLI startup 时进 loading。懒加载只发生在 skill-bundled 的 **tool 可见性**，而不是连接本身。

## 对 Alva 的启发

当前 Alva 的 `McpConfigFile` (`crates/alva-protocol-mcp/src/config.rs:17`) 只支持**一处**扁平结构：

```rust
pub struct McpConfigFile {
    pub servers: HashMap<String, McpServerEntry>,
}
```

具体建议：

1. **分层加载**：新增 `McpConfigLoader` 服务，按优先级从 global / workspace / skill-bundled 各自加载 `McpConfigFile`，然后合并到运行时 `McpClient::register_many()`。merge 策略：name 冲突时 workspace > global，skill-bundled 自己带命名空间避免冲突。
2. **trust flag**：`McpServerEntry` 加 `source: McpSource { Global, Workspace, SkillBundled(skill_name), Flag }` 字段；`McpClient::connect()` 对 `Workspace` 来源先检查 `trust_list`。默认 deny（`state = AwaitingApproval`）。
3. **`includeTools: Option<Vec<String>>`** 加到 `McpServerEntry`。glob crate 已在 workspace 里（`glob-match` 或 `globset`）。
4. **`approval_callback`** trait：让上层 UI（extension）决定怎么弹审批对话框。agent-core 层只负责状态机，不耦合到 GUI。

**切勿照搬** Amp 的 `mcp-flag / mcp-other` 两个枚举 —— 那是 node 插件系统的历史包袱，Alva 应该只用 `Global / Workspace / SkillBundled` 三源。
