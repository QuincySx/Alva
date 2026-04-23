# Auto-Commit / Trailer 注入

> Amp **不自动 commit**。但只要用户/agent 执行 `git commit`，Amp 在 Bash 层**改写命令**本体，追加 thread 追溯 trailer。

---

## 两个核心 setting

反编译在 settings default (`DVT` 表) 里找到：

```js
"git.commit.ampThread.enabled": {
  value: !0,                         // default true
  visible: !0,
  description: "Enable adding Amp-Thread trailer in git commits"
},
"git.commit.coauthor.enabled": {
  value: !0,                         // default true
  visible: !0,
  description: "Enable adding Amp as co-author in git commits"
}
```

两者独立。`ampThread` trailer 让 commit 可追溯到 thread URL，`coauthor` trailer 让 GitHub contributor 图上看得到 Amp。

## 关闭开关

| 方式 | 作用 | 备注 |
|---|---|---|
| Setting `git.commit.ampThread.enabled = false` | 关 `Amp-Thread-ID` trailer | 全局 / per-workspace |
| Setting `git.commit.coauthor.enabled = false` | 关 `Co-authored-by: Amp` | 同上 |
| Env `AMP_DISABLE_AMP_COAUTHOR_TRAILER=1` | 关 co-author trailer | 逃生口，可临时 override setting |

`j2R` 是 co-author 开关读取函数：

```js
function j2R(T) {
  if (process.env.AMP_DISABLE_AMP_COAUTHOR_TRAILER === "1" 
   || process.env.AMP_DISABLE_AMP_COAUTHOR_TRAILER === "true") return false;
  return T ?? true;                  // setting 默认 true
}
```

## 触发时机：Bash tool 命令改写

**不是** git 层面的 hook，也**不是** commit-msg hook。是 Amp 自家 Bash tool 在 execute 前 inline 改写 argv。

入口在 Bash tool handler `HzT`（见 `../tools/catalog.md` 的 Bash 节）：

```js
if (T.cmd.includes("git")) {
  let _ = t.config.settings["git.commit.ampThread.enabled"] ?? true;
  let m = j2R(t.config.settings["git.commit.coauthor.enabled"]);
  let y = [];
  if (_) y.push(`--trailer "Amp-Thread-ID: https://ampcode.com/threads/${a.id}"`);
  if (m) y.push('--trailer "Co-authored-by: Amp <amp@ampcode.com>"');
  if (y.length > 0) {
    let u = [
      "-c trailer.AmpThread.key=Amp-Thread-ID",
      "-c trailer.AmpThread.ifexists=replace",
      "-c trailer.AmpCoauth.key=Co-authored-by",
      "-c trailer.AmpCoauth.ifexists=addIfDifferent"
    ].join(" ");
    let p = $2R(T.cmd, u, y);
    if (p !== T.cmd) T = {...T, cmd: p};
  }
}
```

注意几点：
- 预检只看 `cmd.includes("git")` 就往下走。真正判断是否是 `git commit` 在 `$2R` 里。
- 把 `-c trailer.*` 也注入到前面，告诉 git 遇到已有相同 key 时怎么办：
  - `Amp-Thread-ID`: **replace** —— 每个 commit 只留最新 thread
  - `Co-authored-by`: **addIfDifferent** —— 支持人 + Amp 共同 co-author

## `$2R` —— AST 级命令改写器

不用正则而用真正的 shell AST parser（`K$` 看起来是 bun / node 版 shell parser），目的是在复合命令里只改 `git commit`、不误伤 `git commit-graph` 之类：

```js
function $2R(T, R, t) {
  if (!T || t.length === 0) return T;
  let r;
  try { r = K$(T); } catch { return T; }        // 解析失败原样返回

  let e = [];
  for (let i of O2R(r)) {                        // 遍历所有 simple commands
    if (!ruT(i.program, "git", T)) continue;     // program === "git"
    let s = i.arguments.find(c => ruT(c, "commit", T));
    if (!s) continue;                            // 第一个 arg 必须是 "commit"
    if (R) e.push({index: i.program.end.offset, text: ` ${R}`});  // -c 插在 git 后面
    e.push({index: s.end.offset, text: ` ${t.join(" ")}`});       // --trailer 插在 commit 后面
  }
  if (e.length === 0) return T;

  // 从后往前插，避免 offset 失效
  let h = e.sort((i, s) => s.index - i.index);
  let a = T;
  for (let i of h) a = a.slice(0, i.index) + i.text + a.slice(i.index);
  return a;
}
```

### 改写前后示例

输入：
```
git commit -m "fix: handle null in parser"
```

输出：
```
git -c trailer.AmpThread.key=Amp-Thread-ID -c trailer.AmpThread.ifexists=replace \
    -c trailer.AmpCoauth.key=Co-authored-by -c trailer.AmpCoauth.ifexists=addIfDifferent \
    commit --trailer "Amp-Thread-ID: https://ampcode.com/threads/T-abc" \
           --trailer "Co-authored-by: Amp <amp@ampcode.com>" \
           -m "fix: handle null in parser"
```

复合命令 `npm test && git commit -m foo && git push` 里**只有 git commit 那段**被改写。`git push`、`npm test` 原封不动。

## 最终 commit message 长这样

```
fix: handle null in parser

Amp-Thread-ID: https://ampcode.com/threads/T-abc-123-def
Co-authored-by: Alice <alice@example.com>
Co-authored-by: Amp <amp@ampcode.com>
```

关键：

- **git trailer block** 在 body 末尾，用 `git interpret-trailers --parse` 可程序化读出
- `ifexists=replace` 让同 commit 重 commit（amend）时 trailer 不重复累积
- `ifexists=addIfDifferent` 保留既有人类 co-author，只在 Amp 尚未出现时才追加

## 不做的事

Amp **没有**：
- 自动触发 `git commit` 的定时器
- 监听 file change 自动 commit 的功能（对比 Aider 的 `--auto-commits`）
- Post-tool-use commit hook
- 在 agent 循环末尾自动落盘

**用户按 approve 才会走 Bash 跑 git commit**。这一设计和 system prompt 里 `Do not commit or push without explicit consent` 一致。

## 对 Alva 的启发

Alva 有 `CheckpointMiddleware`（文件备份），但没把 git 视为审计链。可以考虑：

1. **在 `alva-host-native` 加一个 `GitTrailerMiddleware`**：拦 Bash tool 执行，识别 `git commit` 命令，inline 注入 `Alva-Thread-ID: ...`。
   - 复用 Amp 的 `-c trailer.Alva.ifexists=replace` 策略
   - 用 shell AST parser（可用 rust 的 `conch-parser` 或 `shell-words` 粗糙实现）避免 regex 陷阱
   - 两个 setting `git.commit.alvaThread.enabled` / `git.commit.coauthor.enabled`

2. **把 trailer 和 Checkpoint 双向绑定**：commit 成功后把 `Alva-Thread-ID` 写进 checkpoint metadata，`alva checkpoint list` 能看到每个 snapshot 对应哪些 commit。

3. **`AMP_DISABLE_*` 样式的 env 逃生口**：让 CI 环境能一键关掉，不用改 settings.json。

4. **显式"不自动 commit"策略**：写进 default system prompt，避免和 Aider 风格混淆。
