# Amp Debug Package — `Debug Instructions` markdown

> Amp 提供一键生成的诊断文档，用户贴给维护者就够用。里面有 Thread URL、Cloudflare 调试链接、DTW 命令、本地日志过滤命令、实时诊断快照。

## 入口函数 `bL0`

从 strings line 63662 提取（去混淆变量名）：

```js
function bL0(input) {
  const { thread, ampURL, logFile, pid, threadViewState } = input;
  const lines = [
    "# Debug Instructions",
    "",
    "## Quick Links",
    `- Thread URL: <${threadURLFor(new URL(ampURL), thread.id)}>`,
  ];

  // 1. 如果 thread 是 DTW-backed，给 Cloudflare 面板直链
  if (isDTW(thread)) {
    lines.push(
      `- Cloudflare Logs: <${WhT(thread.id)}>`,
      `- Cloudflare Data Studio: <${HhT(thread.id)}>`,
    );
    lines.push("", "## DTW Commands", "```sh",
      WJT(thread.id).map(({ label, command }) => `# ${label}\n${command}`).join("\n"),
      "```");
  } else {
    lines.push("", "## DTW Commands",
      "- This thread is not DTW-backed, so DTW commands are unavailable.");
  }

  // 2. 本地 CLI 日志过滤
  if (logFile && pid) {
    const logPath = Aj(logFile);  // 解 file:// URI → 路径
    lines.push(
      "", "## CLI Logs",
      `- Log file: \`${logPath}\``,
      `- PID: \`${pid}\``,
      "",
      "Example: filter logs for this session:",
      "```sh",
      ex("snapshot", pid, logFile),
      "```",
    );
  }

  // 3. 实时诊断（thread / runtime / view state）
  lines.push("", "## Diagnostics", "", qhT(input, "###"));

  return lines.join("\n");
}
```

## 输出示例（从代码重建）

```markdown
# Debug Instructions

## Quick Links
- Thread URL: <https://ampcode.com/threads/T-abc123>
- Cloudflare Logs: <https://dash.cloudflare.com/…/observability/events?…>
- Cloudflare Data Studio: <https://dash.cloudflare.com/…/durable-objects/view/…/studio?…>

## DTW Commands
```sh
# dtw: fetch Cloudflare logs
./scripts/fetch-cloudflare-logs.ts T-abc123
```

## CLI Logs
- Log file: `/Users/xxx/.amp/logs/cli.log`
- PID: `12345`

Example: filter logs for this session:
```sh
tail -n 10000 /Users/xxx/.amp/logs/cli.log | jq -C 'select(.pid == 12345)'
```

## Diagnostics

### Thread
- ID: `T-abc123`
- Title: My thread
- URL: `https://ampcode.com/threads/T-abc123`
- Created: 5 minutes ago (`2026-04-21T10:00:00.000Z`)
- Agent mode: `smart`
- Effective mode: `smart`
- DTW backed: yes
- Executor type: `sandbox`
- Messages: `42` total / `7` human / `0` queued

### Runtime
- Amp URL: `https://ampcode.com`
- Thread pool mode: `dtw`
- Connection state: `connected`
- Connection role: `observer`
- Processing: no
- PID: `12345`
- Client ID: `client-1`
- Log file: `/Users/xxx/.amp/logs/cli.log`

