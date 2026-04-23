# Amp Command Tree

Amp `0.0.1776760235-g65b009` 的完整命令树。顶层是 `amp`，下面按**子命令**分层；每一行有 `[alias: X, Y]` 和 `(hidden)` / `(deprecated)` 标注。

## 顶层入口

```
amp [options] [prompt]          # 默认：无参数进交互 TUI；有参数/stdin/redirect 进 execute
  -x, --execute [message]       # 执行模式（非交互），打印最后一条 assistant message
  -V, --version                 # 打印版本号并 exit 0
  --help                        # commander 自带
```

**主要入口语义** —— `amp` 单独跑，根据以下信号决定模式：

| 信号 | 行为 |
|---|---|
| `-x` flag | 强制 execute |
| stdout 不是 TTY (管道/重定向) | 自动 execute |
| 提供了 positional prompt | 默认 execute |
| `--stream-json` 或 `--stream-json-input` | 隐含 execute |
| 都没有，stdout 是 TTY | 交互式 TUI |

## 命令树

```
amp
├── login                                    [auth] Log in to Amp
├── logout                                   [auth] Remove stored API key
├── threads                                  [alias: t, thread]      Manage threads
│   ├── list                                 [alias: l, ls]          List all threads
│   ├── new                                  [alias: n]              Create a new thread
│   ├── continue [threadIDOrURL]             [alias: c]              Continue an existing thread
│   ├── search <query>                       [alias: find]           Search threads (DSL)
│   ├── visibility [visibility]              [alias: v]              Show/set repo default visibility
│   ├── rename <id> <newName>                [alias: r]              Rename a thread
│   ├── label <id> <labels...>                                       Add labels
│   ├── share <id>                           [alias: s]              Change visibility / share w/ support
│   ├── archive <id>                                                 Archive (or --unarchive)
│   ├── delete <id>                                                  Permanent delete (client + server)
│   ├── handoff [id]                         [alias: h]              Create handoff thread with goal
│   ├── markdown <id>                        [alias: md]             Render thread as markdown
│   ├── export <id>                                                  Export full thread payload as JSON
│   ├── usage <id>                                                   Show cost/usage for a thread
│   └── fork [threadId]                      [alias: f] (deprecated) Prints deprecation warning only
├── mcp                                      Manage MCP servers
│   ├── add <name> [args...]                                         Add stdio / URL MCP server
│   ├── list                                                         List configured servers (+ --json)
│   ├── remove <name>                                                Remove a server
│   ├── doctor [name]                                                Wait for init + show status
│   ├── approve <name>                                               Approve a workspace-scoped MCP server
│   └── oauth                                [subtree]               OAuth for remote MCP
│       ├── login <server-name>                                      Register OAuth creds
│       ├── logout <server-name>                                     Clear OAuth creds
│       └── status <server-name>                                     Show OAuth status
├── permissions                              [alias: permission]     Manage tool-call permissions
│   ├── list                                 [alias: ls]              List rules (+ --builtin / -w)
│   ├── edit                                                         Open rules in $EDITOR (+ -w)
│   ├── add <action> <tool>                                          Prepend rule (action ∈ allow|reject|ask|delegate)
│   └── test <tool-name>                                             Evaluate without running (+ exit 0/1/2)
├── tools                                    [alias: tool]           Tool management
│   ├── list                                 [alias: ls]              List active tools (+ --inspect / --json / --mode)
│   ├── show <tool>                                                  Show schema for one tool
│   ├── make <tool-name>                                             Scaffold toolbox tool (--bun/--zsh/--bash)
│   └── use <tool-name>                                              Invoke a tool ad-hoc (stdin for JSON input)
├── skill                                    [alias: skills]         Manage skills from GitHub/local
│   ├── add <source>                                                 Install from @user/skill, owner/repo, URL, path
│   ├── list                                 [alias: ls]              List available skills (+ --json)
│   ├── remove <name>                        [alias: rm]              Uninstall
│   └── info <name>                                                  Show skill metadata (+ --json)
├── plugins                                  (hidden) [alias: plugin] Plugin management
│   ├── list                                 [alias: ls]              List .amp/plugins/
│   └── exec <plugin> <event>                                        Execute plugin with JSON event (--data)
├── review [diff_description...]                                     Code review via smart-mode thread
├── review-legacy [diff_description...]      (hidden)                Old review engine
├── usage                                                            Show Amp credit balance / usage
├── update                                   [alias: up]             Update Amp CLI to latest
├── version                                                          Print version (same as -V)
├── install                                  (hidden)                Install ripgrep etc into $AMP_HOME/bin
├── git-credential-helper [action]           (hidden)                Internal: sandbox git auth
├── sign-commit                              (hidden)                Internal: sandbox gpg.program
├── keyboard-tester                          (hidden)                Stream parsed terminal input as JSONL
├── live-sync [threadIDOrURL]                (hidden)                Mirror v2 DTW thread → local checkout
│                                                                    (+--apply for snapshot once, --checkout/--skip-checkout/--worker-url)
└── fork [threadId]                          (deprecated) [alias: f] Shows "deprecated" message and exits
```

