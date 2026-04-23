# Skill 懒加载设计

---

## 核心设计

```
System Prompt 里只有这个:
┌─────────────────────────────────────┐
│ ## Skills                            │
│                                       │
│ <available_skills>                    │
│   <skill>                             │
│     <name>web-browser</name>          │
│     <description>...</description>    │
│     <location>.../SKILL.md</location> │
│   </skill>                            │
│   <skill>...</skill>                  │
│   ...                                 │
│ </available_skills>                   │
└─────────────────────────────────────┘
     ← 几百 tokens，不管多少 skill

LLM 看到需要某 skill:
         │
         ▼
    调 load_skill({name})
         │
         ▼
┌─────────────────────────────────────┐
│ <loaded_skill name="web-browser">    │
│ (SKILL.md 完整内容)                   │
│                                       │
│ # When to use                         │
│ ...                                   │
│                                       │
│ # Workflow                            │
│ ...                                   │
│                                       │
│ # Bundled scripts                     │
│ - scripts/screenshot.sh               │
│ ...                                   │
│ </loaded_skill>                      │
└─────────────────────────────────────┘
     ← 这时才把完整内容塞进对话
```

---

## 两种渲染模式

### 普通模式（`vxR`）—— 紧凑 XML

```js
function vxR(skills) {
  let filtered = skills.filter(r => !r.frontmatter["disable-model-invocation"]);
  if (filtered.length === 0) return null;
  
  let rendered = filtered.map(r => 
    [
      "  <skill>",
      `    <name>${r.name}</name>`,
      `    <description>${r.description}</description>`,
      `    <location>${r.baseDir}/SKILL.md</location>`,
      "  </skill>"
    ].join("\n")
  ).join("\n");
  
  return [
    "## Skills",
    "In your workspace you have skills the user created. A **skill** is a guide for proven techniques, patterns, or tools. If a skill exists for a task, you must do it. The following skills provide specialized instructions for specific tasks.",
    `Use the ${LOAD_SKILL_TOOL_NAME} tool to load a skill when the task matches its description.`,
    "",
    'Loaded skills appear as `<loaded_skill name="...">` in the conversation.',
    "",
    "<available_skills>",
    rendered,
    "</available_skills>"
  ].join("\n");
}
```

输出示例：

```
## Skills

In your workspace you have skills the user created. A **skill** is a guide 
for proven techniques, patterns, or tools. If a skill exists for a task, 
you must do it. The following skills provide specialized instructions for 
specific tasks.

Use the load_skill tool to load a skill when the task matches its description.

Loaded skills appear as `<loaded_skill name="...">` in the conversation.

<available_skills>
  <skill>
    <name>web-browser</name>
    <description>Use for interacting with web pages, taking screenshots...</description>
    <location>/Users/alice/.agents/skills/web-browser/SKILL.md</location>
  </skill>
  <skill>
    <name>code-tour</name>
    <description>Generate guided walkthroughs of diffs using the code_tour tool.</description>
    <location>builtin:///skills/code-tour/SKILL.md</location>
  </skill>
</available_skills>
```

### Deep 模式（`OxR`）—— Markdown 列表

```js
function OxR(skills) {
  let filtered = skills.filter(r => !r.frontmatter["disable-model-invocation"]);
  if (filtered.length === 0) return null;
  
  return [
    "## Skills",
    "In your workspace you have skills the user created. A **skill** is a guide for proven techniques, patterns, or tools. If a skill exists for a task, you must do it. The following skills provide specialized instructions for specific tasks..",
    "### Available skills",
    filtered.map(r => `- ${r.name}: ${r.description} (file: ${r.baseDir}/SKILL.md)`).join("\n")
  ].join("\n");
}
```

输出示例：

```
## Skills
In your workspace you have skills the user created. ...

### Available skills
- web-browser: Use for interacting with web pages... (file: /Users/alice/.agents/skills/web-browser/SKILL.md)
- code-tour: Generate guided walkthroughs of diffs using the code_tour tool. (file: builtin:///skills/code-tour/SKILL.md)
```

### 为什么两种

- **普通模式**：XML 结构化，LLM 解析更准，tokens 稍多
- **Deep 模式**：Markdown flat bullet，人类可读性好，tokens 少

具体用哪个由 `mode === "deep"` 判断。

---

## `load_skill` 工具完整描述

```
Load a specialized skill when the task matches one of the skill descriptions
from the system prompt.

Use this tool to inject that skill's instructions and bundled resources into
the current conversation. A loaded skill may provide:
- task-specific workflow guidance
- references to scripts, templates, or files in the skill directory
- additional builtin or MCP tools that become available after loading

Call this tool when:
- the user explicitly asks for a skill by name
- the task clearly matches a skill description from the system prompt

You usually only need to load a skill once per context window. After it is
loaded, continue following its instructions instead of reloading it.

- name: The name of the skill to load (must match one of the skills listed below)

Example: To use the web-browser skill for interacting with web pages, call
this tool with name: "web-browser"
```

---

## Skill 激活额外工具

Skill 可以声明 `builtinTools` 数组：

```js
{
  name: "code-tour",
  description: "Generate guided explanations of code changes...",
  frontmatter: { name: "code-tour", description: "..." },
  content: CxR,
  baseDir: "builtin:///skills/code-tour",
  builtinTools: ["code_tour"]    // ← load 此 skill 后才暴露这个工具
}
```

这让**默认工具集可以更小**。只有加载了 `code-tour` skill 后，`code_tour` 工具才出现在工具集里。

---

## Frontmatter `disable-model-invocation`

```yaml
---
name: internal-only-skill
description: Used by humans, not by the model.
disable-model-invocation: true
---
```

设为 `true`：
- 不出现在 prompt 的 `<available_skills>` 列表里
- 模型不会主动调 `load_skill` 请求它
- 但用户仍可通过 CLI / 其他方式触发

---

## 对 Alva 的启发

你们 `alva-protocol-skill` 已经有"三级加载"。对照 Amp：

1. **两种渲染模式**：是否有 `deep` mode 的专用渲染？
2. **`builtinTools` 激活机制**：skill 声明它要用哪些工具，加载后才启用。你们 `activate_tools(names[])` hook 是不是这个。
3. **`disable-model-invocation`**：你们的 skill 有没有这个字段？内部 utility skill 很有用。
4. **"Only load once" 硬规则**：prompt 里明确写死，模型不会重复 load。你们抄。
