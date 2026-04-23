# Amp Permission Model

Amp 的许可（permission）模型完全基于**用户配置的规则列表** + **内置默认规则**两层栈，没有 `PermissionMode` 枚举（不像 Claude Code / Alva）、没有 sandbox 隔离，也没有 bash 语义分类器。规则按顺序匹配，**first-matching-rule-wins**。

## 规则的 4 种 Action

从 `amp permissions add <action> <tool> [args...]` CLI 定义反推：

| Action | 行为 | 说明 |
|---|---|---|
| `allow` | 直接放行，无需确认 | 类似 Alva `RuleDecision::Allow` |
| `reject` | 直接拒绝，工具调用失败 | 类似 Alva `RuleDecision::Deny` |
| `ask` | 弹出确认 UI（TUI 模式），执行模式下失败 | 类似 Alva `RuleDecision::Ask` |
| `delegate` | 委托外部二进制决策（`--to <program>`，程序必须在 `$PATH`） | **Alva 没有的功能** |

代码证据（CLI action 到 exit code 映射，`u40` 函数）：

```js
function u40(T){
  if(T.action==="ask")return 1;     // exit 1
  if(T.action==="reject")return 2;  // exit 2
  return 0;                          // allow / delegate
}
```

这是 `amp permissions test` 的退出码语义，同时也暗示内部判断结果只有这 3 档，`delegate` 是 allow 的特例（返回前先调外部程序）。

## 规则匹配语法

从 `permissions.toml` 模板（line 64760）提取的**完整示例**：

```toml
# Permission Rules
# First matching rule wins.
#   allow Bash --cmd 'ls*'
#   reject Bash --cmd '*rm -rf*'
#   # Allow fetching Jira issues only project TEST
#   allow mcp__atlassian__jira_fetch_issue --issue_key "TEST*"
#   ask mcp__atlassian__jira_fetch_issue
#   # Ask for all tool uses
#   # Invoke amp-permission-helper (must be on $PATH) to decide for all tools
#   delegate --to amp-permission-helper '*'
#   amp permissions test Bash --cmd 'ls*'
#   amp permissions test mcp__atlassian__jira_fetch_issue --issue_key "TEST*"
# Full reference here: https://ampcode.com/manual/appendix#permissions-reference
```

几个关键点：

1. **每条规则格式**：`<action> <tool-name> [--arg 'glob'] [--arg2 'glob2'] ...`
2. **Glob 支持**：参数值里用 `*` 匹配任意字符（`'ls*'`、`'*rm -rf*'`、`"TEST*"`）
3. **MCP 工具全名**：`mcp__<server>__<tool>`（两个下划线分隔），如 `mcp__atlassian__jira_fetch_issue`
4. **工具名本身也支持 glob**：`'*'` 可以匹配所有工具，`'mcp__playwright__*'` 匹配一个 MCP server 的全部工具（从 `add` 子命令 description 推断："Tool name (supports globs like \"mcp__playwright__*\")"）

## 评估顺序：User → Built-in

关键函数 `S40`（`amp permissions test` 的入口）反编译伪代码：

```js
async function S40(T){
  let t = p40(T.remainingArgs);                    // 解析 --cmd/--issue_key 等参数
  let r = await T.settings.get("permissions", T.scope) ?? [];
  let e = Ev(r);                                    // schema 校验
  let h = lLT(e.data);                             // 规则编译
  // 先查 user 规则
  let a = await K7(T.toolName, t, h, T.context, V7, T.threadId, "user");
  // user 没命中才回退到 built-in $N
  if(!a.matchedEntry) {
    a = await K7(T.toolName, t, $N, T.context, V7, T.threadId, "built-in");
  }
  R.outputResult(T, t, a);
  T.exit(u40(a));
}
```

核心流程：

1. 读 user 配置 `settings.get("permissions", scope)`
2. 对 user 规则列表**按顺序**匹配（`K7` 函数，未全展开但从 API 形状看应是 `for (rule in rules) if match return rule`）
3. 若 user 规则全未命中，再用内置默认规则 `$N` 做同样匹配
4. `matchedEntry` 字段标记命中的 rule，`source` 字段标记 `"user"` 或 `"built-in"`
5. **输出包含 `matchedRule` / `ruleSource`**（见 `_40` 函数中的 `matched-rule: ${t.matchIndex}`）——方便调试

**推论**：Amp 的规则**没有 deny > ask > allow 优先级**（Alva 有），纯粹按"列表里谁在前谁赢"。所以把 `reject *rm -rf*` 放 `allow Bash *` 后面会失效——用户需要自己维护顺序。

## 两层 Scope：Workspace vs Global

所有子命令都支持 `-w/--workspace`：

| 不带 `-w` | 带 `-w` |
|---|---|
| 写到全局 settings（XDG_CONFIG_HOME 下） | 写到工作区 settings（`.amp/settings.json` 或类似） |
| Scope = `"global"` | Scope = `"workspace"` |

代码：`scope: r.workspace ? "workspace" : "global"`

