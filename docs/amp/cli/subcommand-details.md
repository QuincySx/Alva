# Amp Subcommand Details

逐子树的 **arguments / options / action 语义**。不贴完整 action body（太多逻辑），只贴 CLI surface。

## `amp threads` (alias: `t`, `thread`)

`amp threads` 不带子命令时，默认相当于 `amp threads list`。

### `threads list` (alias: `l`, `ls`)
```
amp threads list [options]
  --include-archived                    Include archived threads in the list
  --installation-id <installationID>    Only list threads for a specific installation ID
```

### `threads new` (alias: `n`)
```
amp threads new [options]
  --visibility <visibility>             private | public | workspace | group
```
打印新建 thread 的 ID，不进 TUI。

### `threads continue [threadIDOrURL]` (alias: `c`)
```
amp threads continue [threadIDOrURL] [options]
  --last                                Continue the last thread directly
  --pick                                (DEPRECATED) picker 现在是默认行为
```
不提供 ID 时默认弹出 picker。

### `threads search <query>` (alias: `find`)
```
amp threads search <query> [options]
  -n, --limit <number>                  Max results (default: 20)
  --offset <number>                     Pagination offset (default: 0)
  --json                                Output as JSON
```
Query 是 DSL（大小写不敏感，path 支持 partial match）。

### `threads visibility [visibility]` (alias: `v`)
```
amp threads visibility [visibility]
```
不带参数打印当前仓库默认 visibility；带参数设置。

### `threads rename <id> <newName>` (alias: `r`)
```
amp threads rename T-abc "New Name"   # 带空格必须 quote
```

### `threads label <id> <labels...>`
```
amp threads label T-abc tag1 tag2     # append 模式，不覆盖已有 label
```

### `threads share <id>` (alias: `s`)
```
amp threads share <id> [options]
  --visibility <vis>                    private | public | unlisted | workspace | group
  --support [message]                   Share w/ Amp team for debugging
```
注意这里 `--visibility` 多一个 `unlisted` 值。

### `threads archive <id>`
```
amp threads archive <id> [--unarchive]
```

### `threads delete <id>`
永久删除（client + server），无 undo。

### `threads handoff [id]` (alias: `h`)
```
amp threads handoff [id] [options]
  -g, --goal <goal>                     Goal/prompt for the handoff (or via stdin)
  -p, --print                           Print thread ID instead of opening TUI
```

### `threads markdown <id>` (alias: `md`)
把完整对话输出为 markdown。

### `threads export <id>`
输出完整 thread payload 为 JSON。

### `threads usage <id>`
显示指定 thread 的成本信息（不同于顶级 `amp usage`，后者是账户 balance）。

---

## `amp mcp`

父命令无 default action，直接 `outputHelp() + exit(0)`。

### `mcp add <name> [args...]`
```
amp mcp add <name> [-- cmd args...]     [options]
  --env <kv>                            KEY=VAL，repeatable
  --header <kv>                         KEY=VAL，HTTP header (URL 型 server)，repeatable
  --workspace                           写入 workspace settings 而非 global
```
Server name 规则：`^[A-Za-z0-9@/_-]+$`。两种用法：

```bash
# 1. Stdio server
amp mcp add context7 -- npx -y @upstash/context7-mcp

# 2. URL server
amp mcp add hugging-face https://huggingface.co/mcp

# 3. With env vars
amp mcp add postgres --env PGUSER=orb -- npx -y @modelcontextprotocol/server-postgres postgresql://localhost/mydb
```

### `mcp list`
```
amp mcp list [--json]
```
合并显示 global + workspace 的所有 server，带 source 标注。

### `mcp remove <name>`
从 settings 文件里删除。

### `mcp doctor [name]`
等 MCP service 初始化，报告每个 server 的状态。带 name 只看一个。

### `mcp approve <name>`
显式批准一个 workspace-scoped MCP server。workspace scope server 默认不自动加载（安全），必须 approve。

### `mcp oauth ...`
父子命令，管 remote MCP 的 OAuth 凭证。

#### `mcp oauth login <server-name>`
```
amp mcp oauth login <server-name> [options]
  --server-url <url>                    (required) MCP server URL
  --client-id <id>                      (required) OAuth client ID
  --client-secret <secret>              不支持 PKCE 时才需要
  --scopes <scopes>                     逗号分隔
  --auth-url <url>                      若 server 不支持自动发现
  --token-url <url>                     若 server 不支持自动发现
```

