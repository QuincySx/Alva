# 工具清单 —— 所有 builtin 工具分类

> 按功能分组列出所有已还原的 Amp builtin 工具。每项给出 spec 摘要、用途、关键限制。
> 具体 prompt 全文见原始 `strings.txt`，这里只给结构化摘要。

---

## Core：文件系统 + shell

### Bash (`${Y8}`)

```json
{
  "name": "Bash",
  "inputSchema": {
    "cmd": { "type": "string", "description": "The shell command to execute" },
    "cwd": { "type": "string", "description": "Absolute path to a directory..." }
  },
  "required": ["cmd"]
}
```

- **serial: true** 全局独占
- **disableTimeout: true** 不超时
- **preprocessArgs**: 剥离末尾 `&`
- 预定义的**文件工具偏好规则**在 prompt 里：
  ```
  Use specialized tools instead of ${Bash} for file operations. 
  Use ${Read} instead of cat/head/tail, 
      ${edit_file} instead of sed/awk, 
      ${create_file} instead of echo redirection.
  ```
- 认得以下常见命令类别：`cat/sed/awk/head/tail/less/more/bat/jq/nl/wc/stat`（读类）、`rg/ripgrep/grep/egrep/fgrep/ag/ack/pt`（搜类）、`ls/tree/fd/find`（列目录类）、`python/node/ruby/go`（解释器类）

### Read (`${P8}`)

```json
{
  "name": "Read",
  "inputSchema": {
    "path": { 
      "type": "string", 
      "description": "The absolute path to the file or directory (MUST be absolute, not relative)."
    },
    "read_range": {
      "type": "array",
      "items": { "type": "number" },
      "minItems": 2, "maxItems": 2,
      "description": "An array of two integers specifying the start and end line numbers. Line numbers are 1-indexed. Defaults to [1, 1000]. Examples: [500, 700], [700, 1400]"
    }
  },
  "required": ["path"]
}
```

- **resourceKeys**: `[{key: path, mode: "read"}]`
- 默认读 1000 行，单行超 4096 字节截断 ellipsis
- 单文件超 ~5 MB 直接拒绝
- 二进制文件返回 base64 + mediaType，图片自动识别
- 目录返回 `directoryEntries` 列表，超 20 项截断
- 返回结果带 `trackFiles: [uri]`，file-change-tracker 登记

### Grep (`${ee}`)

```json
{
  "name": "Grep",
  "inputSchema": {
    "pattern":       { "type": "string", "description": "The pattern to search for (regex)" },
    "path":          { "type": "string", "description": "Directory/file to search. Cannot be used with glob." },
    "glob":          { "type": "string", "description": "Glob filter. Cannot be used with path." },
    "caseSensitive": { "type": "boolean" },
    "literal":       { "type": "boolean", "description": "Treat pattern as literal string" }
  },
  "required": ["pattern"]
}
```

- 走 ripgrep 子进程
- 无锁，完全并发
- 路径 vs glob **二选一**（schema 层互斥未强制，在 fn 里判）

### edit_file (`${dr}` / `${_d}`)

```json
{
  "name": "edit_file",
  "inputSchema": X.toJSONSchema(qX),
  "description": "str-replace 风格编辑：path + old_str + new_str + replace_all"
}
```

- **resourceKeys**: `[{key: path, mode: "write"}]`
- `old_str` 必须在文件里存在
- `old_str` 和 `new_str` 必须不同
- 非 `replace_all` 模式下 `old_str` 必须在文件里唯一
- 编辑前自动提示模型"先 Read"（prompt 里硬规则）

### create_file (`${we}`)

```json
{
  "name": "create_file",
  "inputSchema": {
    "path":    { "type": "string" },
    "content": { "type": "string" }
  }
}
```

- 只用于**新建**文件
- 覆盖现有文件只在文件小于 ~250 行时推荐
- 通过 fileChangeTracker 记录

### undo_edit (`${q0T}`)

```json
{
  "name": "undo_edit",
  "inputSchema": {
    "path": { "type": "string" }
  }
}
```

- 回滚最近一次 edit（通过 fileChangeTracker 的 `getLastEdit`）
- 同一文件已独占锁
- 返回撤回的 diff（markdown 格式）

### glob

```json
{
  "name": "glob",
  "inputSchema": {
    "filePattern": { "type": "string" },
    "limit":       { "type": "number", "default": 200 },
    "offset":      { "type": "number" }
  }
}
```

- 走 ripgrep `--files`
- 按 traversal 顺序返回（不按 mtime）

---

## Subagents：委托类

### Task (`${he}`)

```json
{
  "name": "Task",
  "inputSchema": {
    "prompt":      { "type": "string", "description": "The task for the agent to perform..." },
    "description": { "type": "string", "description": "A very short description..." }
  },
  "required": ["prompt", "description"]
}
```

- **disableTimeout: true**
- 子 agent 完成后用 **Gemini 3 Flash** 自动总结
- 父 agent 看不到子 agent 的 tool call 细节
- 什么时候用：
  - 复杂多步任务
  - 会产出大量 token 的操作（子 agent 总结后回传，主上下文省 token）
  - 跨多层应用的改动（frontend/backend/API 并发）
