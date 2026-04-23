---
name: amp-checks
description: Amp 的 checks 框架 —— 可插拔 diff-scoped 代码审查系统。每个 check 是一个 markdown 文件（skill 类型变种），由 `code_review` 工具自动发现并并行执行。覆盖文件格式、触发机制、XML 输出、和主 reviewer 的合并逻辑。
trigger_words:
  - check
  - checks
  - code_review
  - code review
  - checkScope
  - checkFilter
  - checksOnly
  - severity-default
  - diff review
  - 可插拔 review
  - custom check
---

# Amp Checks Framework

Amp 的 `code_review` 工具背后有两层架构：

1. **Main reviewer (`code-review` subagent)** —— Gemini 3.1 Pro Preview，读整个 diff，按通用原则提 `<comment>` 列表（见 `../prompts/subagents.md` Code Reviewer 节）
2. **Per-check runners (`codereview-check` subagent)** —— Claude Haiku 4.5，每个用户定义的 check 文件 fork 一个实例，只关注自己那条规则，按 `<issue>` 结构报告

两层并行跑、结果合并去重，和 eslint plugin / git hooks 完全不同 —— checks 是**给 LLM 的 prompt 式 lint 规则**，不是正则也不是 AST。

## 文件索引

| 文件 | 内容 | 何时读 |
|---|---|---|
| `./check-skill-format.md` | check markdown 文件的 frontmatter + body 模板 + 解析 | 想写一个 check |
| `./code-review-integration.md` | `code_review` 工具如何发现 + 调度 + 合并 checks；完整 XML 输出 | 想理解 tool 到 issue 列表的全链路 |
| `./builtin-checks.md` | 内置 checks 清单（**发现：Amp 没有**）+ 为什么 + 用户常用模式 | 想知道 Amp 出厂带什么 / 社区怎么写 |

## 核心洞察（不用 load 子文件就能用）

1. **Checks 不是 skill，但复用 skill 的 frontmatter + body 格式**。file extension 是 `.md`，路径在 `.agents/checks/`（项目）或 `~/.config/{amp,agents}/checks/`（全局），目录名硬编码常量 `ZX="checks"`。
2. **两套不同模型**：Main reviewer 用 Gemini 3.1 Pro（高推理、看全局）；每个 check runner 用 Claude Haiku 4.5（便宜、聚焦一条规则）。**典型的"大模型做协调、小模型做局部扫描"**。
3. **Only-changed-lines guardrail**：check prompt 里强调 "Report issues ONLY for code that was added or modified in this diff"。这是防止 LLM 对 pre-existing code 念经的关键约束。
4. **失败自动重试 1 次**（`huT=1`）；超过仍失败则在 UI 里显示 `status: "error"`，不阻断其他 checks。
5. **checksOnly 模式**跳过主 reviewer，等价于"只跑 lint，不要综合评论"。对 CI 场景友好。
6. **checkScope / checkFilter 两个参数**：前者是目录（只发现这个目录下的 checks），后者是名字白名单（只跑这几个 check）。可组合。
7. **Amp 二进制里没有 builtin checks**。全部由用户在 `.agents/checks/` 放 `.md` 文件，或从 GitHub 装 skill 包附带过来。

## 快速问答

**Q：check 和 skill 什么关系？**
A：**文件格式完全一样**（frontmatter + body），但路径不同、用途不同。skill 在 `.agents/skills/SKILL.md`，按 trigger_words 懒加载到**主 agent** 的上下文；check 在 `.agents/checks/<name>.md`，只在 `code_review` 工具调用时被独立的 check subagent 单独加载执行。同一个工程可以同时有 skills 和 checks。

**Q：怎么加一个自定义 check？**
A：在项目根创建 `.agents/checks/my-check.md`，写 frontmatter（至少 `name` + `description`，可选 `severity-default`）+ body（用自然语言描述要查什么模式、报什么 issue）。`code_review` 工具下次运行自动发现。详见 `check-skill-format.md`。

**Q：check 和 git pre-commit hook 哪个合适？**
A：互补关系。pre-commit 适合快速确定性检查（lint / format / 单元测试）。check 适合"需要 LLM 判断"的模式：业务规则（"不要在 handler 里直接 `tokio::spawn`"）、架构约束（"不要跨 crate 直接引 private"）、文化规则（"禁止 panic!，用 Result"）—— 这些靠正则写不出来。

**Q：一次 code_review 会跑多少个 check？**
A：项目里 `.agents/checks/*.md` 的**全部文件**（去重按 name），除非传 `checkFilter` 白名单。每个 check 起一个独立的 LLM 推理，所以 **10 个 checks = 10 次 Haiku 调用**。并发执行，不排队。

**Q：check 的输出怎么变成最终 review 列表？**
A：check 发 `<checkResult>` XML，里面若干 `<issue severity=... file=... line=...>`。这些 issue 被转成和主 reviewer 一样的 `comment` 结构（加 `source: <check.name>` 字段），和主 reviewer 的 comments 合并、按 severity 过滤（默认丢弃 `low`），最后按文件分组输出。详见 `code-review-integration.md`。

**Q：为什么 check 不是 tool？**
A：tool 是同步 call/return。check 要让 LLM 自己判断，必须是 **subagent + prompt**。Amp 把这个职责切到单独的 subagent spec（`codereview-check`），用小模型、受限工具集（只有 `Read / Grep / glob / Bash`），和主 reviewer 完全隔离。

## 快速决策树

**想理解 check 文件长什么样？** → `check-skill-format.md`

**想理解 `code_review` 工具的完整流程？** → `code-review-integration.md`

**想知道 Amp 自带什么 check？** → `builtin-checks.md`（剧透：没有，全靠用户写）

**想对比 Amp 的 code_review vs Alva 自己做一套？** → `builtin-checks.md` 的"对 Alva 的启发"节

## 与其他 skill 的关系

- `../prompts/subagents.md` —— Code Reviewer (`iuT`) 主 prompt 在这里；本 skill 讲**外层可插拔的 checks 怎么接入**
- `../skills/SKILL.md` —— Amp 的通用 skill 系统；checks 复用同一套 frontmatter 解析代码
- `../tools/SKILL.md` —— `code_review` 本身是一个 builtin tool，其 spec 在 tools 清单里
