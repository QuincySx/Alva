# 自定义子 Agent（Toolbox）

> `.agents/agents/*.md` 目录下用户自定义的 subagent。Amp 把它们包装成工具注册到工具表里。

---

## 文件结构

```
<workspace>/.agents/agents/my-reviewer.md
<workspace>/.agents/agents/auth-specialist.md
<user-home>/.config/agents/agents/global-specialist.md
```

全局 + workspace 两级。同名以 workspace 优先。

## 文件格式

Markdown + YAML frontmatter：

```markdown
---
name: my-reviewer
description: Specialized code reviewer for security-critical changes
model: claude-sonnet-4.5
toolPatterns:
  - Read
  - Grep
  - edit_file
skills:
  - security-patterns
  - secrets-scanner
---

You are a security-focused code reviewer. When asked to review code:

1. First read all changed files thoroughly
2. Flag any ...
3. ...
```

## Frontmatter 字段

| 字段 | 类型 | 必填 | 含义 |
|---|---|---|---|
| `name` | string | ✅ | 工具名（不含空格）|
| `description` | string | ✅ | 工具描述（给 parent agent 看）|
| `model` | string | 否 | 用哪个 model，默认用父 agent 的 |
| `toolPatterns` | string\|array | 否 | glob 白名单，限制子 agent 能用的工具。默认 `["*"]` |
| `skills` | string\|array | 否 | 预加载的 skill 列表（逗号分隔或数组）|

## 装配过程

Amp 发现 `.agents/agents/foo.md`，把它注册成：

```js
{
  spec: {
    name: frontmatter.name,
    description: `${frontmatter.description || "A custom agent"}

Tools: ${(frontmatter.toolPatterns || ["*"]).join(", ")}`,
    inputSchema: {
      type: "object",
      properties: {
        prompt: {
          type: "string",
          description: "The instruction or question for the subagent"
        }
      },
      required: ["prompt"]
    },
    source: { toolbox: sourcePath },
    meta: { disableTimeout: true }
  },
  fn: (toolUse, ctx) => {
    // 启动嵌套 LLM 循环，用 frontmatter.systemPrompt (= markdown body)
    // 工具集按 toolPatterns 过滤
    // skills 预加载
    return new Ls().run(...)
  }
}
```

- inputSchema **固定** `{prompt: string}` —— 所有自定义 agent 都只接受单 string 输入
- description 字段 **自动追加** `Tools: <patterns>` —— 让父 agent 知道子 agent 的能力范围

## Scripts + Execution Environment

如果 `.agents/agents/foo.md` 所在目录有 `scripts/` 子目录，Amp 会往子 agent 的 system prompt 里注入：

```
# Execution Environment

The scripts for this tool are located in: /path/to/dir

When running scripts from this directory using the `Bash` tool:
1. ALWAYS set the `cwd` parameter to "/path/to/dir".
```

这让 custom agent 可以捆绑自己的 shell 脚本而不要求用户手动把它们加到 PATH。

---

## Toolpatterns Glob

支持的模式：

```yaml
toolPatterns: ["*"]                    # 所有工具
toolPatterns: ["Read", "Grep"]         # 只这两个
toolPatterns: ["Read", "edit_*"]       # 通配符
toolPatterns: ["!Bash"]                # 排除（未验证）
```

---

## 和 Amp Orchestrator Agent 的区别

这两个容易混淆：

| | Toolbox custom agent | Orchestrator (Agg Man) |
|---|---|---|
| 定义 | `.agents/agents/*.md` | Amp 二进制内置的 prompt |
| 注册 | 作为 parent agent 的工具 | 独立 runtime mode |
| 调用 | parent agent 发 `prompt: "..."` | 用户在 ampcode.com 直接聊 |
| 工具集 | `toolPatterns` 过滤 | Aggman 专属工具（`$iT`/`Yg`/...）|
| 作用域 | 子 agent，父看不到内部 | 顶级 agent，指挥 execution threads |

---

## 设计启发

Custom agent 是一个 **"让用户重用 Amp 的 agent loop 机制"** 的机制 —— 完整的 system prompt + 工具 + skill 都能自定义，但共用同一份 LLM 循环 / 工具执行器 / 权限系统。

对 Alva：你们的 `SubAgentExtension` 已经在做这个方向，但要注意：

1. **Description 自动追加 Tools 列表** —— 让 parent agent 的调度决策更 informed
2. **Scripts 目录约定 + cwd 注入** —— 解决"agent 想用自己的脚本但 PATH 没它"的老大难问题
3. **Skill 预加载** —— 省去 parent 在 prompt 里写"先 load skill X"的 boilerplate
4. **固定 inputSchema `{prompt: string}`** —— 简化定义，统一接口

---

## 一个完整例子

`.agents/agents/security-reviewer.md`：

```markdown
---
name: security-reviewer
description: >
  Reviews code for common security issues: injection, XSS, CSRF, SSRF, 
  auth bypass, secrets leakage, timing attacks. Returns prioritized list 
  of findings with severity.
model: claude-opus-4.7
toolPatterns:
  - Read
  - Grep
  - codebase_search_agent
skills:
  - owasp-top-10-patterns
---

You are a security-focused code reviewer with deep knowledge of OWASP Top 10,
CWE taxonomy, and common exploitation patterns.

When invoked with a prompt describing a change:

1. Read the full diff / affected files
2. For each change:
   - Check for injection vulnerabilities
   - Check for authentication / authorization issues
   - Check for sensitive data exposure
   - ...
3. Output findings in this format:

<finding>
  <severity>critical | high | medium | low</severity>
  <category>...</category>
  <file>path</file>
  <line>N</line>
  <description>...</description>
  <fix>...</fix>
</finding>

If no issues found, say "No security issues found" and list what you checked.
```

父 agent 看到的工具描述：

```
security-reviewer: Reviews code for common security issues: injection, XSS, 
CSRF, SSRF, auth bypass, secrets leakage, timing attacks. Returns prioritized
list of findings with severity.

Tools: Read, Grep, codebase_search_agent
```

调用：

```
security-reviewer({ prompt: "Review the changes in src/auth/ for security issues." })
```

子 agent 跑在独立 context 里，用 claude-opus-4.7 + 预加载的 owasp-top-10-patterns skill + 受限工具集，返回 findings。
