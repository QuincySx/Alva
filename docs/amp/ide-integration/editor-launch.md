# Editor Launch

`amp open file.ts:10:20` 怎么按当前 IDE 生成不同 launch 命令。这条路径**反向**使用了 IDE 注册表 (`Dv`)。

## 入口：`YAR(config, uri)` — "openURI in IDE"

这是给 query-based IDE 用的反向 open 函数（ws-based 走 WebSocket 发 `openURI` RPC 给插件，不在本文范围）。

```js
function YAR(T, R) {                       // T = IDE config (YiT/ZiT/JiT/QiT), R = uri string
  let { filePath: t, line: r, column: e } = ZAR(R);
  if (!t) return !1;

  // 组装 "path:line:col" 字符串
  let h = r ? `${t}:${r}${e ? `:${e}` : ""}` : t,
      a = mbR(T);                          // find CLI executable in PATH

  if (!a) return FiT(T, t, r, e);          // ← fallback to URL scheme

  // 首选: code --goto path:10:5
  if (q7(a, ["--goto", h], { stdio: "ignore" }).status === 0) return !0;

  return FiT(T, t, r, e);                  // 再 fallback
}
```

## 3 级 fallback 链路

### Level 1：CLI executable + `--goto`

`mbR(T)` 遍历 `commandCandidates.unix` (或 `.windows`)，试 `<cmd> --version` 是否 exit 0：

```js
function mbR(T) {
  let R = T.commandCandidates.unix;          // ["code"] for VS Code
  for (let t of R)
    if (q7(t, ["--version"], { encoding: "utf-8" }).status === 0)
      return t;
  return null;
}
```

然后调：

```bash
code --goto /abs/path/file.ts:10:20
cursor --goto /abs/path/file.ts:10
windsurf --goto /abs/path/file.ts
code-insiders --goto /abs/path/file.ts:10:5
```

所有 VS Code fork 共用 `--goto file:line:col` 语法（因为都从 VS Code 继承）。

### Level 2：URL scheme + `open`

如果 CLI 不在 PATH（比如用户没点过 "Install 'code' command in PATH"），fallback 到系统 `open`：

```js
function FiT(T, R, t, r) {
  let e = R.replaceAll("\\", "/"),           // Windows path normalization
      h = (e.startsWith("/") ? e : `/${e}`)
          .split("/")
          .map(c => encodeURIComponent(c))
          .join("/"),
      a = t ? `:${t}${r ? `:${r}` : ""}` : "",
      i = `${T.urlScheme}://file${h}${a}`,    // 组装 vscode://file/abs/path/file.ts:10:5
      s = QAR(i);                            // QAR = spawn("open", [uri])
  return s.status === 0;
}

function QAR(T) {
  return q7("open", [T], { stdio: "ignore" });
}
```

生成的 URI 示例：

```
vscode://file/Users/me/proj/src/a.ts:10:5
vscode-insiders://file/Users/me/proj/src/a.ts:10:5
cursor://file/Users/me/proj/src/a.ts:10:5
windsurf://file/Users/me/proj/src/a.ts:10
```

然后 macOS/Windows 的系统打开处理（`open` / `start`）把 URL 交给注册的协议 handler（VS Code 桌面端全都注册过这些 scheme）。

### Level 3：Zed 特殊路径

Zed 不在 `Dv` 注册表里，有**独立的** `kbR()`：

```js
function kbR(T) {
  let { filePath: R, line: t, column: r } = lbR(T);     // parse file:// URI
  if (!R) return !1;
  let e = t ? `${R}:${t}${r ? `:${r}` : ""}` : R,
      h = BbR();                              // find zed / zed-editor binary
  if (!h) return !1;
  return $PT(h, [e], { stdio: "ignore" }).status === 0;  // zed /abs/path:10:5
}

function BbR() {
  let T = ["zed", "zed-editor"];
  for (let R of T)
    if ($PT(R, ["--version"], { encoding: "utf-8" }).status === 0)
      return R;
  return null;
}
```

**注意**：Zed 用**位置参数**，**不是 flag**：

```bash
zed /abs/path/file.ts:10:5       # ✓ 不是 zed --goto ...
```

Zed 没有 URL scheme fallback（不像 VS Code 有 `vscode://`）。如果 `zed` 不在 PATH，直接失败。

## 输入 URI 的 line/col 解析：`ZAR(R)`

Amp 对外接收的 uri 格式是 `file:///abs/path.ts` 带 fragment，fragment 编码 line/col：