#### `mcp oauth logout <server-name>`
清除该 server 的 OAuth 凭证。

#### `mcp oauth status <server-name>`
显示当前 OAuth 状态。

---

## `amp permissions` (alias: `permission`)

支持的 action 值：`allow` / `reject` / `ask` / `delegate`。
全局规则写 `~/.config/amp/settings.json` 的 `permissions:`，workspace 规则写 `.amp/settings.json`。

### `permissions list` (alias: `ls`)
```
amp permissions list [options]
  --json                                JSON 输出
  --builtin                             只显示内置规则
  -w, --workspace                       看 workspace rules（默认 global）
```

### `permissions edit`
```
amp permissions edit [options]
  -w, --workspace                       编辑 workspace permissions
```
启动 `$EDITOR` 打开规则 YAML。stdin 可以直接 pipe 规则。

### `permissions add <action> <tool>`
```
amp permissions add <action> <tool> [options]
  --to <program>                        delegate action 时必填
  --context <type>                      thread | subagent
  -w, --workspace
```
规则插到列表开头（**新规则优先**）。`<tool>` 支持 glob：`mcp__playwright__*`。

### `permissions test <tool-name>`
```
amp permissions test <tool-name> [tool-specific-args...]
  -c, --context <context>               thread | subagent
  -t, --thread-id <id>                  For evaluation context
  --json
  -q, --quiet                           Only exit status, no output
  -w, --workspace
```
**不执行工具**，只评估规则。exit code：
- 0 = allow 匹配
- 1 = ask 匹配
- 2 = reject 匹配

Example:
```bash
amp permissions test Bash --cmd ls
amp permissions test --context subagent Bash --cmd ls
```

---

## `amp tools` (alias: `tool`)

### `tools list` (alias: `ls`)
```
amp tools list [options]
  --inspect                             包含完整 tool schema + effective system prompt
  --json                                输出 JSON 数组
  --mode <mode>                         过滤特定 mode 的工具（default: smart）
```
`--inspect` 是需要 `HARNESS_SYSTEM_PROMPT` feature 或 Sourcegraph 内部邮箱才能用。

### `tools show <tool>`
```
amp tools show <tool> [--json] [--mode <mode>]
```
展示单个 tool 的详细 schema。

### `tools make <tool-name>`
```
amp tools make <tool-name> [options]
  --force                               覆盖已有 tool
  --bun                                 Bun/TypeScript (默认)
  --zsh                                 Zsh shell script
  --bash                                Bash shell script
```
scaffold 一个 toolbox tool 到本地目录。`<tool-name>` 命名建议如 `run-tests` / `build-project`。

### `tools use <tool-name>`
```
amp tools use <tool-name> [--only <field>] [--stream] [-- tool-args...]
```
`allowUnknownOption` + `allowExcessArguments` —— 额外参数全部透传给工具。
- `--only <field>` 从结果提取特定字段
- `--stream` 流式增量输出

stdin 支持：管道进 JSON 作为 tool input。

---

## `amp skill` (alias: `skills`)

### `skill add <source>`
```
amp skill add <source> [options]
  --target <dir>                        指定安装目录
  --global                              装到 ~/.config/agents/skills/
  --overwrite                           覆盖同名 skill
  --name <name>                         自定义 local 名字
```
`<source>` 四种形态：
- `@user/skill-name`（GitHub short form）
- `owner/repo`
- git URL
- local path

### `skill list` (alias: `ls`)
```
amp skill list [--json]
```
输出 skill 列表 + 加载错误（parse 失败的 skill 单独列）。

### `skill remove <name>` (alias: `rm`)
```
amp skill remove <name> [--target <dir>]
```

### `skill info <name>`
```
amp skill info <name> [--json]
```
单个 skill 的详细信息。

---

## `amp plugins` (hidden, alias: `plugin`)

### `plugins list` (alias: `ls`)
列出 `.amp/plugins/` 下的插件。需要 `PLUGINS=all` 环境变量，否则提示 "Plugins are disabled (PLUGINS=off)"。

### `plugins exec <plugin> <event>`
```
amp plugins exec <plugin> <event> [--data <json>]
```
直接对某个插件文件发 event（比如 `tool.result`）。`--data` 是事件 payload，必须是合法 JSON。用于插件调试。

---

## `amp review [diff_description...]`

主 review 入口。通过 smart-mode thread 跑代码审查。

