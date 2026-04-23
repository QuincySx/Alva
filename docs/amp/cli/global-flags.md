# Amp Global Flags

Amp 的**全局 flag** 分两层：
1. `gkT[]` 数组 —— 定义了 19 个跨命令可用的全局选项（自动注册到每个命令）
2. 主命令 `r20(T)` 独有的 flag —— 只在 `amp [prompt]` 主入口上，不会传到子命令
3. 子命令局部 flag —— 在 `subcommand-details.md` 里

本文件覆盖 (1) 和 (2)。

## 主命令独有 flag (`r20`)

只出现在 `amp [prompt]` / `amp -x ...` 这个主入口上，**不会**出现在 `amp threads list` 的 help 里。

| Flag | 类型 | 默认 | 说明 |
|---|---|---|---|
| `-x, --execute [message]` | optional-option | `false` | Execute 模式。可带 message（否则从 stdin）。只打印最后一条 assistant message。stdout 被重定向时自动启用。 |
| `-r, --remote` | switch (hidden) | `false` | 配合 `-x` 使用，在 Amp 服务器 async agent 上跑。 |
| `--stream-json` | switch | `false` | 配合 `-x`，Claude Code 兼容 NDJSON 输出（`{type:"assistant", message:{...}}`）。 |
| `--stream-json-thinking` | switch | `false` | 同上，但包含 `type:"thinking"` blocks。隐含 `--stream-json`。 |
| `--stream-json-input` | switch | `false` | 从 stdin 读 JSON Lines 作为用户消息。要求同时有 `-x` 和 `--stream-json`。 |
| `--stats` | switch (hidden) | `false` | 配合 `-x`，输出 JSON 含 result + token usage（用于 /evals）。 |
| `--archive` | switch | `false` | 配合 `-x`，命令完成后归档 thread。 |
| `-l, --label <label>` | option (repeatable) | `[]` | 配合 `-x`，给 thread 加 label，可多次用。 |
| `-V, --version` | switch | — | 打印版本后 exit 0。 |

## `gkT[]` 全局选项数组（跨命令）

在 `r20()` 里用 `for (let b of gkT)` 自动注册到主命令，子命令通过 `optsWithGlobals()` 读取这些值。

### Flag 类型映射表（来自 `gkT[]` 项的 `type` 字段）

| 类型 | 注册方式 | 含义 |
|---|---|---|
| `flag` | `--foo` 和 `--no-foo` 都注册 | 可开可关，有默认值 |
| `switch` | 只注册 `--foo` (=true) | 单向开关 |
| `optional-option` | `--foo [value]` | 可带可不带值 |
| `option`（默认） | `--foo <value>` | 必须带值 |

### 完整 `gkT[]` 清单（19 项）

| Flag | 类型 | 默认 | Hidden | 说明 |
|---|---|---|---|---|
| `--notifications` / `--no-notifications` | flag | TTY=on, SSH=bell, execute=off | | 声音/终端 bell 通知 |
| `--color` / `--no-color` | flag | TTY 时 on | | 彩色输出 |
| `--settings-file <path>` | option | `$AMP_SETTINGS_FILE` 或 `~/.config/amp/settings.json` | | 覆盖设置文件路径 |
| `--log-level <level>` | option | — | | `trace`/`debug`/`info`/`warn`/`error` |
| `--log-file <path>` | option | — | | 覆盖默认 log 路径 |
| `--format <ui\|jsonl\|new-ui>` | option | — | ✓ (deprecated) | 输出格式，已弃用 |
| `--dangerously-allow-all` | switch | `false` | | **禁用所有 tool 调用确认**，agent 随便跑 |
| `--jetbrains` / `--no-jetbrains` | flag | auto-detect | | JetBrains 集成（自动包含 open file + selection） |
| `--ide` / `--no-ide` | flag | `true` | | IDE 连接（auto 包含 open file + selection） |
| `--interactive` / `--no-interactive` | flag | — | ✓ (deprecated) | 强制交互 UI |
| `--mcp-config <json-or-path>` | option | — | | 额外 MCP 配置（merge 到 settings） |
| `-m, --mode <mode>` | option | `smart` | | Agent mode（`smart`/`large`/`deep`/`internal`） |
| `--take-me-back` | switch | `false` | ✓ | 禁用 v2 thread 模式，用 legacy worker runtime |
| `--neo` | switch | `false` | ✓ | 用 Neo TUI |
| `--headless [threadId]` | optional-option | — | ✓ | headless DTW harness，可带 thread ID 接上 executor |
| `--api-key <key>` | option | — | ✓ | DTW 内部命令用（覆盖 `AMP_API_KEY`） |
| `--sp <text-or-path>` | option | — | ✓ | 自定义 system prompt（text 或 file path） |
| `--system-prompt <text>` | option | — | ✓ | 自定义 system prompt（纯 text） |
| `--model <spec>` | option | — | ✓ | 覆盖 model。格式 `provider:model` 或 `mode=provider:model,mode=provider:model` |

