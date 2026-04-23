# SKILL.md 文件格式

---

## 三种存储位置

```
优先级从高到低：

1. Workspace skills:
   <workspace>/.agents/skills/<skill-name>/SKILL.md

2. Global user skills:
   ~/.config/agents/skills/<skill-name>/SKILL.md

3. Builtin skills:
   builtin:///skills/<skill-name>  (嵌在 Amp 二进制里)
```

同名 skill 按优先级覆盖。

---

## 基本结构

```
<skill-name>/
├── SKILL.md              ← 必须
├── scripts/              ← 可选：捆绑的脚本
│   ├── run.sh
│   └── helper.py
├── mcp.json              ← 可选：skill 专属 MCP server 配置
└── templates/            ← 可选：模板文件
    └── component.tsx.hbs
```

`SKILL.md` 也支持小写 `skill.md`（大小写 fallback）。

---

## Frontmatter

```yaml
---
name: web-browser
description: >
  Use this skill when the task requires interacting with web pages—taking 
  screenshots, filling forms, extracting content, or automating browser actions.
  Triggers on mentions of "browser", "webpage", "screenshot", "extract from site".

# 可选字段
disable-model-invocation: false
model: claude-sonnet-4.5          # 覆盖默认模型（在此 skill 范围内）
severity-default: medium          # check 类 skill 使用
---
```

**必填字段**：
- `name` —— 唯一标识符，不含空格
- `description` —— 给模型看的"什么时候用我"

**可选字段**：
- `disable-model-invocation` —— 从 prompt 隐藏
- `model` —— 本 skill 运行时切换模型
- `severity-default` —— check 类 skill 的默认严重等级
- 其他自定义字段（skill 类型特定）

---

## Body 内容

frontmatter 之后是 Markdown body，就是 skill 的 "system prompt content"。

```markdown
---
name: web-browser
description: ...
---

# Web Browser Skill

## When to use
- 用户要求查看网页内容
- 需要截图对比
- 需要填表 / 点按钮

## Workflow
1. 先判断用户需求：静态内容 vs 交互？
2. 静态 → 用 read_web_page
3. 交互 → 启动 MCP 浏览器工具链
4. 完成后清理截图 / 临时文件

## Bundled scripts
- `scripts/screenshot.sh` —— 快速截屏工具
- `scripts/form-fill.py` —— 常用表单填写

## Constraints
- 不得截屏含 PII / secrets
- 超时默认 30s
```

加载时，body 以 `<loaded_skill name="web-browser">...</loaded_skill>` 块注入对话。

---

## `mcp.json` —— Skill 专属 MCP 配置

如果 skill 需要额外的 MCP server（像 chrome-devtools），在目录里放 `mcp.json`：

```json
{
  "chrome-devtools": {
    "command": "npx",
    "args": ["-y", "chrome-devtools-mcp@latest"],
    "includeTools": ["navigate_page", "take_screenshot", "click"]
  }
}
```

**硬规则**：必须写 `includeTools`。不写 = chrome-devtools 的全部 26 个工具都进 context（17700 tokens）。

Amp 的 skill 创建指南原文：

> **This is critical.** MCP servers often expose many tools (chrome-devtools 
> has 26 tools = 17,700 tokens). Always use `includeTools` to expose only 
> what the skill needs.
> 
> Ask the user: "Which tools from this MCP do you actually need?"
> 
> ```json
> "includeTools": ["navigate_page", "take_screenshot", "click"]
> ```
> 
> This reduces token cost by 90%+ and keeps the skill focused.

---

## Scripts 目录 + 自动 CWD 注入

Skill 里有 `scripts/` 时，加载后 prompt 自动追加：

```
# Execution Environment

The scripts for this tool are located in: /path/to/skill

When running scripts from this directory using the `Bash` tool:
1. ALWAYS set the `cwd` parameter to "/path/to/skill".
```

这让 skill 可以**自带工具脚本**，不要求用户手动加 PATH 或改 workspace 配置。

---

## 内置 Skill 的特殊结构

