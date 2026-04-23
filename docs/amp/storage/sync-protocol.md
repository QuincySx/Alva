# 同步协议 —— Thread Service + Version Vector

---

## Thread Service

`threadService` 对象是所有 thread CRUD 的入口。方法列表（从反编译提取）：

```ts
interface ThreadService {
  // 读
  get(threadID): Promise<Thread | null>;
  observe(threadID): Observable<Thread>;        // 订阅实时变化
  getPrimitiveProperty(threadID, key): Promise<any>;
  
  // 改（全部走 server）
  setTitle(threadID, title): Promise<void>;
  flushVersion(threadID, version): Promise<void>;   // ← 关键
  archive(threadID, archived: boolean): Promise<void>;
  delete(threadID): Promise<void>;
  
  // 搜索
  listThreads(options): Promise<{ threads: ThreadInfo[]; hasMore: boolean }>;
  
  // 工具
  registerTool(tool): void;
  tools: Observable<Tool[]>;
}
```

---

## Version Vector（`thread.v`）

每个 thread 有一个 **monotonic version number**：

```ts
type Thread = {
  id: "T-xxx",
  v: 42,     // ← 这个
  // ...
}
```

每次 thread 状态改动（加 message、改 title、归档...）server 端 bump `v`。

### 增量同步

```ts
// 本地已知 version: 40
// 调 observe(threadID)
// Server 只推 v > 40 的变化
```

这让**多客户端同步**和**断线恢复**简单：

```
Client A (version 40) 改 title         →  v = 41
Client B (version 40) 连上             ←  收到 v = 41 的 patch
Client A 添 message                     →  v = 42
Client B 添另一条 message              ?→  冲突解决（推断：server 端 last-writer-wins 或 CRDT）
```

### `flushVersion` 的语义

```js
let thread = await ae.getOrCreateForThread(connection, threadID);
await thread.handle({ type: "title", value: newTitle });
await threadService.flushVersion(threadID, thread.v);   // 确保 local v 推到 server
```

**用途**：
- 确保这次操作在 server 端持久化
- 其他 client 能看到这个 version
- 用作"操作完成"的 barrier

---

## Remote Executor Thread

```js
let result = await l3.createRemoteExecutorThread({
  prompt: userInput,
  repositoryURL: "https://github.com/owner/repo"
}, { config });

// result.ok ? result.result.url : result.error
```

创建远程 execution thread（DTW）。返回新 thread 的 URL。

---

## Server API 端点

从代码里反推出的 API 端点（相对 `AMP_URL`）：

```
POST /api/createRemoteExecutorThread
POST /api/getThreadLinkInfo
GET  /api/threads?...           (listThreads)
POST /api/threads/{id}/rename
POST /api/threads/{id}/archive
DELETE /api/threads/{id}
...
```

所有请求带 `Authorization: Bearer <API_KEY>`。

### 特殊端点

```
/api/provider/anthropic    ← Amp proxy 到 Anthropic（不走用户自己的 key）
/api/internal              ← 内部 API（sign commit 等）
```

---

## Thread Ownership 检查

```js
if ((await l3.getThreadLinkInfo({ thread }, { config: R })).ok !== true) {
  throw `This thread belongs to a different user and cannot be continued for 
  security reasons. Set AMP_RESUME_OTHER_USER_THREADS_INSECURE=1 to bypass 
  this check.`;
}
```

CLI `amp --thread T-xxx` 启动时会先 check 线程归属。不是你的 thread 默认拒绝，除非设 env var 强制覆盖。

---

## 搜索 DSL

`threads search` 命令和 `search_threads` 工具支持一套 DSL：

```
Keywords:    "race condition" 或 auth
File filter: file:src/auth/login.ts
Repo filter: repo:github.com/owner/repo
Ref filter:  ref:main
Author:      author:me  或 author:alice
Date:        after:2024-01-01  before:2024-02-01

组合（implicit AND）:
  auth file:src/foo.ts repo:amp ref:main

所有 matching 都是 case-insensitive。
File paths 做 partial matching。
```

---

## Visibility 模式

```ts
type Visibility =
  | "private"                          // 只有作者
  | "public_discoverable"              // 全网可搜到
  | "public_unlisted"                  // 知道 URL 能看
  | "thread_workspace_shared"          // 所在 workspace 成员可看
  | { sharedGroupIDs: string[] };      // 指定分组可看
```

Enterprise workspace 支持 default visibility 配置。Private 可以被 workspace admin 禁用。

---

## CLI 命令

```bash
amp threads list                   # 列表
amp threads list --json            # JSON 输出
amp threads rename T-xxx "title"
amp threads archive T-xxx
amp threads archive --unarchive T-xxx
amp threads delete T-xxx
amp threads export T-xxx           # Markdown
amp threads export --json T-xxx    # JSON
amp threads share T-xxx            # Share with Amp support
amp threads handoff T-xxx          # 见 context/handoff.md
amp threads handoff --goal "..."   # 对最近的 thread
amp threads visibility T-xxx public
amp threads label T-xxx +bug -wip  # 标签
amp threads visibility-default     # 工作区默认 visibility（enterprise only）
amp threads search <query>         # 用上面的 DSL
```

---

## 本地缓存（观察到的最小缓存）

```
~/.amp/
├── (无 thread 数据)
└── ...其他
```

**没有本地 thread DB**。所有 `threadService` 调用走网络。

这有个实际后果：**Amp 必须能联网才能用**。即使跑 local executor 也要连 ampcode.com 存 thread。

（有 feature flag 可能允许 fully local，但默认不是。）

---

## 对 Alva 的启发

Amp 是**云优先**架构，Alva 是**local-first** —— 两条路。但仍有可以借鉴的点：

### 1. Version vector > event log

你们的 session 持久化可以用 `session.v` monotonic 版本号。比 append-only event log 更简单，对 LLM 对话场景够用。

### 2. Observe-based UI

`threadService.observe(threadID)` 返回 Observable，UI 订阅。你们 Tauri 前端 + Rust 后端可以用 Tauri 的 event bus 做类似的事。

### 3. Ownership / permission 模型

Thread 可以跨用户分享，但有明确的 owner + visibility enum。如果你们做多用户协作，抄这套。

### 4. 搜索 DSL

`file:` / `repo:` / `ref:` / `author:` / `after:` / `before:` 这套 DSL 对开发者友好。你们如果做 thread 列表搜索，抄这套比全文搜索直观。

### 5. 不要做"完全本地"

Amp 强制走 server 是有商业原因的。但即使你们是 local-first，**checkpoint 远端备份**也是值得考虑的功能（断电 / 换电脑场景）。
