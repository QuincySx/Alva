# IDE Detection

Amp 怎么判断当前是哪个 IDE 在跑。6 个独立的 detection 函数 + 一个合并器 `NPT()`。

## 1. 字符串匹配：4 个独立 matcher

Amp 把 IDE 名字当成字符串检测，不区分大小写。

### VS Code 家族（`gN`）

```js
function gN(T) {
  let R = T.toLowerCase();
  return R.includes("vscode")
      || R.includes("vs code")
      || R.includes("cursor")
      || R.includes("windsurf");
}
```

**注意一个分支合并**：Cursor 和 Windsurf 都是 VS Code fork，Amp 把它们视作 "VS Code 家族"。后面 `NPT()` 返回时会标 `"VS Code"`（用于 prompt 里告诉 LLM），但识别时把四个混在一起。

### JetBrains 全家桶（`n9T`）

```js
function n9T(T) {
  let R = T.toLowerCase();
  return R.includes("intellij")
      || R.includes("webstorm")
      || R.includes("pycharm")
      || R.includes("goland")
      || R.includes("phpstorm")
      || R.includes("rubymine")
      || R.includes("clion")
      || R.includes("rider")
      || R.includes("datagrip")
      || R.includes("appcode")
      || R.includes("android studio")
      || R.includes("fleet")
      || R.includes("rustrover");
}
```

13 个产品硬编码进去。漏了 `Aqua` / `MPS`（JetBrains 不常见的）。

### Neovim（`qbR`）

```js
function qbR(T) { return T.toLowerCase().includes("neovim"); }
```

### Zed（`zbR`）

```js
function zbR(T) { return T.toLowerCase().includes("zed"); }
```

## 2. 合并器：`NPT(T) -> "Neovim" | "Zed" | "JetBrains" | "VS Code"`

```js
function NPT(T) {
  if (qbR(T)) return "Neovim";
  else if (zbR(T)) return "Zed";
  else if (n9T(T)) return "JetBrains";
  else if (gN(T)) return "VS Code";
}
```

**顺序**：Neovim > Zed > JetBrains > VS Code。首次匹配即返回。"VS Code 家族" 都统一标成字符串 `"VS Code"`，给 LLM 提示时不区分 Cursor/Windsurf。

## 3. 运行时环境检测：CLI 启动那一刻就能判定

Amp 启动时（在 `main` 入口）按这个顺序判定 "我跑在哪个 IDE 的 terminal 里"：

```js
// 简化自真实代码
if (R.jetbrains) {
  WI("JetBrains");                     // CLI flag 显式指定
} else if (R.ide && lm0()) {           // lm0: TERM_PROGRAM === "vscode"
  WI("VS Code");
} else if (R.ide && vm0()) {           // vm0: process.env.NVIM !== undefined
  WI("Neovim");
} else if (R.ide) {
  let E = await WF0();                 // 扫描 query-based IDE 进程
  if (E) {
    let M = NPT(E.ideName);
    if (M) WI(M);
  }
}
```

`WI(name)` 是一个 setter，把当前 IDE 族名存到全局 `qI`。这个值后续被塞进 system prompt，让 LLM 知道 "我正被 VS Code 调用"。

### 环境变量检测函数

```js
function lm0() {
  return process.env.TERM_PROGRAM !== undefined
      && process.env.TERM_PROGRAM === "vscode";
}

function vm0() {
  return process.env.NVIM !== undefined;
}

function DbR() {
  return process.env.TERM_PROGRAM?.toLowerCase() === "zed"
      || process.env.ZED_TERM === "true";
}
```

**关键环境变量**：

| 变量 | 谁设 | 判定 |
|---|---|---|
| `TERM_PROGRAM` | VS Code 集成 terminal / Zed terminal | `vscode` / `zed` |
| `TERM_PROGRAM_VERSION` | 同上 | 用来判断 Zed stable/preview/nightly |
| `NVIM` | Neovim `:terminal` 里启动的 subprocess 自带 | 存在即是 |
| `ZED_TERM` | Zed terminal 额外标记 | `"true"` |
| `ZED_CHANNEL` | 用户手动覆盖 Zed channel | `stable`/`preview`/`nightly`/`dev` |
| `VSCODE_USER_DATA_DIR` | 用户手动覆盖 VS Code userData 路径 | 路径字符串 |
| `CURSOR_USER_DATA_DIR` / `WINDSURF_USER_DATA_DIR` | 同上，VS Code fork 各有一个 | 路径字符串 |

