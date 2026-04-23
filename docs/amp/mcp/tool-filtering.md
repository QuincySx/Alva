# MCP Tool Filtering & Naming

Amp 对 MCP tools 有两个硬性设计决策：**强制 name 前缀**（避免和 builtin / 其它 server 的 tool 同名）、**`includeTools` glob 过滤**（否则上下文会爆）。

## Naming convention

MCP tools 在注册到主 tool registry 时被前缀为：

```
mcp__<server_name>__<tool_name>
```

**两个下划线分隔**（不是一个，避免和 tool 自己名字里的下划线混）。

反编译里 permissions 规则的例子把这个 naming 写得很清楚：

```
# from strings.txt:64763
allow mcp__atlassian__jira_fetch_issue --issue_key "TEST*"
ask   mcp__atlassian__jira_fetch_issue
```

以及 CLI 测试命令：

```
amp permissions test mcp__atlassian__jira_fetch_issue --issue_key "TEST*"
```

prompt 里呈现给 LLM 的 tool 名字就是 `mcp__atlassian__jira_fetch_issue`，模型以标准 function-calling 语义调用；dispatcher 拆前缀回 `(server=atlassian, tool=jira_fetch_issue)` → 走 MCP client。

## `includeTools` 过滤

反编译里的原话（skill authoring 指南）：

> ### ALWAYS Filter MCP Tools
>
> **This is critical.** MCP servers often expose many tools (chrome-devtools has 26 tools = 17,700 tokens). Always use `includeTools` to expose only what the skill needs.
>
> Ask the user: "Which tools from this MCP do you actually need?"
>
> ```json
> "includeTools": ["navigate_page", "take_screenshot", "click"]
> ```
>
> This reduces token cost by 90%+ and keeps the skill focused.
>
> - `includeTools`: **Always set this.** Glob patterns for which tools to expose. Do not guess tool names; use web search to find the tool names if in doubt.

**17700 tokens = ~13% of 128K context** 只为了列 tool specs，还没跑任何 tool。这是真实问题。

## Glob 语义

反编译里看到 UI 展示就是原样数组 join，没看到 glob matcher 的具体实现。但"Glob patterns"这个词组 + 典型 MCP tools 的 snake_case 命名 → 几乎确定是 Unix-style glob（`*`、`?`、`[abc]`，可能 `**` 不支持因为 MCP tool name 是扁平的）。

实际效果估计：

```json
"includeTools": [
  "navigate_*",       // navigate_page, navigate_back, navigate_forward
  "take_*shot",       // take_screenshot
  "!experimental_*"   // 反编译里没看到 negation，推测不支持
]
```

## 过滤发生的时机

Tools listing 成功之后，在 UI 展示和暴露给 LLM **之前**。

反编译里看到过滤逻辑的片段（UI 列表里显示的 `includeTools: ...` 标签）：

```js
// from strings.txt:63984
if (b || m || n.includeTools && n.includeTools.length > 0) {
  let d = [];
  if (b) d.push(b);
  if (m) d.push(m);
  if (n.includeTools && n.includeTools.length > 0)
    d.push(`includeTools: ${n.includeTools.join(", ")}`);
  // 渲染 "└─ skill: web-browser | deferred until skill load | includeTools: navigate_page, take_screenshot"
}
```

UI 显示 `includeTools` 标签时连原始 glob 都不匹配结果显示（即展示原配置）。实际过滤发生在 `toolService.getToolsForMode()` 聚合路径里（反编译推断，因为 source 枚举有 `mcp-*` 分类）。

## Deferred loading（skill-bundled 专属）

反编译里的 tool UI：

```js
let g = d.spec.meta?.deferred === true;
let k = g ? "  ◌ " : "  ✓ ";  // ◌ 空心圆 = deferred, ✓ = 可用
```

skill-bundled MCP server 的 tools 都带 `meta.deferred: true`，即使 MCP 已经连上、tools 已经 list 出来了，在 LLM prompt 里也**不列名字**（节省 token）。只有 skill 被 load 时 deferred flag 才解除，tools 才进可见池。

