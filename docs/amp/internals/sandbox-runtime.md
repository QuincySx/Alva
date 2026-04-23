# Sandbox Runtime —— `executorType:"sandbox"` 与 `.agents/preview`

> Amp 的 executor 有两种身份：`local-client`（你的 CLI）和 `sandbox`（跑在云端的 headless executor）。sandbox 模式下 system prompt 会注入三段额外内容，其中包括一份 "`.agents/preview` 文件" 约定。

---

## 强证据 vs 推测

**强证据**：
- `executorType` 字面量只有两种：`"local-client"` 和 `"sandbox"`（grep 唯一命中）。
- `bootstrapExecutor({ executorType: "sandbox", ... })` 被 DTW headless harness (`kVT` 类) 显式调用。
- Sandbox 模式触发**三条专属 system prompt 注入**（shallow-git 提示 + `.amp/in/artifacts` 规则 + `.agents/preview` 指引）。
- `.agents/preview` 路径常量 `a6R=".agents/preview"` 存在。

**推测**：
- "跑在 Firecracker / Docker / microVM" —— **没直接证据**。DTW 上下文提到 Cloudflare Workers + Durable Objects + Rivet actor，Workers 本身提供 isolate，**没看到 VM 字样**。
- ".agents/preview" 文件具体格式 —— binary 只检查文件存在、读内容告诉 LLM "follow it"；**格式完全自由**，没 schema。
- Sandbox 是否允许网络 / 能否写磁盘 —— 没相关字符串。

## `executorType` 两种取值

```js
// 在 headless DTW harness 里强制 "sandbox"
await T.executorRuntime.bootstrapExecutor({
  executorType: "sandbox",
  trigger: R,
  workspaceRoot: this.options.workspaceRoot,
  threadID: T.threadId,
  ...
});
```

```js
// 在本地 CLI 里，默认
executorType: "local-client"
```

**判断**：`executorType` 是**运行形态标签**，不是隔离级别标签 —— 它告诉 system prompt builder "当前在哪跑"，从而决定要不要加 sandbox-specific 指引。**没证据说本地 CLI 有"安全 sandbox"模式**，所有沙箱相关文案都指向**远程 DTW worker**。

## Sandbox 模式下的 System Prompt 注入

位置：system prompt builder（`A.push(...)` 链路），条件 `r.meta?.executorType === "sandbox"`。

按顺序注入三段：

### 1. `.amp/in/artifacts` 用法规则（`cqT`）

```
Use `.amp/in/artifacts` only for files the user should review in the
Artifacts tab, such as screenshots, videos, or data exports. Keep build
artifacts, transient inspection screenshots, and other temporary scratch/
debug files out of that folder. When you mention an artifact saved there,
link to it; for image artifacts, prefer Markdown image format using a
workspace file URI, for example
  ![screenshot](file:///workspace/.amp/in/artifacts/example.png)
`.amp/out` is not indexed by that tab.
```

**解读**：sandbox 里 agent 产物会被 web UI (ampcode.com) 的 "Artifacts tab" 展示 —— 这是 DTW 场景专属功能，本地 CLI 没这 tab，所以本地不注入。

### 2. Shallow Git 提示（`h6R`）

```
Git history note: This checkout is shallow; run `git fetch --unshallow`
before using git history commands like `git log` or `git blame`, or
trying to find a specific commit.
```

**解读**：sandbox 克隆仓库是 shallow（只拿最新 commit，省时间）—— 和 CI runner 一样。Amp 提前告诉 LLM 别直接 `git log` 就抓瞎。

### 3. Preview URL 指引（`zwR` 动态生成）

这个最有意思。完整输出：

```
Sandbox preview URLs: The user cannot open sandbox-local URLs directly,
so never tell them to use raw localhost or 127.0.0.1 for sandbox web servers.
Only share a preview URL when this environment or repo explicitly provides
how to derive one.
[IF .agents/preview exists:]
  This repo has preview instructions at `<uri>`; read that file and follow
  it before sharing a preview URL, and do not invent a URL pattern.
[ELSE:]
  If the repo has a `.agents/preview` file, read it and follow it.
  Otherwise, say that you do not have a configured preview URL instead of
  guessing.
When you do have a preview URL, hyperlink it.
```

**解读**：sandbox 里跑 dev server 的 localhost **对用户不可访问**（sandbox 是远端、用户在自己机器上）。Amp 把"怎么映射 dev server → 可访问 URL"的规则**外包给了 repo 自己** —— 通过 `.agents/preview` 文件。

## `.agents/preview` 文件约定

### 路径

常量 `a6R=".agents/preview"` —— 固定路径，无扩展名。

### 查找逻辑（`qwR` 函数）

```js
async function qwR(T, R) {
  if (!R) return null;                          // 无 workspaceRoot
  let t = LR.joinPath(Be(R), ...a6R.split("/"));
  try {
    await T.stat(t);                             // 只 stat，不 read
    return ze(t);                                // 返回 URI
  } catch (r) {
    if (r instanceof Ah) return null;            // FileNotFound
    // log warning, 返回 null
  }
}
```

