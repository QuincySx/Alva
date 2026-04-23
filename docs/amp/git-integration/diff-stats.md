# Diff Stats & Thread-level Snapshots

> Amp 把整个 working tree 的 git 状态算成一个稳定的 "snapshot" artifact，配 diffStat (added/deleted/changed) 数字。Agg Man (orchestrator) 用它判断 executor thread 的进度，`live-sync` 子命令用它做反向同步。

---

## 顶层 API：`uVT(cwd)`

一次调用拿到 **完整 thread snapshot**：

```js
async function uVT(T, R = {}) {
  let { maxDiffBufferBytes: t = rg } = R;
  let r = Date.now();
  let e = await Qo(T, ["rev-parse", "--show-toplevel"]);
  if (!e) return cxT(r, "not a git repository");

  let h = e.trim();                                // repo root
  let a = $_0(h);                                  // repo name (dirname)
  let [i, s, c] = await Promise.all([
    Qo(h, ["rev-parse", "--verify", "HEAD"]),      // HEAD sha
    Qo(h, ["symbolic-ref", "--short", "HEAD"]),    // current branch
    Qo(h, ["status", "--porcelain=v1", "--untracked-files=all", "-z"])
  ]);
  if (c === null) return cxT(r, "failed to read git status");

  let A = i?.trim() || null;                       // head
  let C = s?.trim() || null;                       // branch
  let o = pVT(c);                                  // parse status
  let n = await ty0(h, A);                         // ahead/behind vs origin/HEAD
  let b = n?.aheadCount && n.aheadCount > 0 
        ? await Ry0(h, n.comparisonRef)            // list ahead commits
        : [];
  let _ = await P_0(o, z_0, async (m) => { /* per-file diff + content */ });

  return {
    provider: "git", capturedAt: r, available: true,
    repositoryRoot: h, repositoryName: a, branch: C, head: A,
    diffHash: X_0(_),                              // stable sha256 of diff
    baseRef: n?.baseRef ?? null,
    baseRefHead: n?.baseRefHead ?? null,
    aheadCount: n?.aheadCount ?? 0,
    behindCount: n?.behindCount,
    aheadCommits: b,
    files: _
  };
}
```

- 所有 git 调用走 `Qo` = `git.command`-aware spawner
- diff 有 `maxDiffBufferBytes` (const `rg`) 上限，防超大 repo 爆内存
- 并发获取 HEAD / branch / status，串行做 per-file diff（有并发度 `z_0`）

## Status 解析：`pVT` + `K_0`

输入是 `git status --porcelain=v1 -z --untracked-files=all`，NUL 分隔的双字节状态码：

```js
function K_0(T) {                                  // 2-char status → category
  if (T === "??") return "untracked";
  let R = T[0] ?? " ", t = T[1] ?? " ";
  if (R === "U" || t === "U" || T === "AA" || T === "DD") return "unmerged";
  if (R === "R" || t === "R") return "renamed";
  if (R === "C" || t === "C") return "copied";
  if (R === "A" || t === "A") return "added";
  if (R === "D" || t === "D") return "deleted";
  if (R === "T" || t === "T") return "type_changed";
  return "modified";
}

function pVT(T) {
  let R = [], t = T.split("\x00");
  for (let r = 0; r < t.length; r++) {
    let e = t[r];
    if (!e || e.length < 4) continue;
    let h = e.slice(0, 2);                         // XY status
    let a = e.slice(3);                            // path (skip space)
    if (!a) continue;
    let i = K_0(h);
    if (i === "renamed" || i === "copied") {
      let s = t[r + 1];                            // next NUL token is old path
      if (s) { R.push({path: a, previousPath: s, changeType: i}); r += 1; continue; }
    }
    R.push({path: a, changeType: i});
  }
  return R.sort((r, e) => r.path.localeCompare(e.path));    // stable order
}
```

输出 `changeType` 的 8 种：`untracked` / `unmerged` / `renamed` / `copied` / `added` / `deleted` / `type_changed` / `modified`。

## DiffStat 算法：`V_0`

核心是**逐 hunk 计算 `min(added, deleted)` 当作 changed**，剩余算 pure add / delete：

```js
function V_0(T) {
  let R = 0,                                       // added
      t = 0,                                       // deleted
      r = 0,                                       // changed
      e = 0,                                       // hunk-local +
      h = 0;                                       // hunk-local -
  let a = () => {                                  // flush hunk
    if (e === 0 && h === 0) return;
    r += Math.min(e, h);                           // overlap = changed
    e = 0; h = 0;
  };

  for (let i of T.split("\n")) {
    if (i.startsWith("+") && !i.startsWith("+++")) {
      R += 1; e += 1; continue;
    }
    if (i.startsWith("-") && !i.startsWith("---")) {
      t += 1; h += 1; continue;
    }
    a();                                           // context line or hunk boundary
  }
  a();

  return { added: R, deleted: t, changed: r };
}
```

**这比"每条 + 算一个 insertion"更贴近人类直觉**：

```
- foo
+ bar
```