文档原话：

> The MCP starts at Amp startup but tools stay hidden until the skill loads.

## 整体 tool 源分类

反编译里的 tool source 枚举（tools 列表打印顺序）：

```js
// from strings.txt:64781
let e = ["builtin", "mcp-workspace", "mcp-global", "mcp-flag", "mcp-other", "toolbox", "plugin", "other"];
```

对应到 `amp tools list` 输出：

```
Built-in Tools
  Bash, Read, Grep, edit_file, create_file, ...

MCP Servers (workspace)
  project-mcp: ...

MCP Servers (global)
  context7: 3 tools
    ✓ resolve-library-id — Resolves package name to Context7 library ID
    ✓ get-library-docs — Fetches docs for a library
    ◌ (filtered out tools)
  postgres: 1 tool (connected)
  sourcegraph: awaiting approval

Toolbox Tools (path/to/toolbox)
  ...

Plugin Tools
  ...
```

**每个 source 独立前缀**：`mcp__context7__resolve-library-id`、不是 `mcp-global__context7__resolve-library-id`。source 分类只影响显示和 permissions 规则优先级，不影响 name。

## 对 Alva 的启发

当前 `alva-protocol-mcp::McpToolAdapter::new()` 构造的名字：

```rust
// tool_adapter.rs:32
let full_name = format!("mcp:{}:{}", info.server_id, info.tool_name);
```

用 `:` 分隔。这和 Amp 的 `__` 有几个差异值得讨论：

1. **兼容性**：LLM 对函数名字符集有期待。OpenAI function calling 要求 name `^[a-zA-Z0-9_-]{1,64}$`，`:` 不在允许集合里，**会被 SDK reject 或某些 provider 默默截断**。这是个 bug。Alva 应该立刻换成 `__` 或 `-`。
2. **显示**：permissions 规则里 `mcp__atlassian__jira_fetch_issue` 比 `mcp:atlassian:jira_fetch_issue` 在 glob 规则里更好匹配（`:` 在很多 glob 实现里是特殊字符）。

建议的 name 格式：

```rust
// 推荐：和 Amp 对齐，向下兼容性好
fn tool_name(server_id: &str, tool_name: &str) -> String {
    format!("mcp__{}__{}", server_id, tool_name)
}
```

`includeTools` 过滤实现建议：

1. `McpServerEntry` 加字段：
   ```rust
   #[serde(default)]
   pub include_tools: Option<Vec<String>>,
   ```
2. 在 `McpClient` listing tools 之后（而不是在 connect 时），过滤：
   ```rust
   use globset::{Glob, GlobSetBuilder};
   
   fn filter_tools(tools: Vec<McpToolInfo>, patterns: &[String]) -> Vec<McpToolInfo> {
       if patterns.is_empty() { return tools; }
       let mut builder = GlobSetBuilder::new();
       for p in patterns {
           if let Ok(g) = Glob::new(p) { builder.add(g); }
       }
       let Ok(set) = builder.build() else { return tools };
       tools.into_iter().filter(|t| set.is_match(&t.tool_name)).collect()
   }
   ```
3. 把未匹配的 tools **保留在 `McpClient` 里但不返回给 `build_mcp_tools()`**，这样 `mcp doctor` 能展示 "tools available: [a, b, c]; tools exposed: [a, b]"。

Deferred loading 现阶段 Alva 不需要（没有 Amp 那种 skill 系统）。但要留好接口：`build_mcp_tools(client, tools_info)` 应该改成 `build_mcp_tools(client, tools_info, visibility: McpToolVisibility)` 枚举，未来 skill 接入就不用改签名。

**不要**依赖 tool source 分类做 permissions，就用统一的 `mcp__<server>__<tool>` name。Amp 那套 `mcp-workspace / mcp-global / mcp-flag / mcp-other` 是配置来源 taxonomy，不是 tool taxonomy。
