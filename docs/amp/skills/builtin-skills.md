# Amp 内置 Skill（`builtin:///skills`）

> Amp 二进制里写死的三个内置 skill。它们展示了 skill 的实际写法。

---

## 1. Code Tour Skill

**路径**: `builtin:///skills/code-tour`

**完整内容**:

```
Generate guided walkthroughs of diffs using the code_tour tool.

Use this skill when you want a structured explanation of what changed and 
why across one or more files.

Call the `code_tour` tool with a `baseRevision` commit hash and optional 
`focus`.

## Output Expectations
- Explain the change story in clear, ordered sections
- Highlight important files and cross-file relationships
- Call out risk areas and follow-up checks
```

**激活的工具**：`["code_tour"]`

**关联 subagent**：Diff Explainer（见 [`../prompts/subagents.md`](../prompts/subagents.md)）

**典型触发**：
```
User: "walk me through what changed since abc123"
Model: load_skill("code-tour") → code_tour({baseRevision: "abc123"})
```

---

## 2. Code Review Skill

**路径**: `builtin:///skills/code-review`

**完整内容**:

```
# Code Review Skill

Run comprehensive code review using the code_review tool.

Call `code_review` tool to perform a comprehensive review of code changes 
or files.

Use this skill when asked to perform a code review or a review of changes 
to code.

## After the Tool Completes

Display the issues as a concise markdown numbered list. Each item is one 
line in this format:
1. source (severity) - [file-basename](file-path#range): one sentence summary

Examples:
1. security (critical) - [auth.ts](src/auth/auth.ts#L10-L15): JWT secret is hardcoded
2. general (high) - [server.ts](src/server.ts#L42): Missing error handling on database connection

If no issues were found, say so briefly.
Mention which checks were run (if any) and their results.
If issues were found, offer to fix them and make it clear how to reply.
```

**激活的工具**：`["code_review"]`

**关联 subagent**：Code Reviewer（见 [`../prompts/subagents.md`](../prompts/subagents.md)）

**典型触发**：
```
User: "review the changes in src/auth"
Model: load_skill("code-review") → code_review({diff_description: "...", files: ["src/auth/"]})
  → 展示结果，按上面的格式
```

---

## 3. Walkthrough Skill

**路径**: `builtin:///skills/walkthrough`（也写作 `builtin://skills`）

**完整内容**:

```
# Walkthrough Skill

Use this skill to create interactive walkthrough diagrams for exploring 
and understanding codebase architecture.

When to use:
- Exploring codebase architecture and structure
- Understanding code flows and execution paths
- Visualizing relationships between components, modules, or services
- Onboarding to unfamiliar codebases
- Documenting complex system interactions

When NOT to use:
- Simple file reading (use Read tool instead)
- Single file analysis without relationship context
- Modifying or editing code
- Quick lookups of specific symbols or functions

The walkthrough process involves two steps:
1. **walkthrough**: Explore the codebase by following references, imports, 
   and call sites to build up an understanding of how components relate
2. **walkthrough_diagram**: Generate a visual diagram based on the 
   exploration results

Start by using the walkthrough tool to explore a starting point, then 
iteratively expand your understanding by following connections. Once you 
have sufficient context, use walkthrough_diagram to visualize the relationships.

Examples of good use cases:
- "Walk me through how authentication works in this codebase"
- "Create a diagram showing the data flow from API request to database"
- "Explore the relationship between the Router and Controller components"
- "Show me how the event system connects publishers and subscribers"
```

**激活的工具**：`["walkthrough", "walkthrough_diagram"]`

**关联**：三阶段 walkthrough prompt（见 [`../prompts/subagents.md`](../prompts/subagents.md)）

---

## 内置 Skill 的对象定义

```js
bDT = {
  name: "code-tour",
  description: "Generate guided explanations of code changes with the code_tour tool.",
  frontmatter: {
    name: "code-tour",
    description: "Generate guided explanations of code changes with the code_tour tool."
  },
  content: CxR,                               // 完整 SKILL.md body
  baseDir: "builtin:///skills",
  builtinTools: ["code_tour"]
};

_DT = {
  name: "code-review",
  description: "Perform a formal code review. ...",
  frontmatter: { ... },
  content: _xR,
  baseDir: "builtin://skills",                 // 注意两种 URI 混用
  builtinTools: ["code_review"]
};

yDT = {
  name: "walkthrough",
  description: "Create an interactive walkthrough diagram...",
  frontmatter: { ... },
  content: uxR,
  baseDir: "builtin://skills",
  builtinTools: ["walkthrough", "walkthrough_diagram"]
};
```

---

## 观察到的设计模式

### 1. **每个 skill 绑一个主工具**

所有三个内置 skill 都是"skill = 工具的使用手册"，工具本身只有在 skill loaded 后才暴露。这让：

- 默认工具集保持小
- 每个工具带详细使用文档（通过 skill body）
- 使用规范可版本化（改 SKILL.md 不改工具实现）

### 2. **Skill body 是"fresh eyes" 的培训材料**

不仅教工具怎么用，还教：
- 什么时候**不要**用（防误用）
- 输出格式（走一致体验）
- 和其他工具的搭配（workflow）

### 3. **URI scheme 统一**

`builtin:///skills/<name>` 让内置和用户 skill 在 `baseDir` 字段上没歧义。Prompt 里一律显示 location，用户 debug 时能知道 "这个 skill 来自哪里"。

---

## 对 Alva 的启发

你们可以直接抄这三个 skill 的"工具 + 手册"模式：

1. **`code_tour` / `code_review` / `walkthrough` 三个工具本身设计** —— 你们如果做 code review 功能，抄这套。
2. **Skill 作为工具的使用手册** —— 减少 tool description 的长度（工具 description 里只写核心，详细规则留在 skill），tool 表瘦一圈。
3. **工具按 skill 激活** —— 默认不暴露 `code_tour`，加载 code-tour skill 才暴露。你们的 `SkillsExtension` 支持这个模式吗？
