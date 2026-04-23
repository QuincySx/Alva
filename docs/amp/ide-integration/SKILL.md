---
name: amp-ide-integration
description: Amp 怎么检测 VS Code / Cursor / Windsurf / Zed / JetBrains / Neovim，怎么按 editor 生成 launch 命令 (code --goto / URL scheme / open)，怎么从 Zed SQLite 和 VS Code state.vscdb 读工作区上下文（query-based IDE），以及 WebSocket 连接 IDE 插件时收什么协议消息（activeEditor / selectionRange / visibleFiles）。想做 IDE 联动 / "amp open file.ts:10" / 读 IDE 工作区状态时加载。
trigger_words:
  - IDE detection
  - editor launch
  - code --goto
  - vscode url scheme
  - zed sqlite
  - state.vscdb
  - workspace.json
  - workspaceStorage
  - activeEditor
  - selectionDidChange
  - visibleFilesDidChange
  - pluginMetadata
  - JetBrains lockfile
  - query-based IDE
  - amp open file
---

# Amp IDE Integration

Amp 怎么**双向**连接到编辑器：CLI 里能感知当前在哪个 IDE、能反向打开文件到 IDE、能读 IDE 工作区状态（不需要装插件）、也能通过插件收实时 selection 事件。

## 文件索引