```js
function ZAR(T) {
  try {
    let R = NR.parse(T);
    if (R.scheme !== "file") return {};
    let t = JAR(R.fragment);                   // parse fragment "L10:5-L20:3"
    return { filePath: R.fsPath, line: t?.line, column: t?.column };
  } catch { return {}; }
}

function JAR(T) {
  let R = T.match(fPT),                        // regex
      t = R?.groups?.line;
  if (!t) return null;
  return {
    line: Number.parseInt(t, 10),
    column: R.groups?.column ? Number.parseInt(R.groups.column, 10) : void 0
  };
}
```

### 关键 regex：`fPT`

```js
fPT = /^L(?<line>\d+)(?:(?<columnSeparator>:|C)(?<column>\d+))?(?:-L(?<endLine>\d+)(?:(?<endColumnSeparator>:|C)(?<endColumn>\d+))?)?$/
```

**支持的 fragment 语法**：

| Fragment | 含义 |
|---|---|
| `#L10` | 行 10 |
| `#L10:5` | 行 10 列 5 (GitHub-style) |
| `#L10C5` | 行 10 列 5 (alt) |
| `#L10:5-L20:3` | range，行 10 列 5 到行 20 列 3 |
| `#L10C5-L20C3` | 同上（C 分隔符） |

实际上 `openURIInIDE` 只用 start（line/column），range 的 end 被丢掉。但 agent 输出 diff 时会带 range 便于 UI 高亮。

## Zed 的变体：`lbR` 和 `vbR`

Zed 单独实现了一遍（不共用 `ZAR`/`JAR`），regex 是 `BPT`：

```js
BPT = /^L(?<line>\d+)(?:(?<columnSeparator>:|C)(?<column>\d+))?(?:-L(?<endLine>\d+)(?:(?<endColumnSeparator>:|C)(?<endColumn>\d+))?)?$/
```

**完全一样的 regex**，只是 minifier 重新命名了。Amp 的代码组织是"按 IDE 独立模块"，所以有重复代码。

## 流程图

```
amp openURI "file:///a.ts#L10:5"
          │
          ▼
     ZAR(uri)  ←── parses fragment "L10:5" via fPT regex
          │
          ▼
   {filePath, line, column}
          │
          ▼
 ┌─ config.ideName === "Zed" ?
 │       │ yes                            │ no
 │       ▼                                ▼
 │   BbR() find "zed"/"zed-editor"    mbR(T) find "code"/"cursor"/...
 │       │                                │
 │       ▼                                ▼
 │   zed /a.ts:10:5                   code --goto /a.ts:10:5
 │                                        │ fail?
 │                                        ▼
 │                                    FiT(T, path, line, col)
 │                                        │
 │                                        ▼
 │                                    open vscode://file/a.ts:10:5
 └────────────────────────────────────────┘
```

## 陷阱

### Windows 路径编码

`FiT` 有特殊处理：

```js
let e = R.replaceAll("\\", "/"),           // 反斜杠 → 斜杠
    h = (e.startsWith("/") ? e : `/${e}`)  // 保证 leading slash
        .split("/")
        .map(c => encodeURIComponent(c))   // 每个 segment 单独编码
        .join("/");
```

不用 `encodeURI()` 整串编码，因为 `/` 会被保留。空格、中文、`#` 都会被转义。

### Cursor/Windsurf 是否真的支持 `--goto`？

从反编译的 `commandCandidates` 看，Amp **假设** 它们支持 `--goto`（因为继承了 VS Code binary）。实测 Cursor 1.x / Windsurf 是支持的。万一哪天某 fork 改了，Amp 会直接失败——没有 "try alternate flag" 机制。

### `open` 命令的平台差异

`QAR(T) = q7("open", [T])`——只在 macOS / Linux（有 `xdg-open` 别名）能用。Windows 没有 `open`（有 `start`，但 Amp 没写 Windows 分支），所以 Windows 下 URL scheme fallback 可能是坏的。

## 对 Alva 的启发

1. **统一的 `openFileInEditor(uri)` API** — Alva 的 Tauri bridge 应该按这个函数签名设计：接收 `file://path#L10:5` 形式的 URI，内部根据当前 IDE 族 dispatch 到不同命令。
2. **3 级 fallback**：CLI flag → URL scheme → 报错。不要只做一种，因为用户的 `code` 命令装 / 没装都存在。
3. **抄 `#L10:5-L20:3` fragment 语法**——这是 GitHub 的 permalink 格式，用户已经有直觉了。Alva 存 diff 时可以用这个表达 range 而不用单独的 `line_start / line_end` 字段。
4. **把 `commandCandidates` 做成配置**，可以运行时扩展。用户想让 Alva 支持 Fleet？添加一条 `{ideName: "Fleet", urlScheme: "fleet", commandCandidates: {unix: ["fleet"]}}` 就行，不用改代码。
5. **Windows 补全**——Amp 缺 `start` 分支。Alva 用 Rust 写 `std::process::Command::new("cmd").args(["/C", "start", uri])` 就能解决。