Amp 似乎**不做自动合并**——从 `settings.get("permissions", scope)` 的调用来看，每次只查一个 scope。这和 Alva 的 workspace/global 层级加载模型可能不一样。

## Context 维度：`thread` vs `subagent`

`amp permissions test` 有 `-c/--context <context>` 选项：

> Execution context (thread or subagent)

这说明 Amp 的规则可以按"主 agent 跑" vs "Task 子 agent 跑"分开——例如你可以**允许主 agent 用 Bash，但禁止 subagent 用 Bash**。`K7` 函数签名里第 4 个参数就是 `T.context`。

## `amp permissions` 子命令全集

CLI 入口（`d40` 函数注册）：

```
amp permissions list        [--json] [--builtin] [-w]
amp permissions test <tool> [--context thread|subagent] [-t <thread-id>] [--json] [-q] [-w] [<tool-args>...]
amp permissions edit        [-w]
amp permissions add <action> <tool> [--to <program>] [--context <type>] [-w] [<tool-args>...]
```

### `list` — 列出规则

```js
async function _40(T){
  // --builtin 只看内置；否则读 user settings
  let R = T.builtinOnly ? $N : await T.settings.get("permissions", T.scope) ?? [];
  if(T.json) T.stdout.write(JSON.stringify(R, null, 2));
  else T.stdout.write(bG(R));  // bG 是 pretty-print 函数
}
```

`$N` 是**内置默认规则常量**，代码里直接 import。查不到具体内容（二进制里是对象字面量），但从 `--builtin` flag 存在推测应包含若干高危命令拒绝规则。

### `test` — 模拟评估

```bash
amp permissions test Bash --cmd ls                               # 测 user 规则
amp permissions test --context subagent Bash --cmd ls            # 指定 subagent 上下文
amp permissions test mcp__atlassian__jira_fetch_issue --issue_key "TEST*"
```

输出（text mode，`m40` 函数）：

```
tool: Bash
arguments: {"cmd":"ls"}
context: subagent
action: allow
matched-rule: 2          # 命中的规则 index
source: user             # user 或 built-in
```

退出码：`allow=0, ask=1, reject=2`（见上面 `u40`）。**这让 CI / shell 脚本可以直接 `if amp permissions test ...; then ...`** —— 设计很干净。

### `edit` — 交互式编辑

打开 `$EDITOR`，保存后做 schema 校验，**校验失败时把错误注释插在对应行之前，重新打开**，最多 3 次重试：

```js
// 从 KQT 函数反编译
do {
  write(h, content);
  spawn(editor, h);
  let A = read(h);
  let C = VQT(A);       // 解析 + 校验
  if (C.success) {
    settings.set("permissions", C.entries, scope);
    break;
  }
  content = C.contentWithErrors;   // 带 `# Error: ...` 注释的版本
  if (++i > 3) {
    stderr.write("aborting, errors unresolved after multiple edit attempts");
    exit(1);
  }
} while (true);
```

`contentWithErrors` 里错误会以 `# Error: <msg> at line <n>` 形式**插在坏行之前**。这是很友好的 UX：用户不会丢失自己写的内容，只是要修。

### `add` — 追加规则

```js
// A40 函数核心逻辑
let existing = await R.get("permissions", t) ?? [];
let a = [r.data[0], ...existing];  // 新规则插到**最前面**
await R.set("permissions", a, t);
```

**关键：新规则插到列表最前，不是最后。**这是有意为之：结合"first-match-wins"语义，新加的规则优先级最高。

## Delegate 机制（Amp 独有）

```
delegate --to amp-permission-helper '*'
```

委托给一个外部可执行程序。程序必须在 `$PATH` 上。这让**企业可以插入自己的策略引擎**（OPA、自家审计系统、飞书审批流）。

没找到 `delegate` 具体执行逻辑的反编译（可能被 minify 得很厉害），但从命名看应该是：

1. 把 `{tool, args, context, threadId}` 序列化成 JSON 喂给 stdin
2. 程序返回 `allow` / `reject` / `ask`
3. Amp 按返回结果执行

## `--dangerously-allow-all` 全局 Kill Switch

从 CLI options 定义（`i40` 数组）：

```js
{
  long: "dangerously-allow-all",
  description: "Disable all command confirmation prompts (agent will execute all commands without asking)",
  type: "boolean",
  default: !1
}
```

开启后：

- 所有 `ask` 被自动视为 `allow`
- 但 `reject` **仍然生效**（从错误消息 `"tried to run a command that isn't allowlisted. Rerun with --dangerously-allow-all to bypass"` 反推，这个错误只在**非 allow** 命中时抛）

错误消息原文（两处几乎相同）：

```
Error: The ${Y8} tool tried to run a command that isn't allowlisted.
Rerun with --dangerously-allow-all to bypass, or add to the command allowlist
in permissions (https://ampcode.com/manual#permissions).
```

## Execute Mode 下的硬退出

