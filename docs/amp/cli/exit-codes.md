# Amp Exit Codes

## 约定汇总

| Exit Code | 含义 | 出现场景 |
|---|---|---|
| `0` | 成功 | 所有正常完成路径（58 处 `process.exit(0)`） |
| `1` | 业务/CLI 错误 | 所有 `new A0(msg, 1)` 抛出的错误（47 处 `process.exit(1)`） |
| `2` | **仅** `permissions test` 命中 reject | `action === "reject"` 时 |
| `130` | 用户 SIGINT / Ctrl-C | Readline prompt / keypress 监听里手动 `process.exit(130)` |

统计（来自 `/tmp/amp-decompile/strings.txt`）：

```
  58 process.exit(0)
  47 process.exit(1)
   4 process.exit(130)
```

## `A0` 自定义错误类

Amp 用一个 `A0` class（extends Error）承载所有"可预期的 CLI 错误"：

```js
new A0(message, exitCode, suggestion?)
// exitCode 默认 1
// suggestion 会打在错误 message 后面作为 hint
```

统一处理函数：

```js
function ao0(T, R) {             // 主 error handler
  let t = uKT(T, R);             // 提取 exit code（A0 带的，否则 1）
  process.exit(t);
}

async function ZC(T, R) {
  let t = uKT(T, R);
  await uy();                    // 清 terminal
  process.exit(t);
}
```

所有 command action 抛 `A0` → commander `.exitOverride()` → `ao0()` → `process.exit(exitCode)`。

## 具体错误场景

### Exit 1 场景（常见 `new A0(msg, 1)`）

| 场景 | Message 片段 |
|---|---|
| Unknown command | `Did you mean: X? Or run amp --help for all commands.` |
| 参数冲突 | `Choose either --checkout or --skip-checkout, not both.` |
| `--stream-json` 没配 `--execute` | `The --stream-json flag requires --execute mode` |
| `--stream-json-input` 没配 `--stream-json` | `The --stream-json-input flag requires --stream-json` |
| Remote 没配 execute | `The -r/--remote flag requires --execute mode` |
| Invalid model format | `Invalid model format "X". Expected "provider:model"` |
| Invalid thinking level | `Invalid thinking level "X". Expected "low" or "high"` |
| 网络不通 | `Couldn't connect to the Amp server at <URL>.` |
| 未登录 | `You must be logged in to view usage. Run \`amp login\` first.` |
| MCP server name 不合法 | `Invalid server name. Allowed characters: letters, digits, @, /, -, _` |
| MCP add 没有 `--` 后 cmd | `No command provided after -- separator` |
| Live-sync 互斥 | `Choose either a positional thread ID/URL or --apply, not both.` |
| Live-sync 没 V2 feature | `live-sync is not enabled for your user` |
| Non-Sourcegraph 用 --inspect 等 | `You are not allowed to do this.` |
| Experimental 命令没 `--dangerously-allow-all` | `Error: The X command is currently experimental and does not yet support permissions.` |
| Bash 命令不在 allowlist（execute 模式） | `Error: The <tool> tool tried to run a command that isn't allowlisted. Rerun with --dangerously-allow-all to bypass, or add to the command allowlist in permissions` |
| 非 allowlist 工具（execute 模式） | `Error: The <tool> tool is not allowed to run in execute mode. Rerun with --dangerously-allow-all to bypass.` |
| Thread 属于别人 | `This thread belongs to a different user and cannot be continued for security reasons. Set AMP_RESUME_OTHER_USER_THREADS_INSECURE=1 to bypass.` |
| Plugin `--data` 不是 JSON | `Error: --data must be valid JSON` |
| Skill not found | `Skill "<name>" not found.` |

### Exit 2 场景

**只**有一个地方：`permissions test` 子命令里

```js
function u40(T) {
  if (T.action === "ask") return 1;
  if (T.action === "reject") return 2;
  return 0;   // allow / delegate 返回 0
}
```

意义：
- `test` 成功 = 权限允许运行 → 0
- `test` 需要人工 ask → 1
- `test` 会被 reject → 2

所以 CI 脚本可以：

```bash
if amp permissions test Bash --cmd "rm -rf /"; then
  echo "this would run freely (bad!)"
elif [ $? -eq 1 ]; then
  echo "this would ask the user"
else
  echo "this is rejected (good)"
fi
```

### Exit 130 场景（SIGINT）

Readline prompt 里按 Ctrl-C（`\x03`）直接 `process.exit(130)`。另外：

```js
process.on("SIGINT",  () => process.exit());   // 默认 0
process.on("SIGHUP",  () => process.exit());
process.on("SIGTERM", () => process.exit());
```

注意：普通 SIGINT handler 是 exit(0)；只有 readline 自己拿到 Ctrl-C 才是 130。这是个小 inconsistency。

## Commander 特殊 exit

Commander.js 自带几个特殊退出：

| Code | 场景 |
|---|---|
| `commander.help` → exit 0 | `--help` 或 `-h` |
| `commander.version` → exit 0 | `-V` / `--version` |
| `commander.invalidArgument` → exit 1 | 通过 `exitOverride()` 转给 `ao0()` 处理 |
| `commander.unknownCommand` → exit 1 | `T20()` 函数会 suggestion "Did you mean..." |

Amp 用 `R.exitOverride()` 拦截所有 commander 原生 exit，全部走自己的 `ao0()` 流程。

## `amp update` 的特殊错误

Update 命令 `allowUnknownOption(!1)` —— 遇到未知 flag 直接 commander 报错 exit 1。这和大多数命令允许 unknown option 不同（比如 `tools use` 用 `allowUnknownOption(!0)` 透传额外 arg 给 tool）。

## `--version` 的双实现

```js
T.option("-V, --version", "Print the version number and exit", () => {
  SkT(R); process.exit(0);
});
T.command("version").description("Print the version number and exit").action(() => {
  SkT(R); process.exit(0);
});
```

两种都 exit 0。`amp --version` 和 `amp version` 等价。

## 对 Alva 的启发

1. **exit 2 留给 "规则否决"** 这种需要程序判别的结果，alva 的 `extensions test` / `permissions check` 类命令可以抄。0/1/2 三值 enum 比 0/1 二值表达力强。
2. **用自定义 Error 类（A0）** 携带 exit code + suggestion，顶层统一 handler → exit。alva 的 `alva-app-cli` 不要在每个 action 里散着 `process.exit()`。
3. **SIGINT → 130 的 convention** 要遵守，别 exit 0 —— shell 脚本会用 `$?` 判断是被中断还是完成。Amp 这里的 inconsistency（readline 130 vs 信号 handler 0）值得避免，alva 统一 130。
4. **`exitOverride()` + `.action(async)` 抛错**比 `try/catch + process.exit()` 优雅 —— action body 只管业务逻辑，错误路径全交 commander 管。
5. **`permissions test` 的 exit code 语义**要**写进 help 文本里**（Amp 现在没写，用户得翻源码才懂 1 vs 2）。alva 做时记得在 `--help` 里明说 "Exit codes: 0=allow, 1=ask, 2=reject"。
6. **unknown command 给 suggestion**（"Did you mean..."）比干巴巴报错友好。alva 可以抄 `T20()` 的 Levenshtein-like 匹配逻辑。
