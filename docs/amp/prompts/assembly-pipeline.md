# System Prompt 动态装配流水线

System prompt 不是静态常量，而是每次 inference 前重新拼装。这个文档讲清楚**怎么拼、缓存怎么对齐**。

---

## 入口函数：`YwR(deps, thread, mode, abortSignal)`

```js
async function YwR(
  { configService, getThreadEnvironment, filesystem, skillService, ... },
  thread,
  mode,        // "deep" | "smart" | "speed" | "rush" | ...
  abortSignal
) {
  let isDeep = mode === "deep";
  let env    = thread.env?.initial ?? await getThreadEnvironment();
  let config = await configService.getLatest();
  let { workspaceRoot, workingDirectory, rootDirectoryListing } = await iqT(...);

  // ── 组装 context blocks 数组 ─────────────
  let blocks = [];

  // 1. AGENTS.md 发现
  let agentMdList = await ZN({ filesystem, configService }, thread);
  let agentMdOnly = agentMdList.filter(p => p.type !== "subtree");

  // 2. Base prompt header
  blocks.push({
    type: "text",
    text: isDeep ? DEEP_HEADER : NORMAL_HEADER    // e6R or r6R
  });

  // 3. AGENTS.md blocks
  let agentMdBlocks = await Promise.all(
    agentMdOnly.map(async p => {
      let content = await filesystem.readFile(URI.parse(p.uri));
      if (isDeep) {
        let text = `# AGENTS.md instructions for ${dir}\n<INSTRUCTIONS>\n${content}\n</INSTRUCTIONS>`;
      } else {
        let text = `Contents of ${path} (${description}):\n<instructions>\n${content}\n</instructions>`;
      }
      return { type: "text", text };
    })
  );

  // 4. Deep 模式 32 KiB 预算
  if (isDeep) {
    let totalBytes = 0;
    let included = [];
    for (let block of agentMdBlocks) {
      let size = new TextEncoder().encode(block.text).length;
      if (totalBytes + size > 32768) {      // cpR = 32768
        logger.warn("AGENTS.md guidance budget exceeded, truncating");
        break;
      }
      included.push(block);
      totalBytes += size;
    }
    blocks.push(...included);
  } else {
    blocks.push(...agentMdBlocks);
  }

  // 5. Environment block
  blocks.push({
    type: "text",
    text: [
      "# Environment",
      "Here is useful information about the environment you are running in:",
      `Today's date: ${new Date().toDateString()}`,
      `Working directory: ${workingDirectory ? formatPath(workingDirectory) : "(none)"}`,
      `Workspace root: ${workspaceRoot ? formatPath(workspaceRoot) : "(none)"}`,
      isSandbox ? sandboxBanner : null,
      env?.platform ? `Operating system: ${env.platform.os} (${env.platform.osVersion})...` : null,
      repoInfo ? `Repository: ${repoInfo.url}` : null,
      `Amp Thread URL: ${threadURL}`,
      isSandbox ? sandboxPreviewRules : null,
      !isDeep && rootDirectoryListing ? `## Directory listing\n${rootDirectoryListing}` : null
    ].filter(Boolean).join("\n")
  });

  // 6. Signed-In User + Workspace Projects
  blocks.push(...FwR(serverStatus));     // user profile
  blocks.push(...VwR(thread.messages));  // available projects (from aggman context)

  // 7. Skills 索引
  let skills = await skillService.getSkills();
  let skillText = isDeep ? OxR(skills) : vxR(skills);
  if (skillText) blocks.push({ type: "text", text: skillText });

  return { blocks };
}
```

---

## 组装步骤详解

### 步骤 1：Base Header

**Deep 模式头**（`e6R`）：

```
# AGENTS.md guidance files