CLI 非交互模式（无 TTY / 输出被 pipe）下，任何需要 user consent 的工具会直接让 agent 退出：

```js
if (E.length > 0) {
  TT.warn("Tools require user consent - exiting execute mode", {
    blockedTools: E.map((B) => ({name: B.name, id: B.id}))
  });
  // ... 打错误消息 ... process exits
}
```

状态机里这个分支是 `tool_result.run.status === "blocked-on-user"`。**推论**：Amp 执行模式下不能交互 prompt 用户，只能全 allow/reject——这是"执行流水线"设计（CI 里跑 Amp）。

## 对 Alva 的启发

对比 `alva-agent-security/src/` 现状：

### 1. 可以抄：**delegate action**

Alva 目前只有 `Allow / Deny / Ask` 三档。加一个 `Delegate { program: PathBuf }` 变体让企业用户接入自家策略引擎，成本极低：

```rust
// rules.rs
pub enum RuleDecision {
    Allow,
    Deny,
    Ask,
    Delegate { program: String },  // 新增
}

// 执行时（在 guard.rs 或 middleware 里）
RuleDecision::Delegate { program } => {
    let out = std::process::Command::new(program)
        .arg("--tool").arg(tool_name)
        .arg("--args").arg(serde_json::to_string(&args)?)
        .arg("--context").arg(context)
        .output()?;
    match std::str::from_utf8(&out.stdout)?.trim() {
        "allow" => RuleDecision::Allow,
        "deny" => RuleDecision::Deny,
        _ => RuleDecision::Ask,
    }
}
```

这个功能很"企业向"但实现简单，可以直接抄。

### 2. 可以抄：**`permissions test` 子命令**

Alva 目前没有"不跑工具、只看会不会被拦"的 dry-run 入口。Amp 的 `amp permissions test` + 分级退出码（0/1/2）设计得很好，让 CI 脚本能直接：

```bash
if alva permissions test Bash --cmd "rm -rf $TARGET"; then
  echo "This would be auto-approved, aborting"
  exit 1
fi
```

在 `alva-app-cli` 加一个 `permissions test` 子命令，调用 `alva-agent-security::PermissionRules::check(tool, args)`，按 decision 返回 exit code。

### 3. **不要抄：first-match-wins**

Amp 的 first-match-wins + "add 插到最前"让规则列表行为不直观。比如用户先 `allow Bash *` 然后 `reject Bash *rm -rf*`，第二条规则**永远不会生效**（因为第一条已经全匹配）。

Alva 现在的 `deny > ask > allow > default(ask)` 优先级**更安全**（高危规则总是赢），应当保留。

### 4. 可以抄：**edit 命令的 schema 错误注释循环**

Alva 目前编辑规则得手动改 JSON 文件，校验失败的话错误散在日志里。抄一下 Amp 的做法：

- 打开 editor
- 保存后校验
- **失败时在出错行前面插入 `# Error: <msg>`** 并重新打开
- 最多 3 次，还不行就放弃

UX 提升巨大，实现也不复杂（`contentWithErrors` 拼接逻辑反编译里看得很清楚）。

### 5. **缺失但可以考虑**：Amp 的 `context` 维度（thread vs subagent）

Alva 的 `PermissionRules` 目前是**全局一份**。Amp 允许按"主 agent 还是子 agent"分两套规则——这对 `Task`-like 分派场景很有用（让 subagent 权限小于主 agent，天然沙箱效果）。

实现建议：

```rust
pub struct PermissionRule {
    pub tool_pattern: String,
    pub decision: RuleDecision,
    pub context: Option<RuleContext>,  // 新增
}

pub enum RuleContext {
    Main,     // 只对主 agent 生效
    Subagent, // 只对 Task/子 agent 生效
    // None (未设置) = 两者都生效
}
```

### 6. 对比表

| 维度 | Amp | Alva 现状 | 建议 |
|---|---|---|---|
| 规则优先级 | first-match-wins | deny > ask > allow | **保留 Alva** |
| Actions | allow/reject/ask/delegate | Allow/Deny/Ask | 加 `Delegate` |
| Scope | global + workspace | 单层 | 分层（workspace 覆盖 global） |
| Context | thread/subagent | 全局 | 加 context 字段 |
| Built-in 规则 | 有 `$N` fallback | 无 fallback | 加一组 built-in deny rules（`rm -rf /`、`:(){ :\|:& };:` 等） |
| Plan mode | 无 | 有（`PermissionMode::Plan`） | **保留 Alva**——这是 Alva 比 Amp 强的点 |
| Sandbox | 无 | 有（`SandboxConfig` + Seatbelt） | **保留 Alva** |
| CLI `test` 子命令 | 有 | 无 | **值得加** |

总结：Amp 的规则系统**表达力更强**（glob arg 匹配 + delegate 委托），但**默认安全性弱**（无 sandbox、无 classifier、无 plan mode）。Alva 应当保持 "deny 优先 + sandbox + plan mode" 的安全骨架，同时吸收 Amp 的 delegate / context / test 三个优点。