### 非 `gkT` 但也能在主命令上出现的 flag

从 `r20()` 直接 `.option()` 出来的：

| Flag | 说明 |
|---|---|
| `--visibility <visibility>` | 设置 thread visibility：`private`/`public`/`workspace`/`group`（`share` 子命令还多一个 `unlisted`） |

## 环境变量（从 help 末尾列出）

```
AMP_API_KEY         Access token for Amp (see https://ampcode.com/settings)
AMP_URL             URL for the Amp service (default is https://ampcode.com/)
AMP_LOG_LEVEL       Set log level (can also use --log-level)
AMP_LOG_FILE        Set log file location (can also use --log-file)
AMP_SETTINGS_FILE   Set settings file path (can also use --settings-file)
```

其他从 code 里提到的（不在 help 里但真的会读）：

| 变量 | 作用 |
|---|---|
| `AMP_TEST_UPDATE_STATUS` | 测试用，伪造 update status |
| `AMP_SKIP_UPDATE_CHECK=1` | 禁用启动时 update 检查 |
| `AMP_WORKER_URL` | 覆盖 DTW worker URL |
| `AMP_RESUME_OTHER_USER_THREADS_INSECURE=1` | 允许 continue 别人的 thread（不安全） |
| `PLUGINS` | `off`（默认）/`all` —— 控制是否加载 `.amp/plugins/` |
| `AMP_HOME` | 安装 ripgrep 等工具的目录 |
| `XDG_CONFIG_HOME` | 配置文件根（默认 `~/.config`） |

## Flag 精选（alva 最值得抄的）

### 1. `--dangerously-allow-all`
名字本身就是警告。用在 CI / pipeline 里的标准模式：

```bash
echo "commit all unstaged" | amp --dangerously-allow-all -x
```

### 2. `--stream-json` 的双向 IO
- 输出 `--stream-json`：Claude Code 兼容 NDJSON，消费方容易 jq
- 输入 `--stream-json-input`：从 stdin 喂 JSON Lines 做连续对话
这套设计让 amp 可以被另一个 orchestrator 当 subprocess 跑。alva 的 remote-runtime 已经做了类似的事，CLI 层面可以再补。

### 3. `--mode <mode>` 是一级公民
不是 plugin、不是 flag 附属，直接独立一个 `-m/--mode`。Amp 有 `smart`/`large`/`deep`/`internal` 四个 mode。alva 如果设计了 agent modes，CLI 上也应该给一级 flag。

### 4. `--visibility` 在主命令和 share 上都出现
——但值集合不同（share 多一个 `unlisted`）。**别在一个 CLI 里让同名 flag 在不同子命令下接收不同 value 集合**。alva 这里应该统一。

### 5. flag 去重 + help 过滤
从 `Im0()` 源码看，Amp 的 help formatter 会：
- 过滤掉 `hidden` 选项
- 避免 global + local 同名选项重复显示
- **主命令的 help** 过滤掉 `--execute` 和 `--interactive`（因为是"行为开关"而不是配置）

alva 可以学这个 `Global options:` 章节的拆分 —— 别把所有 flag 堆在一个 `Options:` 章节里。

## 对 Alva 的启发

1. **`gkT[]` 式集中定义**比每个 `.option()` 分散好。一个数组/结构体 + 自动注册 + 类型枚举（flag/switch/option）→ 便于 help 生成、序列化到 settings、做 CLI → settings 覆盖链。alva 的 `alva-app-cli` 应该这么写。
2. **flag 命名使用 kebab-case**，无例外。短 alias 用 `-x` / `-m` / `-r` 保留给最高频。
3. **hidden flag 占 11/29 = 38%**。大量"逃生舱"选项（`--sp`/`--model`/`--take-me-back`）留给高级用户，不污染 help。alva 也应保留这个能力 —— 尤其是 model override 和 system prompt override。
4. **不要** 让 parent command 的 `.option()` 默默 leak 到 subcommand。Amp 显式 `optsWithGlobals()` 把 global 和 local 分开 —— 文档清晰。
5. `--no-<flag>` 自动配对（flag 类型）比单独搞 `--disable-X` 漂亮。