## 命令数量统计

| 类别 | 数量 |
|---|---|
| 可见顶级命令 | 11 (login, logout, threads, mcp, permissions, tools, skill, review, usage, update, version) |
| 隐藏顶级命令 | 7 (plugins, review-legacy, install, git-credential-helper, sign-commit, keyboard-tester, live-sync) |
| 已弃用 | 1 (fork) |
| **顶级小计** | **19** |
| threads 子命令 | 14（含 fork 作为 hidden 子命令） |
| mcp 子命令 | 5 + oauth 下 3 |
| permissions 子命令 | 4 |
| tools 子命令 | 4 |
| skill 子命令 | 4 |
| plugins 子命令 | 2 |
| **子命令小计** | **36** |

总共 **~55 个可调用命令路径**（含隐藏）。

## Alias 覆盖表

从反编译里提取的全部 `.alias()` 调用：

| Alias | 指向 |
|---|---|
| `t`, `thread` | threads |
| `l`, `ls` | list（在 threads / tools / skill / plugins / permissions 下） |
| `c` | threads continue |
| `n` | threads new |
| `r` | threads rename |
| `s` | threads share |
| `h` | threads handoff |
| `md` | threads markdown |
| `v` | threads visibility |
| `f` | threads fork (deprecated) |
| `find` | threads search |
| `rm` | skill remove |
| `up` | update |
| `tool` | tools |
| `skills` | skill |
| `plugin` | plugins |
| `permission` | permissions |

注意 Amp **没有**给 `mcp` / `review` / `usage` 这些命令起短名 —— 只给高频 `threads`-下的命令起单字母别名。alva 可以参考这个取舍。

## TUI slash commands（不是 CLI 子命令，但相关）

TUI 内部还有 `/context` / `/compact` / `/handoff` 等 slash commands，这些**不是** CLI 命令 —— 只在交互模式下输入。`context` 因此不出现在 CLI 树里。

## 对 Alva 的启发

1. **默认无参数 = 交互式 TUI** 是个好约定。重定向/管道/显式 flag 才切 non-interactive。alva 可以照抄。
2. **`threads` / `mcp` / `permissions` / `tools` 是四个核心名词**，每个都有自己的子命令空间。alva-app-cli 可以类比：`alva threads` / `alva mcp` / `alva permissions` / `alva tools`（或用 `extensions` 替代 `tools`，因为 alva 里是 Extension-first 架构）。
3. **命令别名策略**：只给**最高频**命令加单字母别名（`t`/`c`/`n` 即 `threads/continue/new`）。冷门命令不加别名减少 help 污染。
4. **hidden commands** 放内部/实验性功能（`live-sync`/`keyboard-tester`）—— 对外 help 看不到但能调用。alva 可以用这招装 debug 命令。
5. **子命令的 `action(() => helpOnly())`**：很多 parent 命令（如 `mcp`）没有 default 行为时直接 outputHelp + exit —— 比什么都不输出强。