Builtin skills 在二进制里是这样的对象（例：code-tour）：

```js
bDT = {
  name: "code-tour",
  description: "Generate guided explanations of code changes with the code_tour tool.",
  frontmatter: {
    name: "code-tour",
    description: "Generate guided explanations of code changes with the code_tour tool."
  },
  content: CxR,                           // 完整 SKILL.md 内容
  baseDir: "builtin:///skills",
  builtinTools: ["code_tour"]             // ← 激活此工具
}
```

`baseDir` 是虚拟 URI `builtin:///skills`（或 `builtin://skills`，二进制里两种都有）。

---

## 动态发现流程

```js
async function discoverSkills(filesystem, workspaceRoot) {
  let skills = [];
  
  // 1. Builtin (已打包在二进制)
  skills.push(...BUILTIN_SKILLS);
  
  // 2. Workspace: .agents/skills/
  let wsDir = path.join(workspaceRoot, ".agents/skills");
  if (await exists(wsDir)) {
    for (let entry of await readdir(wsDir, { withFileTypes: true })) {
      if (!entry.isDirectory()) continue;
      let skill = await loadSkillFromDir(path.join(wsDir, entry.name));
      if (skill) skills.push(skill);
    }
  }
  
  // 3. Global: ~/.config/agents/skills/
  let globalDir = path.join(homedir(), ".config/agents/skills");
  if (await exists(globalDir)) {
    // 同上
  }
  
  return skills;
}

async function loadSkillFromDir(dir) {
  let skillPath = path.join(dir, "SKILL.md");
  if (!exists(skillPath)) skillPath = path.join(dir, "skill.md");
  if (!exists(skillPath)) return null;
  
  let raw = await readFile(skillPath, "utf-8");
  let { data: frontmatter, content: body } = parseFrontmatter(raw);
  
  if (!frontmatter.name || !frontmatter.description) {
    throw new ValidationError("Missing required fields in frontmatter", 
      'Add both "name" and "description" fields to the frontmatter');
  }
  
  return {
    name: frontmatter.name,
    description: frontmatter.description,
    frontmatter,
    content: body,
    baseDir: dir,
    // 动态检测附加资源
    builtinTools: await detectBuiltinTools(dir),
    mcpConfig: await loadMcpJson(dir)
  };
}
```

---

## 发现 + 渲染 时机

在 system prompt 装配流水线（见 `../prompts/assembly-pipeline.md`）的第 7 步：

```js
let skills = await t.skillService.getSkills();
let skillText = isDeep ? OxR(skills) : vxR(skills);
if (skillText) blocks.push({ type: "text", text: skillText });
```

每次 prompt 装配都重新发现。**不缓存**（文件改动立即生效）。

---

## `load_skill` 工具的内部流程

```js
fn: async ({ args }, ctx) => R8(async () => {
  let result = await ZBR(ctx.skillService, args.name, args.arguments, {
    mcpService: ctx.mcpService,
    toolService: ctx.toolService,
    // ...
  });
  
  // ZBR 做的事：
  // 1. 按 name 查 skill
  // 2. 读 SKILL.md body
  // 3. 如果有 mcp.json，注册 MCP server（按 includeTools 过滤）
  // 4. 如果有 builtinTools，激活那些工具
  // 5. 如果有 scripts/，拼 Execution Environment 段
  // 6. 把 <loaded_skill> 块作为 info message 注入 thread
  
  return { status: "done", result: { loaded: args.name } };
})
```

---

## 对 Alva 的启发

你们 `alva-protocol-skill` 有"三级加载"。对照 Amp 细节：

1. **Frontmatter schema 完全照搬** —— 不要自己发明 schema，除非有充分理由
2. **三路径 discovery（builtin + workspace + global）** —— 用户熟悉的约定
3. **`includeTools` 过滤 MCP** —— 这个必须做，token 节省巨大
4. **Scripts 目录 + cwd 注入** —— 开发者体验提升
5. **`builtinTools` 激活** —— 进一步减少默认工具集