- 什么时候不用：
  - 单一逻辑任务
  - 读单个文件 / 单个 edit
  - 不确定要做什么时（自己决策，不委托）

### Oracle (`${wr}`)

```json
{
  "name": "oracle",
  "inputSchema": {
    "task":      { "type": "string" },
    "context":   { "type": "string" },
    "files":     { "type": "array", "items": { "type": "string" } },
    "thinking":  { "type": "string", "enum": ["low", "high"], "default": "high" }
  }
}
```

- 用高 reasoning 模型（GPT-5.4 级别）
- 一次性调用（zero-shot），没有 follow-up
- 视为"高级顾问意见"，不是指令
- 典型用法：架构决策 / 性能分析 / race condition 调查

### codebase_search_agent / finder (`${vt}`)

```json
{
  "name": "codebase_search_agent",
  "inputSchema": {
    "query": { 
      "type": "string",
      "description": "The search query describing what the agent should find..."
    }
  }
}
```

- **disableTimeout: true**
- 内含完整 LLM 子循环（Task 的一种特化）
- 做"语义级"搜索：把自然语言 query → 多次 grep/read → 综合结果
- 相对 grep 的优势：
  - 处理多步骤发现
  - 理解同义词（auth → authentication/login/jwt/session）
  - 自动跟踪代码链（定义 → 使用点 → 相关文件）

---

## Skill 相关

### load_skill (`${is}`)

```json
{
  "name": "load_skill",
  "inputSchema": {
    "name":      { "type": "string" },
    "arguments": { "type": "string" }
  },
  "required": ["name"]
}
```

- 把对应 skill 的 SKILL.md 内容注入 `<loaded_skill name="...">` 块
- 可能激活额外工具（skill 声明的 `builtinTools`）
- 常见：每个 context window 只 load 一次

### walkthrough / walkthrough_diagram (`${W0T}` / `${lET}`)

- 三阶段（explore → plan → emit）生成 mermaid 交互图
- 父 agent 调 `walkthrough(topic, context)` → 返回 `{diagram: {code, nodes}}`
- 再调 `walkthrough_diagram({code, nodes})` → 渲染成 UI

### code_tour

```json
{
  "name": "code_tour",
  "inputSchema": {
    "baseRevision": { "type": "string", "pattern": "^[0-9a-fA-F]{7,40}$" },
    "focus":        { "type": "string" }
  }
}
```

- 生成按 commit 的"引导式 diff 讲解"
- 背后走 Diff Explainer subagent

### code_review

```json
{
  "name": "code_review",
  "inputSchema": {
    "diff_description": { "type": "string" },
    "files":            { "type": "array", "items": { "type": "string" } },
    "instructions":     { "type": "string" },
    "thinking":         { "type": "string", "enum": ["low", "high"], "default": "high" }
  }
}
```

- Code Reviewer subagent
- 输出 XML 格式的 issue 列表（severity / commentType / fix）

---

## Task 管理

### todo_write (`${H0T}`)

```json
{
  "name": "todo_write",
  "inputSchema": {
    "action":     { "type": "string", "enum": ["create","list","get","update","delete"] },
    "taskID":     { "type": "string" },
    "title":      { "type": "string" },
    "description":{ "type": "string" },
    "repoURL":    { "type": "string" },
    "status":     { "type": "string" },
    "dependsOn":  { "type": "array", "items": { "type": "string" } },
    "parentID":   { "type": "string" }
  }
}
```

- **跨会话持久化**（不只在当前 thread 里）
- 支持 DAG 依赖（`dependsOn`）和父子树（`parentID`）
- `list(ready: true)` 查询所有 blocker 已完成的任务
- prompt 里有硬规则：任务描述要详细到"未来 thread 能接力做"

### create_handoff_context

见 [`../prompts/compaction-recap.md`](../prompts/compaction-recap.md)。

### handoff

```json
{
  "name": "handoff",
  "inputSchema": {
    "goal":   { "type": "string" },
    "follow": { "type": "boolean", "default": false },
    "mode":   { "type": "string", "enum": ["deep", "smart", "rush", ...] }
  },
  "required": ["goal"]
}
```

- 创建新 thread，继承当前 thread 的关键上下文
- `follow: true` → 用户 UI 跳到新 thread
- 见 [`../context/handoff.md`](../context/handoff.md)

### read_thread

```json
{
  "name": "read_thread",
  "inputSchema": {
    "threadID": { "type": "string", "description": "T-{uuid} or ampcode.com URL" },
    "goal":     { "type": "string" }
  }
}
```

- 用 **Gemini 3 Flash** 按 `goal` 从老 thread 提取相关片段
- 不是"拉整个 thread"，是**目标导向的压缩读取**

---

## Web

### read_web_page (`${ly}`)

```json
{
  "name": "read_web_page",
  "inputSchema": {
    "url":          { "type": "string" },
    "objective":    { "type": "string" },
    "forceRefetch": { "type": "boolean" }
  }
}
```