The following files contain workspace-specific guidance. Treat them as 
ground truth for commands, style, and structure of this project. When 
discovering a new recurring command or convention, consider asking the 
user to append it here.
```

**普通模式头**（`r6R`）：

```
The user has provided the following files to use as additional context:
```

### 步骤 2：AGENTS.md 发现与注入

AGENTS.md 发现遵循层级规则：
- Workspace root 的 `AGENTS.md`（或 `CLAUDE.md` 作为 fallback）
- 所有子目录的 AGENTS.md（作为 subtree 规则，仅在相关目录下生效）

**Deep 模式格式**（XML 大写标签）：

```
# AGENTS.md instructions for /path/to/dir

<INSTRUCTIONS>
(AGENTS.md 内容)
</INSTRUCTIONS>
```

**普通模式格式**（小写标签 + 文件引用）：

```
Contents of file:///path/to/AGENTS.md (5 KB, 120 lines):
<instructions>
(AGENTS.md 内容)
</instructions>
```

### 步骤 3：32 KiB 预算

**只在 deep 模式强制**。超过记录 warn log：

```
logger.warn("AGENTS.md guidance budget exceeded, truncating remaining files", {
  totalBytes: 33500,
  budgetBytes: 32768,
  includedBlocks: 3,
  droppedBlocks: 2
});
```

### 步骤 4：Environment Block

固定格式的多行文本，**dynamically 产生**：

```
# Environment

Here is useful information about the environment you are running in:
Today's date: Mon Apr 21 2026
Working directory: /Users/alice/project
Workspace root: /Users/alice/project
Operating system: darwin (25.3.0) on arm64
Repository: https://github.com/alice/project
Amp Thread URL: https://ampcode.com/threads/T-xxx

