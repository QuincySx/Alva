# Workspace State

从 Zed SQLite / VS Code state.vscdb / workspace.json **不装插件**读 IDE 工作区上下文。这是 Amp 的 `connection: "query"` 模式核心。

## 为什么要 query 模式

WebSocket-based IDE bridge 要求用户装 Amp 插件。对多数用户（尤其第一次跑 amp）这是摩擦。Amp 的解法：**直接读 IDE 的本地 storage**。

两种数据源：
- **Zed**：`~/Library/Application Support/Zed/db/<n>-<channel>/db.sqlite` — 标准 SQLite，Zed 自己用的 "workspaces" 表
- **VS Code 家族**：`~/Library/Application Support/{Code,Cursor,Windsurf,...}/User/workspaceStorage/<hash>/state.vscdb` + `workspace.json`

读取前必须 verify `sqlite3` CLI 可用：

```js
async function ybR() {
  return (await R9T("sqlite3", ["-version"])).status === 0;
}
async function LPT() {                  // Zed-specific alias
  return (await i9T("sqlite3", ["-version"])).status === 0;
}
```

**不用 better-sqlite3 等 npm binding**，靠 spawn CLI。好处是 zero-config（macOS / 多数 Linux 自带 sqlite3），坏处是多 fork 开销。

## 1. VS Code 家族：state.vscdb 读取

### 路径发现

```js
function gPT(T) {                       // T = IDE config object (YiT/ZiT/...)
  let R = process.env[T.userDataEnv];   // 先看 VSCODE_USER_DATA_DIR 等
  if (R) return R;
  return oi.join(os.homedir(), "Library", "Application Support",
                 T.userDataDirName, "User");
}
```

在 macOS 是 `~/Library/Application Support/Code/User`，userData 目录。然后拼 `/workspaceStorage`。

### Scan workspaceStorage

```js
async function dPT(T, R) {              // R = Set of cwd prefixes to filter by
  if (R?.size === 0) return [];
  let t = oi.join(gPT(T), "workspaceStorage");
  if (!await zI(t)) return [];
  let r = await md.promises.readdir(t, { withFileTypes: true });
  return (await Promise.all(r.filter(e => e.isDirectory()).map(async e => {
    let h = e.name,                      // 32-char hash
        a = oi.join(t, h),
        i = oi.join(a, "state.vscdb"),
        s = oi.join(a, "workspace.json");
    if (!await zI(i) || !await zI(s)) return null;
    let c = await obR(s);                // parse workspace.json -> folder path
    if (!c || !await zI(c)) return null;
    if (R && !R.has(z7(c))) return null;
    let A = await md.promises.stat(i);
    return { storageID: h, workspaceFolder: c, stateDBPath: i, mtime: A.mtimeMs };
  }))).filter(e => e !== null)
      .sort((e, h) => h.mtime - e.mtime)
      .slice(0, pbR);
}
```

**每个 workspaceStorage 子目录**：
- 目录名 = hash (32 hex)
- `workspace.json` — 记录这个 storage 对应哪个实际工作区（folder 路径）
- `state.vscdb` — SQLite 数据库，存编辑器状态

按 mtime 降序 (`pbR` 大概是 `20`) 限制扫描数量。

### 解析 workspace.json

```js
async function obR(T) {
  try {
    let R = await md.promises.readFile(T, "utf-8"),
        t = JSON.parse(R);
    if (t.folder) return NR.parse(t.folder).fsPath;         // single-folder workspace
    if (t.configuration) return oi.dirname(NR.parse(t.configuration).fsPath);  // multi-root
    return null;
  } catch { return null; }
}
```

两种 workspace 类型：
- `{ folder: "file:///path" }` — 单文件夹
- `{ configuration: "file:///some.code-workspace" }` — 多根，取 workspace 文件所在目录

### Scan active windows：`xPT`

```js
async function xPT(T) {
  let R = oi.join(gPT(T), "globalStorage", "storage.json");
  if (!await zI(R)) return null;
  let t = await md.promises.readFile(R, "utf-8"),
      r = JSON.parse(t).windowsState,
      e = Array.isArray(r?.openedWindows) ? r.openedWindows : [],
      h = e.length > 0 ? e : [r?.lastActiveWindow],
      a = new Set;
  for (let i of h) {
    if (!i) continue;
    let s = cbR(i);                       // extract folder from window state
    if (!s) continue;
    a.add(z7(s));                         // z7 = normalized path (lowercase, forward slashes)
  }
  return a;
}
```