| 文件 | 内容 | 何时读 |
|---|---|---|
| `./ide-detection.md` | 6 类 IDE 的检测函数和判断依据 (env vars / process list / .app path / URL scheme) | 想做 "detect 当前宿主 IDE" |
| `./editor-launch.md` | `amp open file.ts:10:20` 怎么按 editor 生成命令 (code --goto / vscode:// URL / zed file:10) + `L10:5-L20:3` fragment 语法 | 想做 "从 CLI 反向打开文件到 IDE" |
| `./workspace-state.md` | query-based IDE 从 Zed SQLite + VS Code state.vscdb + workspace.json 读当前打开文件/光标/选区，不需要插件 | 想做 "无插件也能读 IDE 状态" |
| `./user-state-capture.md` | WebSocket-based IDE 的 JSON-RPC 协议：selectionDidChange / visibleFilesDidChange / pluginMetadata / getDiagnostics / openURI | 想做"IDE 插件 → agent 后端"的协议 |

## 架构速查

```
┌──────────────────────────────────────────────────────────────┐
│  Amp CLI                                                      │
│  ┌───────────────────────────┐   ┌─────────────────────────┐  │
│  │  IDE client (ws: path)    │   │ IDE client (query path) │  │
│  │  VS Code / Cursor /       │   │ Zed / JetBrains / VS Co │  │
│  │  Windsurf — plugin 装了   │   │ de 未装插件             │  │
│  │  → WebSocket localhost    │   │ → SQLite + JSON 读本地  │  │
│  │     :port  (lockfile)     │   │    storage 文件         │  │
│  └───────────────────────────┘   └─────────────────────────┘  │
│              │                                │               │
│              └────────────► IDEClient ◄───────┘               │
│                              (统一状态流)                     │
└──────────────────────────────────────────────────────────────┘
         │                                    │
         ▼                                    ▼
  selection/file events                openURI / getDiagnostics
  (IDE → Amp)                          (Amp → IDE)
```

## 2 种连接模式

| connection | 谁 | 通道 | 发现方式 |
|---|---|---|---|
| `ws` | VS Code / Cursor / Windsurf / JetBrains（装了插件时） | WebSocket + JSON-RPC | 插件写 `~/.local/share/amp/ide/*.json` lockfile，内含 port + authToken |
| `query` | Zed（总是）/ VS Code（未装插件时） | 纯文件读，无进程通信 | scan `ps` 找 IDE 进程 → 找对应 userData 目录 → 读 `state.vscdb` / Zed 的 `db.sqlite` |

VS Code 可以走两种路径：**装了 Amp 插件走 ws，没装就降级到 query**。这个设计对 Alva 非常有启发——"插件可选"。

## 6 类 IDE 的检测总览

| IDE | 检测函数（反编译名） | 判断依据 |
|---|---|---|
| **VS Code / Cursor / Windsurf / VSCode Insiders** | `gN(T)` 匹配名字 + `dPT()` 读 `workspaceStorage/` | lowercase 匹配 `"vscode"`/`"vs code"`/`"cursor"`/`"windsurf"`；按 `userDataDirName`（如 `"Code"`/`"Cursor"`）拿到 storage 目录 |
| **Zed** | `MPT(T)` 匹配 `.app` 路径；`DbR()` 看 `TERM_PROGRAM=zed`；`ps` 扫进程 | `/zed.app/` 或 `/Zed Preview.app/` 或 executable = `zed`/`zed-editor` |
| **JetBrains 全家桶** | `n9T(T)` 匹配任一产品名 | lowercase 包含 `intellij`/`webstorm`/`pycharm`/`goland`/`phpstorm`/`rubymine`/`clion`/`rider`/`datagrip`/`appcode`/`android studio`/`fleet`/`rustrover` |
| **Neovim** | `qbR(T)` / `vm0()` | 名字含 `"neovim"` **或** `process.env.NVIM !== undefined` |

合并函数 `NPT(T)` 按优先级返回 IDE 族：Neovim → Zed → JetBrains → VS Code。

## 常见 Q&A（不用深入子文件就能答）

**Q：Amp 为什么分 `ws` 和 `query` 两种 connection？**
A：不是所有 IDE 都有 Amp 插件。Zed 从未有插件，VS Code 可选。`query` 模式在 IDE 不配合时也能读到工作区上下文，只是没实时事件，靠轮询。

**Q：`amp open file.ts:10:20` 是怎么实现的？**
A：三级 fallback：
1. 有 `code` / `cursor` / `windsurf` 等 CLI 在 PATH 里 → 调 `code --goto file.ts:10:20`；
2. fallback 到 URL scheme：`cursor://file/path:10:20` 通过 `open` 命令打开；
3. Zed 专用：`zed file.ts:10:5`（纯参数，不是 flag）。

**Q：没装插件怎么读 VS Code 当前打开哪个文件？**
A：`sqlite3 -readonly` 查 `~/Library/Application Support/Code/User/workspaceStorage/<hash>/state.vscdb` 的 `ItemTable`，key = `memento/workbench.parts.editor` 和 `memento/workbench.editors.files.textFileEditor`，解出 leaf grid → activeGroup → mru[0] → editor URI。Cursor/Windsurf 完全同构（dataDirName 不同）。

**Q：Zed 状态怎么读？**
A：`~/Library/Application Support/Zed/db/0-stable/db.sqlite`，query `editors` + `items` + `panes` + `editor_selections` join 到 active group 的 active editor，得到 bufferPath + byte offset，自己算行列号。

**Q：JetBrains 怎么装插件？**
A：插件写 lockfile 到 `~/.local/share/amp/ide/*.json`（schema `HPT`：`{workspaceFolders, port, ideName, authToken, pid, connection: "ws"|"query", workspaceId?}`），Amp CLI 启动时 `GPT()` scan 这个目录。

**Q：Amp 反向打开的文件是给 LLM 用的吗？**
A：不是。是 agent 给**人**看的——点 diff 里的"jump to file"时调 `openURIInIDE(uri)`，让编辑器光标跳过去。IDE 的 selection 反而是给 LLM 的上下文。

## 对 Alva 最该抄的 1 个点

**"lockfile + 两种 connection 模式"**。Alva 的 Tauri GUI 目前是"纯 app 模式"，但做 IDE bridge 时不要只考虑 plugin 方案。Amp 的 query-based fallback（sqlite3 + workspace.json）让无插件用户也能有 70% 体验，降低用户接入门槛。具体路径：

1. Alva 先实现 `query` 模式：Zed SQLite / VS Code state.vscdb 直接读
2. 再做 VS Code / JetBrains 插件（写 lockfile → port → WS 连回 Tauri）
3. Tauri 端用统一 `IdeClient` 抽象，`connection: "ws" | "query"` 枚举切换

详见 `editor-launch.md` 末尾的"对 Alva 的启发"。
