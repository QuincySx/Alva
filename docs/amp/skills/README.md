# Skills 目录

> Amp 的 Skill 系统是"懒加载"范式的典范。这个目录讲清楚完整设计。

## 文件清单

| 文件 | 内容 |
|---|---|
| [`design.md`](./design.md) | Skill 懒加载哲学 + 两种渲染模式 |
| [`file-format.md`](./file-format.md) | SKILL.md frontmatter + bundled resources |
| [`builtin-skills.md`](./builtin-skills.md) | Amp 内置的 code-tour / code-review / walkthrough |

## TL;DR

> **Skills 是"磁盘上的 prompt"，不是"内置 prompt"。**
>
> 系统 prompt 里只挂 `name` + `description`（几十到几百 tokens），用户触发或 LLM 判断相关时，通过 `load_skill(name)` 工具**按需加载**完整内容到对话里。
>
> 这让"支持 50 个 skill" 和 "支持 5 个 skill" 的基础 prompt token 成本几乎一样。

## 为什么关键

[AGENTS.md](../../../AGENTS.md) 提到你们有 `alva-protocol-skill` 做"渐进式三级加载"。Amp 的实现是相同思路的**生产级参考**，包括：

- 三种存储位置（builtin / workspace / global）
- 两种渲染模式（normal XML / deep Markdown）
- Skill 可带 scripts + mcp.json
- 可激活额外工具
- "每个 context window 只 load 一次"的硬规则