## Directory listing
List of files (top-level only) in the user's workspace:
README.md
package.json
src/
test/
...
```

**Sandbox 特殊规则**：如果是 sandbox executor，追加：

```
Sandbox preview URLs: The user cannot open sandbox-local URLs directly, 
so never tell them to use raw localhost or 127.0.0.1 for sandbox web servers.
Only share a preview URL when this environment or repo explicitly provides 
how to derive one.
This repo has preview instructions at `.agents/preview`; read that file 
and follow it before sharing a preview URL, and do not invent a URL pattern.
When you do have a preview URL, hyperlink it.
```

### 步骤 5：Signed-In User Block（`FwR`）

```
# Signed-In User
- Amp username: alice
- Connected GitHub login: @alice
- Connected Slack user ID: U012ABCDEF
```

（未登录的字段显示 "No stored X identity is currently known."）

### 步骤 6：Workspace Projects Block（`VwR`）

从 thread messages 倒着找最近一条带 `aggmanContext.availableProjects` 的 user message，提取出来：

```
# Workspace Projects
- my-frontend: alice/my-frontend (https://github.com/alice/my-frontend)
- my-backend: alice/my-backend (https://github.com/alice/my-backend)
```

去重 + 最多 50 个 project。

### 步骤 7：Skills 索引

详见 [`../skills/design.md`](../skills/design.md)。两种渲染：

- **普通模式 XML**（`vxR`）：紧凑，每个 skill 一个 `<skill>` 块
- **Deep 模式 Markdown**（`OxR`）：每个 skill 一行 bullet

---

## SHA-256 分片指纹（`zmT`/`Vf`）

**设计目的**：精确检测 prompt 哪部分变了，为 prompt caching 命中率提供可观测性。

### 分片哈希函数

```js
async function Vf(text) {
  let bytes = new TextEncoder().encode(text);
  let hash = await crypto.subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(hash))
    .map(r => r.toString(16).padStart(2, "0"))
    .join("")
    .slice(0, 16);   // 截前 16 hex
}
```

### 组合哈希函数

```js
async function zmT(basePrompt, contextComponents, additionalComponents, finalBlocks, tools) {
  let h = { basePrompt: await Vf(basePrompt) };
  for (let [i, c] of contextComponents.entries()) {
    h[`contextBlock_${i}`] = await Vf(c.text);
  }
  for (let [i, c] of additionalComponents.entries()) {
    h[`additionalBlock_${i}`] = await Vf(c.text);
  }
  for (let [i, c] of finalBlocks.entries()) {
    h[`finalBlock_${i}`] = await Vf(c.text);
  }
  h.tools = await Vf(JSON.stringify(tools.map(t => t.name)));
  return h;
}
```

返回的 hash map 长这样：

```js
{
  basePrompt:        "a1b2c3d4e5f6a7b8",
  contextBlock_0:    "12345678abcdef01",   // AGENTS.md #0
  contextBlock_1:    "87654321fedcba09",   // AGENTS.md #1
  contextBlock_2:    "...",                // Environment
  contextBlock_3:    "...",                // Signed-In User
  contextBlock_4:    "...",                // Workspace Projects
  contextBlock_5:    "...",                // Skills
  tools:             "aabbccdd11223344"
}
```

### 变化检测（`FmT`）

```js
function FmT(threadID, newHashes, source, meta) {
  let prev = S4.get(threadID);
  if (!prev) {
    logger.debug("System prompt build complete (first build)", { threadID, ...meta });
    S4.set(threadID, newHashes);
    return;
  }

  let changed = {};
  let changedValues = {};
  let allKeys = new Set([...Object.keys(prev), ...Object.keys(newHashes)]);

  for (let key of allKeys) {
    if (prev[key] !== newHashes[key]) {
      changed[key] = { old: prev[key] ?? "missing", new: newHashes[key] ?? "missing" };
      // 对于变化的 key，记录**原文**（不仅是 hash）
      if (key === "basePrompt") changedValues[key] = source.basePrompt;
      else if (key === "tools") changedValues[key] = source.tools.map(t => t.name);
      else if (key.startsWith("contextBlock_")) {
        let i = parseInt(key.split("_")[1]);
        changedValues[key] = source.contextComponents[i]?.text;
      }
      // ... 类似处理 additionalBlock / finalBlock
    }
  }

  if (Object.keys(changed).length > 0) {
    logger.debug("System prompt build complete (CHANGES DETECTED)", {
      threadID,
      changedKeys: Object.keys(changed),
      changedValues    // 原文对比
    });
    S4.set(threadID, newHashes);
  } else {
    logger.debug("System prompt build complete (no changes)", { threadID });
  }
}
```

### 用户价值

用户遇到 "为什么今天 Amp 贵了 3x" 时，log 直接告诉 debugger：

```
System prompt build complete (CHANGES DETECTED)
  threadID: T-xxxxx
  changedKeys: ["contextBlock_3"]
  changedValues:
    contextBlock_3: "# Workspace Projects\n- my-new-project: ..."
```

**定位 prompt caching 失效**从"毫无线索"变成"一目了然"。

---

## 模型选择路由（`ZwR`）

```js
function ZwR(model) {
  if (model.name === "gpt-5-codex") return "gpt-5-codex";   // codex 风格 prompt
  if (model.name.includes(...)) return "...";               // 其他 provider 路由
  return "default";                                          // 默认选
}
```

不同模型对 prompt 敏感度不同，Amp 按 model 选 prompt 变体。

---

## 内部测试用户特判（`QwR`）

```js
function QwR(serverStatus) {
  if (!serverStatus || !serverStatus.isAuthed()) return false;
  return serverStatus.features.some(f => f.name === HARNESS_SYSTEM_PROMPT && f.enabled)
      || isInternalEmail(serverStatus.user.email);   // @sourcegraph.com 等
}
```

内部员工 / 开了 feature flag 的用户会走新版 prompt。这是**灰度发布 prompt 修改**的基础设施。

---

## 设计启发

1. **Context 是 block 数组，不是字符串** —— 每块独立哈希，独立缓存，独立变化追踪。
2. **总是 log 分片变化** —— 为运营期的 cache 优化提供数据。
3. **Budget 硬限制**（32 KiB AGENTS.md） —— 不能让用户自己的文档把 context 撑爆。
4. **Feature flag 控制 prompt 变体** —— 灰度放量新 prompt 是标准操作。
5. **Environment 是动态的** —— date / repo / workspace root 等每次重算。
