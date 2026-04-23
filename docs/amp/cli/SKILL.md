---
name: amp-cli
description: Amp 完整 CLI 命令树反编译梳理。15 个顶级命令 + ~35 个子命令 + 20+ 全局/局部 flag + exit code 约定。alva-app-cli 设计参考。
trigger_words:
  - amp cli
  - amp 命令
  - amp 子命令
  - amp subcommand
  - amp flag
  - amp exit code
  - alva-app-cli
  - cli 设计
  - commander.js
---

# Amp CLI

Amp CLI 的完整命令树 —— 从 Bun 编译的二进制 strings 里还原 Commander.js 的 `.command()` / `.option()` / `.alias()` 调用，拼出"如果在 2026-04 装了 amp，`amp --help` 会看到什么"。

为什么重要：`alva-app-cli` 即将开工，需要参考一个成熟 coding agent CLI 的命令拆分粒度、flag 命名、interactive vs execute 模式切换、退出码。这是黄金样本。

## 文件索引

| 文件 | 内容 | 何时读 |
|---|---|---|
| `./command-tree.md` | 顶级命令 + 子命令完整树（15 top + 35 sub），hidden/deprecated 标注 | 想看 "amp 有哪些命令" |
| `./global-flags.md` | 全局选项（`gkT[]` 数组）+ 在哪些命令上可用 | 想抄 flag 命名约定 |
| `./subcommand-details.md` | 每个重要子命令的 args / options / example 明细（threads / mcp / permissions / tools / skill / plugins / review / live-sync）| 实现某个具体子命令时查阅 |
| `./exit-codes.md` | 退出码约定（0/1/2/130）+ `A0` 自定义错误类 + 错误场景分类 | 决定 alva 自己的 exit code 语义 |

## 快速问答（不用加载任何子文件就能答）

**Q: Amp 的 CLI 主入口接什么参数？**
A: `amp [prompt]` 默认进交互式 TUI；`amp -x/--execute [message]` 进 execute（非交互）模式，从 stdin 或参数拿 prompt，打印最后一条 assistant message 后退出。stdout 被重定向时自动进 execute 模式。

**Q: 有多少个顶级命令？**
A: 15 个明面的 + 6 个 `hidden:!0` 的 = 21 个。
- 可见：`login` / `logout` / `threads` / `mcp` / `permissions` / `tools` / `skill` / `usage` / `update` / `version` / `review` / `oauth`（子命令）/ `export`（threads 下别名）
- 隐藏：`fork`（已弃用）/ `git-credential-helper` / `sign-commit` / `keyboard-tester` / `live-sync` / `install` / `plugins` / `review-legacy`

**Q: `--execute` 和 `--dangerously-allow-all` 怎么配合？**
A: `--execute` 进非交互模式 → 如果 prompt 里 agent 想调用需要确认的工具（比如 Bash 执行 `rm`），会直接 fail。加 `--dangerously-allow-all` 绕过全部权限检查。生产 pipeline 的常见组合是 `echo "task" | amp -x --dangerously-allow-all`。

**Q: `--stream-json` 输出什么格式？**
A: Claude Code 兼容的 NDJSON：每行一个 `{type: "assistant"|"user"|"system", message: {...}}` 对象。加 `--stream-json-thinking` 包含 thinking blocks（非 Claude Code 扩展）。`--stream-json-input` 反向：从 stdin 读 JSON Lines 作为用户消息。

**Q: 权限系统怎么组织？**
A: `amp permissions` 下四个子命令：`list` / `edit`（在 $EDITOR 里改 YAML）/ `add <action> <tool>`（allow/reject/ask/delegate）/ `test <tool>`（不真跑，只测规则匹配）。所有都支持 `-w/--workspace` 切换 workspace vs global 作用域。

**Q: MCP 怎么管？**
A: `amp mcp {add|list|remove|doctor|approve|oauth ...}`. `add <name> -- <cmd> [args]` 加 stdio server；`add <name> <url>` 加 remote；`--env KEY=VAL` 和 `--header KEY=VAL` 可重复。`doctor` 等待初始化再报告状态；`approve` 专门给 workspace scope server 做显式确认（安全措施）。

**Q: 退出码怎么约定？**
A: `0` 成功 / `1` 所有业务错误（`new A0(msg, 1)` 这样抛） / `2` 仅 `permissions test` 在 reject 时用 / `130` SIGINT。Unknown command 会建议最接近的命令名后 exit 1。

## 版本信息

从二进制里提取的 build 元数据：
- `version: "0.0.1776760235-g65b009"`
- `buildTimestamp: "2026-04-21T08:34:22.777Z"`
- `buildType: "release"`

Amp 版本号格式：`0.0.<seconds-since-epoch>-g<short-sha>`，所以 `0.0.1776760235` 对应 2026-04-20 左右的 UNIX 时间戳。

## 原始产物

- 反编译 strings 源：`/tmp/amp-decompile/strings.txt` 行 63393–66357 是 CLI 主要定义区
- 关键锚点函数：`r20(T)` = 主命令创建，`Mx0` = MCP 子树，`d40` = permissions 子树，`zD0` = tools 子树，`e70` = skill 子树，`M40` = plugins 子树，`ZfT` = review 工厂，`gkT[]` = 全局 options 数组定义
