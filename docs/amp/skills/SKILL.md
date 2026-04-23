---
name: amp-skills-system
description: Amp 自己的 Skill 系统的完整设计 —— 懒加载、SKILL.md 格式、两种渲染模式、builtin skills、MCP includeTools 过滤。想做 skill 系统或懒加载上下文时加载。
trigger_words:
  - skill system
  - SKILL.md
  - skill frontmatter
  - load_skill
  - skill lazy load
  - disable-model-invocation
  - includeTools
  - builtin skills
  - code-tour skill
  - walkthrough skill
  - skill 懒加载
---

# Amp Skills System

Amp 怎么用 "只挂名字不挂内容" 的方式让用户可以有任意数量的 skill 而不爆 context。

## 文件索引

| 文件 | 内容 | 何时读 |
|---|---|---|
| `./design.md` | 懒加载哲学 + 两种渲染模式 (vxR XML / OxR markdown) + load_skill 工具 | 想懂核心机制 |
| `./file-format.md` | SKILL.md frontmatter 字段 + 发现路径 + scripts 目录 + mcp.json includeTools | 想写自己的 skill |
| `./builtin-skills.md` | Amp 内置 3 个 skill (code-tour / code-review / walkthrough) 完整内容 | 想看 skill 实例 |

## 核心设计（不用 load 子文件）

```
System prompt 里只有这个:
┌─────────────────────────────────────┐
│ <available_skills>                    │
│   <skill>                             │
│     <name>web-browser</name>          │
│     <description>...</description>    │
│     <location>.../SKILL.md</location> │
│   </skill>                            │
│   ...                                 │
│ </available_skills>                   │
└─────────────────────────────────────┘
     ← 几百 tokens，不管多少 skill

LLM 需要时调 load_skill({name}):
         │
         ▼
┌─────────────────────────────────────┐
│ <loaded_skill name="web-browser">    │
│ (SKILL.md 完整内容注入对话)           │
│ </loaded_skill>                      │
└─────────────────────────────────────┘
```

## 关键机制速查

- **三级存储路径**：`builtin:///skills/*`、`<workspace>/.agents/skills/*`、`~/.config/agents/skills/*`
- **Frontmatter 必填**：`name`, `description`
- **Frontmatter 可选**：`disable-model-invocation` (从 prompt 隐藏) / `model` / `severity-default`
- **两种渲染**：normal → XML `<skill>` tags / deep → markdown bullets
- **Scripts 目录 + cwd 注入**：skill 加载时 prompt 追加 `cwd: /path/to/skill` 指令
- **mcp.json includeTools 强制过滤**：不写就全进 context（17K+ tokens 浪费）
- **`builtinTools` 激活**：skill 加载后才暴露对应工具
- **"Only load once per context"**：prompt 硬规则，不重复 load

## 三个内置 skill（速查）

| Skill | 激活工具 | 用途 |
|---|---|---|
| `code-tour` | `code_tour` | 按 commit 的引导式 diff 讲解 |
| `code-review` | `code_review` | 严格 code review（XML 输出） |
| `walkthrough` | `walkthrough` + `walkthrough_diagram` | 三阶段 mermaid 交互图 |

## 对 alva-protocol-skill 的启发

你们已有"三级加载"方向对。对照 Amp 确认：

- 两种渲染模式（normal / deep）都有？
- `disable-model-invocation` 字段？
- `includeTools` MCP 过滤？
- `builtinTools` skill 激活工具？
- Scripts 目录约定 + 自动 cwd 注入？
- "Only load once per context" 硬规则写进 prompt？