这个函数找出"**当前打开的窗口**"对应哪些 folder。返回 `Set<path>`，用来 filter `dPT()`——只要当前开着的 workspace，不要历史的。

### 读 editor state：`TbR(dbPath)`

```js
async function TbR(T) {
  let R = await nbR(T,
    "select key, value from ItemTable " +
    "where key in ('memento/workbench.parts.editor', " +
                   "'memento/workbench.editors.files.textFileEditor')"
  );
  if (R.length === 0) return null;

  let t = RbR(R.find(s => s.key === "memento/workbench.parts.editor")?.value);
  if (!t) return null;

  let r = t.leaves.flatMap(s => s.editors),     // all open files across all groups
      e = t.leaves.find(s => s.id === t.activeGroup) ?? t.leaves[0],
      h = e ? rbR(e) : void 0,                   // active editor in active group
      a = ebR(R.find(s => s.key === "memento/workbench.editors.files.textFileEditor")?.value),
      i = h ? await hbR(h, t.activeGroup, a) : void 0;   // cursor/selection

  return { openFile: h, openFiles: r, selection: i };
}
```

**关键 SQL**：

```sql
select key, value from ItemTable
where key in ('memento/workbench.parts.editor',
              'memento/workbench.editors.files.textFileEditor');
```

VS Code 把 editor layout 序列化成 JSON 存到 `ItemTable` 的这两行里：
- `memento/workbench.parts.editor` — 整个编辑器网格的结构（group / tab 布局）
- `memento/workbench.editors.files.textFileEditor` — 每个 file editor 的 cursor / scroll / viewState

### 解析 editor layout：`RbR`

```js
function RbR(T) {
  if (!T) return null;
  let R = JSON.parse(T),
      t = R["editorpart.state"] ?? R.editorpart?.state,
      r = t?.activeGroup;
  if (typeof r !== "number") return null;
  let e = t?.serializedGrid,
      h = SPT(e?.root ?? e);               // recursive grid flatten
  return { activeGroup: r, leaves: h };
}
```

### Grid 递归：`SPT`

VS Code editor grid 是一棵 branch/leaf 树，`SPT` 递归 flatten 出所有 leaf。每个 leaf = 一个 editor group (tab bar)，携带 `{ id, editors[], preview, mru[] }`。

### 拿到 active editor：`rbR`

`T.mru[0] ?? T.preview ?? 0` —— Amp 用 **mru[0]**（most-recently-used）判断当前 tab，不是 preview（preview 是 ctrl+click 那种临时预览）。

### URI 解析：`tbR`

editor.value 是 JSON 字符串，内含 `resourceJSON` 对象：
- `resourceJSON.external` → 非 file URI（git/diff/untitled），直接透传
- `resourceJSON.scheme === "file" && .path` → local file，构造 `file://` URI

### 读光标/选区：`hbR`

```js
async function hbR(T, R, t) {
  let r = t.get(T)
       ?? t.get(decodeURIComponent(T))
       ?? t.get(encodeURI(T));              // URI 格式 3 种都试
  if (!r) return;
  let e = (r[String(R)] ?? Object.values(r)[0])?.cursorState?.[0];
  if (!e) return;
  let h = await abR(T);                     // read file content for selection text
  if (h === void 0) return;
  let a = GiT(e.selectionStart.lineNumber, e.selectionStart.column),   // 1-based → 0-based
      i = GiT(e.position.lineNumber, e.position.column),
      s = KiT(a, i) <= 0 ? a : i,           // min
      c = KiT(a, i) <= 0 ? i : a,           // max
      A = s.line === c.line && s.character === c.character
          ? ibR(h, s.line)                  // empty selection → return whole line
          : sbR(h, s.line, s.character, c.line, c.character);
  return {
    range: { startLine: s.line, startCharacter: s.character,
             endLine: c.line, endCharacter: c.character },
    content: A
  };
}
```

**关键行为**：
- line/column 从 1-based (VS Code) 转 0-based (amp 内部)
- empty selection（cursor，无 selection）返回**光标所在整行文本** — 这样 agent 总能拿到"上下文"
- 有 selection 则返回选中文本

### SQLite URI query

```js
function CbR(T) {
  let R = T.replaceAll("\\", "/").split("/").map(encodeURIComponent).join("/");
  return `file:${R.startsWith("/") ? R : `/${R}`}?immutable=1`;
}

async function nbR(T, R) {
  let t = await R9T("sqlite3",
    ["-readonly", "-json", CbR(T), R],
    { timeout: ubR });
  // returns JSON array
}
```

