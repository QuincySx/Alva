# File Change Tracker

> `fileChangeTracker`：Amp 独立于 message history 的**文件改动追踪器**。
> 二进制里叫 `XWT` / `ZWT`（工厂函数）。

---

## 数据结构

```ts
Map<ToolUseID, Map<FileURI, ChangeRecord>>
```

```ts
type ChangeRecord = {
  oldContent: string | null,    // 修改前内容（null = 文件原本不存在）
  newContent: string | null,    // 修改后内容（null = 文件被删除）
  timestamp: number,            // 发生时间
  reverted: boolean,             // 是否已被 undo
}
```

双层 map：第一层按 `ToolUseID` 分组（知道哪次 tool call 改的），第二层按文件 URI 索引（同一次 tool call 可能改多个文件）。

---

## Public API（推断）

```ts
interface FileChangeTracker {
  // 记录一次改动
  record(args: {
    toolUse: ToolUseID,
    uri: FileURI,
    before: string | null,
    after: string | null
  }): Promise<void>;
  
  // 查单文件最近一次未回滚的 edit
  getLastEdit(path: FileURI): Promise<ChangeRecord | null>;
  
  // 列全部改动（用于聚合展示 "本 thread 改了多少文件"）
  getAllRecords(): Promise<ChangeRecord[]>;
  
  // 以 ToolUseID 为粒度反向应用（undo 一次 tool call 的全部 edit）
  revertByToolUse(toolUseID: ToolUseID): Promise<void>;
}
```

---

## 数据持久化

tracker 背后有 `fileChangeTrackerStorage`：

```ts
interface FileChangeTrackerStorage {
  load(threadID): Promise<Map<ToolUseID, Map<FileURI, ChangeRecord>>>;
  save(threadID, data): Promise<void>;
}
```

每个 thread 独立一份 tracker。thread 切换 / 重启 CLI 都能恢复。

---

## 使用场景

### 1. `undo_edit` 工具

```js
fKR = ({ args: T }, { dir: R, fileChangeTracker: t }) => {
  return R8(async (r) => {
    validateDir(R);
    validate(T);
    let path = parseURI(T.path);
    await kA(path).acquire();       // 独占锁
    try {
      let lastEdit = await t.getLastEdit(path);
      if (!lastEdit) {
        return {
          status: "error",
          error: { message: `No edit history found for file '${T.path}'.` }
        };
      }
      let diff = generateDiff(lastEdit.newContent, lastEdit.oldContent, T.path);
      await writeFile(path.fsPath, lastEdit.oldContent);
      await lastEdit.revert();      // 标记 reverted: true
      return {
        status: "done",
        result: diff
      };
    } finally {
      kA(path).release();
    }
  });
}
```

### 2. 聚合统计展示（UI）

```js
function displayFileChanges(fileChangeTracker) {
  let records = fileChangeTracker.getAllRecords();
  let aggregated = aggregateByPath(records);
  
  for (let { path, additions, modifications, deletions, reverted } of aggregated) {
    console.log(`${path}: +${additions} -${deletions} (${reverted ? "reverted" : "active"})`);
  }
}
```

CLI 底部显示 "Changed files: 12" / "Lines: +234 -56" 等信息都是读这个。

### 3. Thread merge / review workflow

Amp 的 merge workflow 最终会让 execution thread 基于 tracker 的数据生成 commit。

### 4. 跨 agent 协作

当本地 CLI 和云端 DTW 同时跑时，双方共享一个 tracker（通过 thread service 同步）。互相能看到对方改了什么。

---

## Prune / Revert 机制

```js
let r = new Map();      // tracker state
let e = (uri) => {
  // 找出某 uri 的最新未回滚记录
  let latest;
  for (let [toolUseID, fileMap] of r.entries()) {
    for (let [recordURI, record] of fileMap.entries()) {
      if (!equalURIs(recordURI, uri)) continue;
      if (!latest || record.timestamp > latest.timestamp) latest = record;
    }
  }
  return latest;
};

let h = async ({ filesFilter, toolUsesToRevert, pruneRevertedToolUses }) => {
  let keep = new Map();
  let toRevert = new Map();
  
  for (let [toolUseID, fileMap] of r.entries()) {
    let isReverting = toolUsesToRevert ? toolUsesToRevert.has(toolUseID) : true;
    for (let [uri, record] of fileMap.entries()) {
      if (filesFilter && !filesFilter(uri)) continue;
      if (record.reverted) continue;
      
      let acc = keep.get(uri) || { changesAfterKeep: [] };
      if (!isReverting) {
        if (!acc.latestKeepChange || record.timestamp > acc.latestKeepChange.timestamp)
          acc.latestKeepChange = record;
      } else {
        acc.changesAfterKeep.push(record);
        toRevert.set(record, toolUseID);
      }
      keep.set(uri, acc);
    }
  }
  
  // ... 对 changesAfterKeep 做回滚
};
```

这个函数是实现 "revert everything after a specific tool_use" 的底层算法。

---

## 和 Git 的关系

tracker **不是 git**：

- git 需要 `git add` + commit 才记录；tracker 每次 edit 立即记录
- git 有 branches / tags / history graph；tracker 就是个 flat map
- git 有自己的 diff 算法；tracker 保存完整 before/after，需要 diff 时现算
- **tracker 是 "undo 的权威记录"**，git 是"commit 的权威记录"

tracker 会用 git 的辅助工具（如 `generateDiff` 走 `jsdiff` 风格的算法），但数据所有权独立。

---

## 与 `discoveredGuidanceFiles` 的关系

在每条用户消息上，Amp 还存一个 snapshot：

```ts
type UserMessage = {
  role: "user",
  content: ContentBlock[],
  discoveredGuidanceFiles?: Array<{uri: URI, lineCount: number}>,  // ← 这里
  // ...
}
```

这是**当时 AGENTS.md 的引用快照**（不是 edit tracker）。

**区别**：
- `fileChangeTracker` 追 **agent 改的文件**
- `discoveredGuidanceFiles` 追 **当时注入 prompt 的 AGENTS.md 文件**

两者合起来让 thread replay 能忠实还原当时行为。

---

## 对 Alva 的启发

你们 `alva-agent-context` 有 4 层 container（AlwaysPresent / OnDemand / RuntimeInject / Memory）+ `ContextHooks` 8 钩子 —— 方向对。但对照 Amp：

### 可以抄的点

1. **独立 crate `alva-agent-edit-tracker`** —— 跟 context 容器分离。用户 undo 时不用动 context 机制。

2. **toolUseID 为第一维**的双层 map —— 让"回滚一整次 tool call 的所有 edit"天然成立。你们现在如果是按 path 单层存储，会丢这个语义。

3. **强制所有写类工具走 tracker** —— `edit_file` / `create_file` / Bash 里的 redirection / `apply_patch` 都必须通过 tracker。不然 undo / diff 展示会不一致。

4. **tracker 是 `Extension`，存 `BusWriter`** —— 按你们现有架构放在 `alva-agent-context::extensions` 或独立 crate，`BaseAgent::builder()` 默认装上，可被替换。

### 可以不做的点

1. **云端同步**（DTW 才需要）—— local-first 项目里，tracker 只要 in-memory + thread 断开前 flush 到 checkpoint。

2. **精细 prune 算法** —— 除非你们做多线程并发（多 agent 同仓库），简化到 "按 ToolUseID 撤销" 即可。