## 4. 进程扫描：WF0 → Dv registry

当 env var 不足以判定时（比如用户在 iTerm 里启动 amp，但 VS Code 在后台开着），Amp 调 `WF0()` 扫 query-based IDE。

```js
async function WF0() {
  for (let T of Dv)                    // Dv = [Zed query, VS Code query, Cursor query, Windsurf query, ...]
    try {
      if ((await T.listConfigs()).length > 0) return T;
    } catch (R) {
      TT.debug("Failed to detect query-based IDE integration", { ideName: T.ideName, error: R });
    }
  return;
}
```

`Dv` 数组里是所有支持 query 模式的 IDE 配置（通过 `EE(cfg)` 工厂构造）。

### `Dv` 注册表的 IDE 配置对象

4 条记录（从反编译里提取到的原始字面量）：

```js
YiT = {                                   // VS Code
  ideName: "VS Code",
  userDataEnv: "VSCODE_USER_DATA_DIR",
  userDataDirName: "Code",
  urlScheme: "vscode",
  appPathMarkers: ["visual studio code.app"],
  executableNames: ["code", "code.exe"],
  commandCandidates: {
    unix: ["code"],
    windows: ["code.cmd", "code.exe", "code"]
  }
};

QiT = {                                   // VS Code Insiders
  ideName: "VS Code Insiders",
  userDataEnv: "VSCODE_INSIDERS_USER_DATA_DIR",
  userDataDirName: "Code - Insiders",
  urlScheme: "vscode-insiders",
  appPathMarkers: ["visual studio code - insiders.app"],
  executableNames: ["code-insiders", "code-insiders.exe"],
  commandCandidates: {
    unix: ["code-insiders"],
    windows: ["code-insiders.cmd", "code-insiders.exe", "code-insiders"]
  }
};

ZiT = {                                   // Cursor
  ideName: "Cursor",
  userDataEnv: "CURSOR_USER_DATA_DIR",
  userDataDirName: "Cursor",
  urlScheme: "cursor",
  appPathMarkers: ["cursor.app"],
  executableNames: ["cursor", "cursor.exe"],
  commandCandidates: {
    unix: ["cursor"],
    windows: ["cursor.cmd", "cursor.exe", "cursor"]
  }
};

JiT = {                                   // Windsurf
  ideName: "Windsurf",
  userDataEnv: "WINDSURF_USER_DATA_DIR",
  userDataDirName: "Windsurf",
  urlScheme: "windsurf",
  appPathMarkers: ["windsurf.app"],
  executableNames: ["windsurf", "windsurf.exe"],
  commandCandidates: {
    unix: ["windsurf"],
    windows: ["windsurf.cmd", "windsurf.exe", "windsurf"]
  }
};
```

**六个关键字段**：
1. `ideName` — 面向 LLM 的展示名
2. `userDataEnv` — 用户手动覆盖 userData 路径时用的 env var
3. `userDataDirName` — 默认 `~/Library/Application Support/<here>/User/workspaceStorage` 里的目录名
4. `urlScheme` — 反向 open 用的 URL scheme (`vscode://`, `cursor://`, ...)
5. `appPathMarkers` — macOS 的 `.app` 路径片段，用来从 `ps` 输出里识别 GUI 进程
6. `executableNames` + `commandCandidates` — CLI 名，用来 PATH 查找 `code --goto` 这种

Zed 的配置不在 `Dv` 里（它的 detection 完全靠独立的 Zed-special-case 代码），JetBrains 也不在（JetBrains 只走 WebSocket + lockfile）。

## 5. Zed 的特殊检测：多产品线

Zed 有四条发布渠道（stable / preview / nightly / dev），每条都是独立的 `.app`。

```js
function MPT(T) {                        // 判断某个命令是否为 Zed
  let R = T.toLowerCase();
  return /\/zed( \w+)?\.app\//.test(R)
      || R.endsWith(`${Rn.sep}zed`)
      || R.endsWith(`${Rn.sep}zed.exe`)
      || R === "zed"
      || R === "zed-editor";
}

function a9T(T) {                        // 识别 channel
  let R = T.match(/\/Zed (Preview|Nightly|Dev)\.app\//i);
  if (!R) return /\/Zed\.app\//.test(T) ? "stable" : null;
  let t = R[1].toLowerCase();
  return ["stable", "preview", "nightly", "dev"].includes(t) ? t : null;
}
```

