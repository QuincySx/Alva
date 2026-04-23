---
name: amp-storage
description: Amp thread / message 的数据结构 + server-side 存储 + version vector 增量同步。需要设计自己的 session / thread 持久化时加载。
trigger_words:
  - thread storage
  - session storage
  - message schema
  - version vector
  - flushVersion
  - thread.v
  - server sync
  - thread persistence
  - discoveredGuidanceFiles
  - parentToolUseId
  - usage field
  - aggman context
---

# Amp Storage & Sync

Thread / Message 数据结构 + server-side 存储协议。

## 文件索引

| 文件 | 内容 | 何时读 |
|---|---|---|
| `./thread-model.md` | Thread / Message / ContentBlock / ToolRun 完整类型定义 | 想懂 Amp 存什么 |
| `./sync-protocol.md` | threadService API + version vector + visibility + DSL 搜索 | 想懂 Amp 怎么同步 |

## 关键事实（不用 load 子文件）

- **Thread 数据在 server（ampcode.com），不在本地** —— `~/.amp/` 只有 settings / plugins / secrets
- **Version vector**：`thread.v: number` monotonic，`flushVersion(id, v)` 推改动到 server
- **每条 assistant message 存 `usage`**：含 cache tokens，方便算当前 context
- **`discoveredGuidanceFiles` 快照在 user message 上**：AGENTS.md 版本绑定在当时消息
- **`parentToolUseId`**：子 agent message 都有，聚合统计时跳过避免重复计
- **`fromAggman` / `fromExecutorThreadID`**：区分 message 来源（orchestrator 注入 / execution thread 回调）
- **Info role message**：`role: "info"` 作为独立类型（summary / notice / tool-interrupt）

## Thread 数据结构骨架

```ts
Thread {
  id: "T-{uuid}",
  v: number,                              // monotonic version
  title, visibility, agentMode,
  messages: Message[],
  trees: [{uri, repository}],
  env?: { initial: ThreadEnvironment },
  meta?: { executorType: "local"|"sandbox"|"dtw" },
  parentThreadID?,                        // handoff 链
}

Message 的 union:
  UserMessage     { content, userState, discoveredGuidanceFiles, fromAggman? }
  AssistantMessage{ content, state, usage!, turnElapsedMs, parentToolUseId? }
  ToolMessage     { content: [{tool_use_id, content, is_error, run}] }
  InfoMessage     { content: [{type:"summary"|"notice"|...}] }
```

## 搜索 DSL（速查）

```
Keywords:    auth "race condition"
File:        file:src/auth/login.ts
Repo:        repo:github.com/owner/repo
Ref:         ref:main
Author:      author:me author:alice
Date:        after:2024-01-01 before:2024-02-01

组合（implicit AND）:
  auth file:src/foo.ts repo:amp ref:main
```

## 对 local-first 项目的启发

Amp 是云优先，不是所有思路都能抄。但这些可用：

- Version vector 比 event log 更简单，够 LLM 对话用
- 每条 assistant message 存 usage（含 cache tokens）
- 搜索 DSL 友好（`file:` `author:` `after:`）
- Ownership 检查 + visibility enum