### View State
…
```

## 关键工具函数

### `WhT(threadID)` — Cloudflare Logs URL

```js
function WhT(threadID) {
  const u = new URL(`https://dash.cloudflare.com/${HJT}/workers/services/view/${oL0}/production/observability/events`);
  u.searchParams.set("filterCombination", '"and"');
  u.searchParams.set("calculations", '[{"operator":"count"}]');
  u.searchParams.set("orderBy", '{"value":"count","limit":10,"order":"desc"}');
  u.searchParams.set("timeframe", "24h");
  u.searchParams.set("conditions", "{}");
  // ... 额外带 thread id filter
  return u.toString();
}
```

`HJT` 和 `oL0` 是 build 时替换的 Cloudflare account ID + worker 名字常量（反编译到不了它们的值，推测来自环境变量）。

### `HhT(threadID)` — Data Studio URL

```js
function HhT(threadID) {
  const u = new URL(`https://dash.cloudflare.com/${HJT}/workers/durable-objects/view/${cL0}/studio`);
  u.searchParams.set("name", threadID);
  u.searchParams.set("jurisdiction", "none");
  return u.toString();
}
```

`cL0` 是 Durable Object 类的常量（同样 build 时替换）。这条链接打开的是某个 thread 在 Cloudflare Durable Object 存储里的 live 数据浏览器。

### `WJT(threadID)` — DTW Commands 列表

```js
function WJT(threadID) {
  return [
    { label: "dtw: fetch Cloudflare logs",
      command: `./scripts/fetch-cloudflare-logs.ts ${threadID}` },
  ];
}
```

目前只有一条命令，看名字是 `./scripts/fetch-cloudflare-logs.ts` —— 这意味着**这个脚本期望调试者是 Amp 工程师，手上有 repo**。对外用户点了只会得到 404。

### `ex(mode, pid, logFile)` — CLI 日志过滤命令

```js
function ex(mode, pid, logFile) {
  const pidFilter = `select(.pid == ${pid})`;
  const logPath = Aj(logFile);
  const tailCmd = mode === "snapshot" ? `tail -n 10000 ${logPath}` : `tail -f ${logPath}`;
  return `${tailCmd} | jq -C '${pidFilter}'`;
}
```

PID filter 直接利用 `TT` logger 里自动注入的 `pid` 字段（见 `./logging.md`）。这样即便多个 amp 进程同时跑（同一个 log 文件），也能过滤出特定 session。

### `qhT(input, headingLevel)` — Diagnostics 实时快照

（strings line 63658 附近）

生成 3 个子段：

```markdown
### Thread
- ID / Title / URL / Created / Agent mode / Effective mode / DTW backed / Executor type
- Messages: `N` total / `M` human / `K` queued
- (optional) Last known agent state

### Runtime
- Amp URL / Thread pool mode / Connection state / Connection role
- Processing / PID / Client ID
- (optional) Log file

### View State
- （如果 threadViewState 可用，dump 额外状态）
```

`__(T)` 函数把值渲染成 markdown code block：

```js
function __(T) { return T === undefined || T === null ? "n/a" : `\`${String(T)}\``; }
```

`lQ(t)` 把时间戳渲染成 "5 minutes ago (`ISO`)" 双显示。

## 怎么触发

扫 strings 没找到明显的 `amp debug` 命令注册（可能是在 UI 里通过快捷键或 slash command 触发）。调用 `bL0(...)` 的地方通过 `onDebug` / `onCopyDebug` 回调，最终走到 clipboard。

一个强信号：`_KT` log transport 把 log 当 span event 发 → 用户触发 debug → `bL0` 生成带 log 过滤命令的 markdown → 维护者拿着这份 markdown 可以直接 `tail ... | jq ...` + 打开两个 Cloudflare 链接。

## 对 Alva 的启发

**这是强烈推荐抄的功能。**

Alva 目前是 GUI（Tauri），但 CLI 也有。每当用户报 bug，维护者要的永远是这几样：

1. **Session 标识** —— thread ID / PID / client ID
2. **环境** —— platform, workspace, model, auth mode
3. **日志过滤命令** —— 告诉用户"跑这条命令把日志贴给我"
4. **外部服务链接**（如果有）—— 远端 log / dashboard 直链

### 具体实施建议

在 Alva 里加一个 `alva debug` CLI subcommand 或 GUI "Copy Debug Info" 按钮，生成：

```markdown
# Alva Debug Info

## Quick Links
- Session: `<session-id>`
- Workspace: `/Users/.../project`
- Model: `claude-opus-4-7`

## Versions
- Alva CLI: 0.x.y
- Rust: 1.xx
- OS: darwin 25.3.0

## Local Logs
- Log file: `~/.alva/logs/cli.log`
- PID: `12345`

Filter this session:
```sh
tail -n 10000 ~/.alva/logs/cli.log | jq -C 'select(.pid == 12345)'
```

## Recent Errors
（最近 N 条 error/warn 级别 log，直接内嵌）

## Config
- AEP（Alva extension plugins）加载状态
- MCP servers 状态
- 激活的 Skills
```

**核心价值**：把"对齐调试语言"的时间从来回 10 轮对话压缩到一次粘贴。

另一个关键点：**`_KT` 式 log→span 桥**让 debug package 不需要单独采集。一个 Rust 等价物：

```rust
impl<S> Layer<S> for LogToSpanEventLayer { /* addEvent on current OTEL span */ }
```

注册到 `tracing_subscriber`，后续任何 `tracing::info!` 自动成为 debug package 能导出的 span event。

---

## 交叉引用

- 日志 / `TT.audit` 级别见 `./logging.md`
- OTEL span 树见 `./tracing.md`
- Cloudflare workers / Durable Objects 架构见 `../remote-runtime/dtw.md`

## 原始产物位置

- strings line 63662：`bL0` 入口
- strings line 63658：`WhT / HhT / WJT / ex / Aj / qhT / __ / lQ` 帮助函数
