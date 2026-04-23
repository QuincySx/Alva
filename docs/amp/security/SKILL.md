---
name: amp-security
description: Amp 的 permission / security / sandbox 系统反编译分析。覆盖 rules DSL（allow/reject/ask/delegate）、amp permissions 子命令（list/test/edit/add）、--dangerously-allow-all 绕过、secret 文件保护、[REDACTED:xxx] 标记、builtin 默认规则 $N。需要了解 Amp 怎么做许可控制或对比 alva-agent-security 时加载。
trigger_words:
  - amp permission
  - amp security
  - dangerously-allow-all
  - allowlist
  - permission rules
  - redacted
  - secret file
  - plan mode
  - builtin permissions
  - K7T permission
  - $N
  - reject Bash
  - delegate permission
---

# Amp Security / Permission System

Amp permission / security 的反编译分析结果。**Amp 没有 sandbox**（全量依赖 rules + user approval + `--dangerously-allow-all` 开关），也**没有 `PermissionMode` 这种枚举**，这和 Alva 现状差别很大。

## 文件索引

| 文件 | 内容 | 何时读 |
|---|---|---|
| `./permission-model.md` | 规则 DSL 语法 + 评估优先级 + `amp permissions {list,test,edit,add}` 子命令 + user/built-in 两层查询 | 想复刻 Amp 的 allowlist/denylist 行为 |
| `./bash-classifier.md` | Amp 没有硬编码 classifier，用 prompt + rules 组合。Prompt 里的 "destructive commands" 规范 | 想对比 Alva 的 `BashClassifier::is_destructive` |
| `./sandbox.md` | **关键发现：Amp 没有 sandbox**。靠 rules + user approval + `--dangerously-allow-all` 兜底 | 想确认 Amp 是否用 Seatbelt（答：否） |
| `./sensitive-files.md` | `[REDACTED:amp-token]` / `[REDACTED:github-pat]` 标记 + `reading-secret-file` 错误码 + `<secret-file-instruction>` prompt | 想看 Amp 怎么处理 `.env`、API token |

## 快速决策树

**想知道 rules 怎么写？** → `permission-model.md`（四种 action：`allow` / `reject` / `ask` / `delegate`）

**想跳过所有确认？** → `--dangerously-allow-all` flag，见 `permission-model.md`

**secret 怎么被保护？** → `sensitive-files.md`，Amp 靠**低层 redaction 管道**（不在 prompt 里做）

**plan mode 行为？** → Amp **没有 plan mode**（与 Claude Code 不同）。只有 `reject` 规则可以整体禁用工具。

## 核心洞察（不用 load 子文件就能用）

1. **4 个 action：`allow` / `reject` / `ask` / `delegate`**（对比 Alva 的三个 `Allow/Deny/Ask`，多一个 `delegate` 委托外部二进制决策）
2. **First-matching-rule-wins**：规则顺序敏感，不像 Alva 的 `deny > ask > allow > default(ask)` 优先级
3. **双栈查询：user rules 先，built-in `$N` 后**：user 没匹配才 fallback 到内置（see `_40` / `S40` 函数）
4. **规则按 tool-name + arg glob 匹配**：如 `allow Bash --cmd 'ls*'`、`reject Bash --cmd '*rm -rf*'`、`allow mcp__atlassian__jira_fetch_issue --issue_key "TEST*"`
5. **workspace vs global 双 scope**：所有 `amp permissions` 子命令都接 `-w/--workspace` flag；workspace 规则覆盖 global
6. **delegate action = 外部二进制决策**：`delegate --to amp-permission-helper '*'` —— 把 permission 判断委托给 `$PATH` 上的二进制，可以接入企业 OPA/custom 策略
7. **无 sandbox、无 classifier**：Amp 彻底走"rules + 用户确认"路线，不做 OS 级隔离、不做 bash 命令语义分析
8. **context 维度**：rules 可以按 `--context thread` / `--context subagent` 限制生效范围（主 agent 和 subagent 规则可以分开）
9. **REDACTED 标记是低层管道做的**：不是 prompt 里让模型自查——实际在文件读取层就替换成 `[REDACTED:amp-token]` 这种占位，模型收到的就已经是脱敏过的

## 常见问答

**Q：Amp 有 sandbox 吗？**
A：**没有**。strings 里搜遍 `Seatbelt`/`sandbox-exec`/`sandbox-preview` 都没有命中。它走的是"全量依赖 allowlist + 用户确认"路线。对比 Claude Code 的 `.agents/preview` / Codex 的 Seatbelt，Amp 更"裸奔"——但它有 `reject` 规则 + `[REDACTED]` 兜底。

**Q：Amp 怎么配 allowlist？**
A：三种方式：
1. `amp permissions add allow Bash --cmd 'ls*'`（命令行追加，插到列表最前，因为 first-match-wins）
2. `amp permissions edit [-w]`（打开 `$EDITOR` 编辑，带 schema 校验循环，最多 3 次错误重试）
3. 直接写 settings（`T.settings.set("permissions", entries, scope)`）

**Q：plan mode 什么行为？**
A：Amp 没有 `Plan` 这种全局只读模式。如果要模拟，得手动写一堆 `reject Edit`、`reject create_file`、`reject Bash --cmd '*'` 规则。Claude Code 和 Alva 都有 plan mode，Amp 没有——这是少有的 Amp 缺失 feature。

**Q：`--dangerously-allow-all` 是什么？**
A：CLI 启动时的全局 kill-switch，关闭**所有**工具确认。错误消息里反复推销这个 flag：`Rerun with --dangerously-allow-all to bypass, or add to the command allowlist in permissions`。

**Q：execute mode 和 interactive mode 区别？**
A：执行 `amp` 命令时若不挂 TTY（或传 `-x`），就是 "execute mode"——此时任何需要用户确认的 tool call 会**直接让 agent 退出**并在 stderr 输出错误。Interactive TUI 模式下才会弹 approval UI。代码证据：`TT.warn("Tools require user consent - exiting execute mode", {blockedTools})`

**Q：做了多少工作量？**
A：4 个 .md 文件，从 strings.txt 62300-66300 段提炼的 permission/security 相关代码片段约 30 处。

## 交叉引用

- Amp 的 tool 系统：`../tools/SKILL.md`
- Amp 怎么 assembly prompt（含 secret-file-instruction）：`../prompts/assembly-pipeline.md`
- Alva 的 security crate 现状：`/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-agent-security/src/`