`{added: 1, deleted: 1, changed: 1}` —— 用户看这是**改了一行**，不是"加了 1 行又删了 1 行"。

Hunk boundary 识别靠"遇到非 `+`/`-` 行就 flush"。对 `---` / `+++` （文件头）特判跳过。

## Diff 获取：`oxT` / `Y_0`

```js
let H4 = ["-c", "core.quotepath=false", "diff", "--no-color", "--no-ext-diff"];
```

所有 `git diff` 都用这组前缀，确保 UTF-8 路径不被转义、输出确定性。

- **tracked file**: `git diff HEAD -- <path>` 或 `git diff --cached -- <path>` + `git diff -- <path>` 合并（staged + unstaged）
- **untracked file**: `git diff --no-index -- /dev/null <path>`（Windows 用 `NUL`）
- **unified 可配置**: `--unified=<n>` 从 caller 传，默认不传 = git 默认 3 行

Amp 对每个 file 存 **三份 diff 和两份内容**：

```js
{
  path, previousPath, changeType, created,
  diff,                 // --unified=3 (default)
  fullFileDiff,         // --unified=W_0 (probably MAX_SAFE_INT), only for "modified"
  oldContent,           // git show HEAD:<path>
  newContent,           // read from working tree
  diffStat: V_0(diff)
}
```

`fullFileDiff` 便于 UI 展示整个文件上下文，`diff` 省 token。

## 稳定 hash：`X_0`

```js
function X_0(T) {
  let R = l_0("sha256");                           // crypto.createHash
  for (let t of T) {
    R.update(t.path);
    R.update("\x00");
    R.update(t.diff);
    R.update("\x00");
  }
  return R.digest("hex");
}
```

`diffHash` 作为 snapshot 的**内容寻址 key**：
- 两次 status 调用 diff 相同 → 相同 hash → artifact 去重
- Agg Man 看 hash 变没变决定要不要重新 pull 新 artifact

## Ahead / Behind 计算：`ty0`

```js
async function ty0(T, R) {
  if (!R) return null;
  let t = await J_0(T);                            // symbolic-ref origin/HEAD
  if (!t) return null;
  let r = await Qo(T, ["rev-list", "--count", `${t.comparisonRef}..HEAD`]);
  let e = await Qo(T, ["rev-list", "--count", `HEAD..${t.comparisonRef}`]);
  // ... parse + return { aheadCount, behindCount, baseRef, comparisonRef }
}
```

先找 `origin/HEAD`（用 `git symbolic-ref --quiet refs/remotes/origin/HEAD`），再数 rev-list。比 `git status -sb` 解析更可靠，因为后者在 detached HEAD / 无 upstream 时输出格式不定。

## Ahead Commits 列表：`Ry0`

```js
async function Ry0(T, R) {
  let t = await Qo(T, ["log", "-z", `--max-count=${q_0}`, "--format=%H%x00%s", `${R}..HEAD`]);
  if (!t) return [];
  return Ty0(t);                                   // parse NUL-separated hash/subject pairs
}
```

限制 `q_0` 条（未反编译出具体数字，推测 50-100）。

## Artifact 上传

```js
function SVT(T) {
  let R = N_0(ry0(T), M_0);                        // serialize + compress (probably gzip)
  return {
    key: mtT,                                       // const key "git.status"
    dataType: "application/json",
    contentBase64: R
  };
}
```

`mtT`（推测 = `"git_status"`）作为 artifact 唯一 key。Agg Man 收到 `artifact_upserted` 事件，key 是这个，就知道要刷新 diff 视图。

## 对 Alva 的启发

Alva 的 `CheckpointExtension` 做的是"tool execute 前备份文件"，没有 repo 级 snapshot。可以补：

1. **`GitSnapshotExtension`**：
   - 接上 `uVT` 同款数据结构（changeType 枚举 8 种 / diffStat / aheadCount）
   - 作为 `AgentContext` 的一部分暴露给 subagent
   - Subagent 可以问 "changed since my parent started work?" 而不用自己跑 git
   - 存为 AEP 可加载的 guidance artifact

2. **抄 `V_0` 的 `min(added, deleted) = changed` 算法**：别用 `--shortstat` 的天真 `insertions / deletions` —— 它在大块替换时数字会让用户困惑。

3. **`H4 = ["-c", "core.quotepath=false", ...]` 前缀**：任何 Alva 内部调 git 的地方都应当用这组，确保：
   - 路径非 ASCII 不被 octal-escape
   - 输出无颜色码
   - 不跑用户配置的 `diff.external`（安全性）

4. **stable diff hash**：checkpoint 相邻两次如果 hash 相同，直接跳过落盘 —— 能省大量 IO。

5. **`--porcelain=v1 -z` + `--untracked-files=all`**：标准组合，比 `git status -s` 稳。别自己拼。

6. **和 `SubAgentExtension` 联动**：subagent 收到的 context 里带 snapshot，subagent 完成后 host 再取一次 snapshot 对比，得到"这个 subagent 改了什么"，可以直接汇报给 parent。