Zed 的 userData 目录按 channel 分路径：`~/Library/Application Support/Zed/db/<n>-<channel>/db.sqlite`。

### Zed channel 解析

```js
function LbR() {
  let T = process.env.ZED_CHANNEL?.trim().toLowerCase() ?? "";
  if (e9T(T)) return T;               // 用户显式设了 ZED_CHANNEL
  if (!DbR()) return null;            // 不在 Zed terminal 里
  let R = process.env.TERM_PROGRAM_VERSION?.trim().toLowerCase();
  if (!R) return null;
  for (let t of ["stable","preview","nightly","dev"])
    if (t !== "stable" && R.includes(t)) return t;
  return "stable";
}
```

Zed 把 channel 编到 `TERM_PROGRAM_VERSION` 字符串里，Amp 用子串匹配。

## 6. JetBrains 的检测只有字符串

JetBrains 不做进程扫描（`Dv` 里没有），也不读什么 sqlite。**它完全依赖插件写的 lockfile**：

```
~/.local/share/amp/ide/*.json       (Unix)
%APPDATA%\amp\ide\*.json            (Windows, 通过 XDG_DATA_HOME 推导)
```

文件内容（Zod schema `HPT`）：

```js
HPT = X.object({
  workspaceFolders: X.array(X.string()),
  port: X.number(),
  ideName: X.string(),               // 这里填 "IntelliJ IDEA" 等
  authToken: X.string(),
  pid: X.number(),
  connection: X.enum(["ws", "query"]).optional(),
  workspaceId: X.number().optional()
});
```

Amp CLI 启动时 `GPT()` 扫目录：

```js
function GPT() {
  if (!Bv.existsSync(OL)) return [];  // OL = path.join(nN, "ide"), nN = ~/.local/share/amp
  let T = [];
  for (let R of Bv.readdirSync(OL, { withFileTypes: true }))
    if (R.isFile() && R.name.endsWith(".json")) {
      let t = qPT.join(OL, R.name);
      T.push(t);
    }
  return T;
}
```

然后 `KPT(file)` 读 + Zod parse + `s$(pid)` 检查进程存活（过滤僵尸 lockfile）。

## 7. 排序策略：多 IDE 同开时选谁

`wv()` 是聚合函数，把所有 IDE configs (lockfile + query 扫到的) 合并，排序规则 `VbR()`：

1. **workspaceFolder 精确等于 cwd** 的排最前
2. **工作区 folder 是 cwd 的前缀或反之**（子目录关系）次之，短的优先
3. 其他的按 `ideName` 字典序

这样在 monorepo 下打开多个 IDE 时（e.g. VS Code 在 `/repo`、Cursor 在 `/repo/app`），用户 `cd /repo/app && amp` 会选 Cursor。

## 8. 陷阱和 debug 技巧

- **Cursor 的 process name** 在 macOS 是 `Cursor Helper (Renderer)` 等多个子进程，Amp 的 `appPathMarkers: ["cursor.app"]` 靠路径片段避开这个问题。
- **WSL** 里 VS Code 会变成 `code-server`，不在 `executableNames` 里——Amp 在 WSL 体验会降级到纯 env var 判断。
- **JetBrains plugin 未装**时 `WF0()` 找不到它，Amp 完全不知道它存在。
- 环境变量 `AMP_IDE` / `AMP_JETBRAINS` 没有——不像 Claude Code 那样有显式 override（Claude 有但更 ad-hoc）。Amp 完全靠 CLI flag `--ide` / `--jetbrains` 和运行时检测。

## 对 Alva 的启发

1. **把 IDE 配置数据结构抽出来**（类似 `Dv` 注册表），6 字段 = `name / env_override / data_dir_name / url_scheme / app_markers / exe_names`。加新 IDE 只需 append 一条。
2. **分层检测**：env var > process scan > lockfile。每层失败不中断，降级到下一层。
3. **Zed 的 channel 检测** 是硬编码，不优雅但简单。Alva 如果要支持 JetBrains 全家桶，也建议硬编码一个字符串列表，不要过度设计成"产品发现机制"。
4. **注意：VS Code 家族全部合并成一个 detection**（Cursor/Windsurf 都走 VS Code schema），但**展示时分开**（lockfile 里 `ideName` 不同）。Alva Tauri GUI 如果要显示"当前 IDE"也应走这个路径：检测归一，显示保留。