- 无 objective → 返回整页 markdown
- 有 objective → 只返回相关段
- 默认用几天以内的 cache，`forceRefetch: true` 强制 live fetch
- **不支持 localhost**（用 curl via Bash）

### web_search (`${r$}`)

```json
{
  "name": "web_search",
  "inputSchema": {
    "objective":      { "type": "string", "description": "broader task or research goal" },
    "search_queries": { "type": "array", "items": { "type": "string" } },
    "max_results":    { "type": "number" }
  }
}
```

- `objective` 是必填的（不是"关键词"，是"为什么搜"）
- 可选 keyword queries 辅助召回

---

## 外部代码理解（Librarian 工具集）

所有这些工具都给 **Librarian** 子 agent 用，不给主 agent 直接用：

### read_github / list_directory_github / list_repositories / search_github / glob_github / commit_search / diff

支持 public + 用户授权的 private repo。接受 `owner/repo` 或 `https://github.com/owner/repo` 作 `repository` 参数。

### 各种 `*_bitbucket_enterprise` 变体

支持自建 Bitbucket Server/Data Center。要先配 `instanceUrl`。

---

## Visualization

### chart (`${U7}`)

```json
{
  "name": "chart",
  "inputSchema": {
    "cmd":        { "type": "string", "description": "Shell command that outputs JSON" },
    "chartType":  { "type": "string", "enum": ["bar", "line", "area"] },
    "xColumn":    { "type": "string" },
    "yColumns":   { "type": "array", "items": { "type": "string" } },
    "title":      { "type": "string" },
    "stacked":    { "type": "boolean" },
    "horizontal": { "type": "boolean" },
    "hoverColumns": { "type": "array" },
    "groupColumn": { "type": "string" }
  }
}
```

- 跑 shell 命令 → 期望 JSON 数组输出 → 渲染
- 最多 100 点每 series（多了 silent drop）
- 用 `jq -c .` 保证单行 JSON
- **只在用户明确要图时才用**（不要自动）

### mermaid

```json
{
  "name": "mermaid",
  "inputSchema": {
    "code": { "type": "string" }
  }
}
```

- 支持的类型：`graph/flowchart` / `sequenceDiagram` / `classDiagram` / `stateDiagram(-v2)` / `erDiagram`
- **不支持** `xychart-beta`
- 不要自定义颜色
- prompt 里：**深色填充 + 浅色 stroke/text**（适合终端）

### image_generation (`${VW}`)

```json
{
  "name": "image_generation",
  "inputSchema": {
    "prompt":          { "type": "string" },
    "inputImagePaths": { "type": "array", "maxItems": 3 }
  }
}
```

- 用 Gemini 3 Pro Image (Nano Banana) 生成
- **只在用户明确要图时用**
- UI 绘图用 mermaid，分析图像用 `analyze_file`

### walkthrough_diagram (`${lET}`)

```json
{
  "name": "walkthrough_diagram",
  "inputSchema": {
    "code":  { "type": "string" },
    "nodes": { "type": "array" }
  }
}
```

- 渲染交互式 walkthrough（点节点显示详细内容）

---

## 文件分析

### analyze_file (`${OET}`)

```json
{
  "name": "analyze_file",
  "inputSchema": {
    "path":           { "type": "string" },
    "objective":      { "type": "string" },
    "context":        { "type": "string" },
    "referenceFiles": { "type": "array", "items": { "type": "string" } }
  }
}
```

- 单文件分析（代码 / 图像 / PDF）
- 用 **Gemini 3 Flash** 跑
- 有 `referenceFiles` 时做对比分析

---

## REPL

### repl

```json
{
  "name": "repl",
  "inputSchema": {
    "binary":         { "type": "string", "description": "e.g., node, python, psql, redis-cli" },
    "args":           { "type": "array", "items": { "type": "string" } },
    "objective":      { "type": "string" },
    "replDescription":{ "type": "string" }
  }
}
```

- 起一个长运行的 REPL 子进程
- 内含一个**嵌套 LLM 循环**
- 嵌套 LLM 的 prompt 明确："Your response text goes VERBATIM to the REPL"
- 不要用 Python 用 `-i`，bash 用 `-i` 才有 interactive mode
- 嵌套 LLM 有 `stop` 工具退出

---

## Orchestrator (Agg Man) 专用工具

这些只在 **orchestrator mode** 的工具集里：

### create_execution_thread (`${$iT}`)
### send_to_execution_thread (`${Yg}`) （带 workflow 参数）
### callback (`${Qg}`)
### read_thread (`${Pv}` ← 注意和前面的同名但不同变量)
### search_threads (`${bd}`)
### create_project (`${JCR}`)
### archive_thread (`${TAR}`)
### restore_thread (`${RAR}`)
### workspace_doc_* (`${NCR}` / `${UCR}` / `${HCR}`)
### slack_read / slack_post (`${XW}` / `${jiT}`)

详见 [`../orchestration/execution-threads.md`](../orchestration/execution-threads.md)。

---

## Plugin / MCP 动态工具

任何 `.amp/plugins/*.ts` 或 MCP server 注册的工具都按上面的 spec 结构出现，source 分别是 `"plugin"` 或 `"mcp-*"`。