三个关键 flag：
- `-readonly` — 安全
- `-json` — 输出直接 JSON.parse
- URI 加 `?immutable=1` — SQLite 不尝试加锁/等待 WAL checkpoint（VS Code 正在运行时 db 被锁）

## 2. Zed：db.sqlite 读取

Zed 数据结构比 VS Code 更规范（是个真正的关系数据库）。

### 路径

```
~/Library/Application Support/Zed/db/<number>-<channel>/db.sqlite
```

`<channel>` = `stable` / `preview` / `nightly` / `dev`（见 `ide-detection.md`）。

### SQL queries

**List workspaces**（最近 200 个）：

```sql
select workspace_id, paths, timestamp
from workspaces
order by timestamp desc
limit 200
```

`paths` 字段可能是 JSON array `["/a", "/b"]` 或 `\t`-分隔的字符串（旧版）。Amp `EPT()` 两种都支持。

**Active editor + selection**：

```sql
select e.item_id, e.buffer_path, s.start, s.end
from editors e
join items i on e.item_id = i.item_id
join panes p on i.pane_id = p.pane_id
left join editor_selections s on s.editor_id = e.item_id
where e.workspace_id = ?
  and i.active = 1
order by p.active desc
limit 1
```

拿到：`itemId`、`bufferPath`（文件绝对路径）、`startOffset`、`endOffset`（**字节 offset**，不是 line/col）。

**All open files in this workspace**：

```sql
select distinct e.buffer_path
from editors e
join items i on e.item_id = i.item_id
where e.workspace_id = ?
  and e.buffer_path != ''
```

### Byte offset → line/col：`tsT`

Zed 存的是字节偏移，Amp 自己算行列：

```js
function tsT(T, R) {
  let t = 0, r = 0, e = Math.min(R, T.length);
  for (let h = 0; h < e; h += 1)
    if (T[h] === 10) t += 1, r = h + 1;    // newline = \n
  return { line: t, character: R - r };
}
```

注意 `character` 是 **byte offset 减去 line-start byte offset**——对 UTF-8 多字节字符**不准**。VS Code 的 LSP 协议是 UTF-16 code units，这里是 UTF-8 bytes，二者都不是 grapheme。Amp 没处理这个 edge case。

### Content extraction：`ObR(bufferPath, startOffset, endOffset)`

Amp 读**磁盘**上的文件（不是 Zed 内存 buffer），用 `RsT` clamp 到文件大小，`tsT` 算 line/col，`slice` 出内容。Cursor（start===end）返回**整行**，selection 返回切片。

**注意陷阱**：用户刚编辑未保存时，Amp 读到旧磁盘内容 + 对应新版的 offset → 乱码/越界。query 模式的固有限制。

## 3. 最终聚合：`gbR()` / `wv()`

`gbR()` (Zed listConfigs) 流程：`LPT()` 检查 sqlite3 → `PPT()` + `MPT()` ps scan Zed 进程 → `vL()` 找最近的 db → `jPT()` 读 workspaces → `jbR()` 包装成 IdeConfig。

`wv(opts)` 是总入口（见 `ide-detection.md` §7 排序），返回 `IdeConfig[]`。被 agent 启动流程调用来初始化 IDE 上下文。

## 对 Alva 的启发

1. **最值得抄**：**"query 模式" 作为 "ws 模式" 的 graceful fallback**。Alva 当前的 IDE bridge 应该**先**做 query 模式（sqlite3 + workspace.json），因为：
   - 零配置，用户 `cargo run --bin alva-app-tauri` 就能感知 VS Code 状态
   - 不影响 IDE 启动、不装插件
   - 做不到实时事件（只能轮询）但已经覆盖 70% 场景
2. **SQLite 查询独立线程池** — Alva Rust 项目用 `rusqlite::Connection::open_with_flags` 加 `SQLITE_OPEN_READONLY` + URI `?immutable=1`，避免和 IDE 抢锁。
3. **把 "editor state schema" 硬编码，别试图从 VS Code 读 ABI**。VS Code 的 `memento/workbench.parts.editor` 是内部 format，可能随版本变。Amp 选择"硬读 + try/catch"——失败就返回空，不 crash。
4. **字节 offset 的坑**：如果要支持 Zed，Alva 要复刻 `tsT()` 的字节→行列转换，并注明 UTF-8 多字节不准。或者干脆直接返回 `byte_offset` 给 LLM，让 LLM 自己决定怎么用。
5. **workspace 聚合排序策略** 抄过来：cwd 精确匹配最前，前缀关系其次，其他字典序——确保 monorepo 下选对 IDE。