```
amp review [diff_description...] [options]
  -f, --files <files...>                只关注这些文件
  -i, --instructions <text>             额外 review instructions
  -s, --check-scope <dir>               搜索 checks 的目录
  -c, --check-filter <checks...>        只跑指定 check 名
  --checks-only                         跳过主 review agent，只跑 checks
  --summary-only                        只生成 diff summary，不做 full review
  --thinking <level>                    high (default) | low
```

还有 `amp review-legacy`（hidden）跑旧的 review 引擎 —— 同样的 options。

---

## `amp live-sync [threadIDOrURL]` (hidden)

实验性：监听 v2 DTW thread，把远端 working tree 改动 mirror 到本地 checkout。

```
amp live-sync [threadIDOrURL] [options]
  --apply <threadIDOrURL>              Apply 当前 snapshot 一次就退出（不长监听）
  --checkout                            差异时自动 checkout 对应 commit
  --skip-checkout                       差异时不提示 checkout
  --worker-url <url>                    覆盖 DTW worker URL

Examples:
  amp live-sync T-5928a90d-d53b-488f-a829-4e36442142ee
  amp live-sync --apply T-5928a90d-d53b-488f-a829-4e36442142ee
  amp live-sync https://ampcode.com/threads/T-5928a90d-d53b-488f-a829-4e36442142ee
```

positional ID 和 `--apply` 互斥。需要 user 有 `V2` feature flag。

---

## `amp usage`

和 `threads usage <id>` 不同 —— 这是账户级别。

```
amp usage
```
显示当前 Amp credit balance + usage。未登录时 exit 1 + 提示 `amp login first`。

---

## `amp update` (alias: `up`)

```
amp update [options]
  --target-version <version>            指定版本（默认 latest）
```
`allowUnknownOption(!1)` —— 未知 flag 会报错而不是忽略。

支持的 package manager（自动检测）：`brew` / `npm` / `pnpm` / `binary`。
- `brew`: 提示 `brew upgrade`，不自动执行
- `binary`: 下 `amp-bin` 最新，替换 `process.execPath`
- `npm/pnpm`: `npm install -g @sourcegraph/amp@<version>`

## `amp install` (hidden)

```
amp install [options]
  --force                               强制重装
  --verbose                             显示进度
```
装 ripgrep 等外部工具到 `$AMP_HOME/bin`。

---

## `amp oauth` (独立，也在 `mcp oauth` 下 reuse)

这个 subtree 定义在 `Px0()` 函数里，通过 `t.addCommand(Px0(...), {hidden:!0})` 挂到 `mcp` 下。详见 `mcp oauth`。

## `amp git-credential-helper [action]` (hidden)

Sandbox 里 `git` 调用的 credential helper。action 默认 `get`。实现 git credential helper 协议。

## `amp sign-commit` (hidden)

Sandbox 里 commit signing。作为 `gpg.program` 给 git commit 签名用。`allowUnknownOption()` —— 接任何 git 传过来的 flag。

## `amp keyboard-tester` (hidden)
```
amp keyboard-tester [--raw]
```
debug 用，把解析过的终端输入 event 流式打印 JSONL。`--raw` 也打印原始字节。

## `amp fork [threadId]` (deprecated, alias: `f`)

打印一行红色警告："The fork command has been deprecated." 然后 exit 1。保留为 hidden 防止旧脚本炸太惨。

---

## 对 Alva 的启发

1. **`permissions test` 的 exit code 多态（0/1/2 各有语义）** 非常优雅 —— CI 脚本可以直接 branch 看行为。alva 的权限系统也可以这么做。
2. **`tools make` scaffold 多语言版本**（bun/zsh/bash）—— 一条命令管 3 种不同 runtime 的 tool 开发。alva 的 extension-builtin 可以做类似的 `alva extensions new <name> --rust|--python`。
3. **`mcp add -- <cmd> [args]`** 用 `--` 分隔符隔离 CLI flag 和 server command —— 对内嵌 shell 命令的 UX 清爽。
4. **`threads handoff` 把 stdin 当 goal** —— `echo "continue XXX" | amp threads handoff T-abc` 这种无缝 pipe 是好设计。
5. **父命令 no-default = `outputHelp() + exit(0)`**，不是报错或打印 "Missing command"。用户体验好。
6. **`fork` 留着作为 deprecated stub** —— 比直接删掉友好，旧脚本至少能看到明确提示。