**只检查文件存不存在**，不解析内容。内容交给 LLM 自己去 read。

### 内容格式

**binary 里没有 schema、没有解析器**。Amp 让 LLM `read_file(.agents/preview)` 自己读，然后按里面的**自然语言**指令映射 URL。

**推测** —— 可能的实际内容：
```
# .agents/preview
Dev server runs on port 3000 inside the sandbox.
Public preview URL pattern: https://preview-{threadId}.amp.dev/
```

或者命令形式：
```
Run `./scripts/get-preview-url.sh <port>` to resolve a port to a public URL.
```

格式完全看 repo 作者自由发挥。Amp 只保证 "如果存在就读、不存在就诚实说不知道"。

## DTW headless harness 入口链路

`kVT` 类（在 strings 63354-... 行）—— Amp 的 "Headless DTW Harness"：

```js
class kVT {
  async start() {
    // 建 ws 连到 rivet actor / Cloudflare workers endpoint
    let T = await this.createClient(this.options.threadId);
    await this.runLoop();
  }

  async connectAsExecutor(T, R) {
    await T.executorRuntime.bootstrapExecutor({
      executorType: "sandbox",           // ← 这里
      trigger: R,
      workspaceRoot: this.options.workspaceRoot,
      threadID: T.threadId,
    });
  }
}
```

**连接点**：
- WebSocket 经过 `teT(ampURL)` / `RIVET_PUBLIC_ENDPOINT` 环境变量 → Cloudflare / Rivet。
- `threadActor.getOrCreate([T], { params: { apiKey, threadId, executorClientId }})` —— 推测 Rivet actor = thread 的 Durable Object 封装。
- 相关 CLI flag 在 `dtw.md` 里（`--apply <threadID>`、`--checkout`、`--worker-url`）。

## 本地 CLI sandbox 模式？

**没证据**。本地 CLI 的 executor 始终是 `local-client`；想跑 sandbox 必须通过 "Headless DTW Harness" 这个专门入口（不是普通 `amp` 命令直接开的）。

**推测**：可能只有 ampcode.com web UI 或 Aggman（orchestrator）会触发 headless harness 启动 —— 即用户从 web UI 点"在远程跑这个 thread"，Sourcegraph 的基础设施端才起一个 headless 进程连回 DTW。

## 没有的

查过但**没找到**：
- `sandbox-exec` / Seatbelt profile（macOS 原生沙箱）—— 完全没提。
- `firecracker` / `runc` / `containerd` —— 0 命中。
- `seccomp` / `capabilities` —— 0 命中。
- `docker` —— 命中但都是无关（markdown 教用户运行 docker 命令的 string）。

**结论**：Amp 的 "sandbox" **不是** OS 级沙箱 —— 它是**"远程 DTW executor 运行形态"** 的代称，隔离由 Cloudflare Workers + Durable Objects 架构天然提供。

## 对 Alva 的启发

Alva 目前是 local-first，不直接需要 DTW。但如果未来做**远程 runtime**（CI agent、Web 触发的云端 agent）：

1. **`.agents/preview` 约定可以抄**：
   - 把"如何把内部 URL 映射成用户可访问 URL"放进 repo，由 LLM 运行时读取。
   - 文件路径可以用 `.alva/preview` 避免冲突。
   - 格式完全自由，LLM 能理解的自然语言 or shell 命令。
2. **`executorType` 运行形态标签**：Alva 的 `EngineRuntime` trait 可以加 `runtime_kind: "local" | "remote"`，driver 层据此调整 system prompt（本地不注入 shallow-git 提示、远程要）。
3. **Shallow git 提示**：远端 runtime 为了冷启动快会 shallow clone，必须提前告诉 LLM —— 直接抄 h6R 文案。
4. **别直接给 localhost URL**：远端 agent 永远不要回复 `http://localhost:3000` —— 这是 Amp 栽过坑总结出的规则。

不要抄：
- `executorType:"sandbox"` 字面量名 —— 容易误导（这不是"安全沙箱"）。Alva 用 `runtime_kind:"remote"` 更诚实。
- Artifacts tab 规则 —— Alva 不一定有 web UI 的产物 tab。

## 引用

- `/tmp/amp-decompile/strings.txt`
  - `executorType:"sandbox"`：63354 行附近（`kVT.connectAsExecutor`）。
  - `executorType:"local-client"`：63590 行附近。
  - `zwR`、`qwR`、`a6R=".agents/preview"`、`cqT`、`h6R`：63005-65278 行区间。
  - System prompt 注入分支：63009 行附近，`r.meta?.executorType==="sandbox"?cqT:null` 等三个三目。
- 关联：`../remote-runtime/dtw.md` 有更完整的 DTW 架构推断。
