# Thread 数据模型

> 从反编译里提取的 Thread / Message 结构。推断出的 TypeScript 接口（二进制里没有类型信息）。

---

## Thread

```ts
type Thread = {
  id: string;                          // "T-{uuid}" 格式
  v: number;                            // monotonic version
  
  // 展示字段
  title?: string;
  visibility: "private" | "public_discoverable" | "public_unlisted" 
            | "thread_workspace_shared" | "private_sharedGroups";
  agentMode: "smart" | "deep" | "speed" | "rush" | string;
  userLastInteractedAt: number;
  
  // 统计
  messageCount?: number;
  summaryStats?: SummaryStats;
  
  // 内容
  messages: Message[];
  
  // Trees（workspace 信息）
  trees?: Array<{
    uri: string;                        // file:// URI
    repository?: { url: string };
  }>;
  
  // Agent 相关
  env?: {
    initial: ThreadEnvironment;         // 首次启动时的 env 快照
  };
  meta?: {
    executorType: "local" | "sandbox" | "dtw";
    // ...
  };
  
  // 队列（UI 层概念）
  queuedMessages?: QueuedMessage[];
  
  // 父子关系（handoff 链）
  parentThreadID?: string;
  
  // sharing
  sharedGroupIDs?: string[];
};
```

### Thread Environment

```ts
type ThreadEnvironment = {
  platform?: {
    os: "darwin" | "linux" | "windows";
    osVersion: string;
    cpuArchitecture?: string;
    webBrowser?: boolean;               // running in web browser
  };
  trees?: Array<{
    uri: string;
    repository?: { url: string };
  }>;
  // ...
};
```

---

## Message

```ts
type Message = UserMessage | AssistantMessage | ToolMessage | InfoMessage;

type UserMessage = {
  role: "user";
  content: ContentBlock[];
  interrupted?: boolean;
  
  // ⭐ AGENTS.md 快照：即使文件后来改了，thread replay 时还能恢复当时规则
  discoveredGuidanceFiles?: Array<{ uri: string; lineCount: number }>;
  
  // User state 快照（Aggman 上下文的来源）
  userState?: {
    aggmanContext?: {
      availableProjects?: Array<{ name: string; repositoryURL: string }>;
    };
    runningTerminalCommands?: string[];
    activeEditor?: string;
    selectionRange?: { start: Position; end: Position };
    cursorLocation?: Position;
    cursorLocationLine?: string;
    // ...
  };
  
  // 其他来源标识
  fromAggman?: boolean;
  fromExecutorThreadID?: string;
  sentAt?: number;
};

type AssistantMessage = {
  role: "assistant";
  content: ContentBlock[];
  
  // 状态
  state: 
    | { type: "streaming" }
    | { type: "complete", stopReason: "end_turn" | "tool_use" | "max_tokens" | ... };
  
  // ⭐ 关键：每条 assistant message 都存当时的 token 用量
  usage?: {
    inputTokens: number;
    outputTokens: number;
    cacheCreationInputTokens?: number;
    cacheReadInputTokens?: number;
    model?: string;
    totalInputTokens: number;
  };
  
  // 计时
  turnStartTime?: number;
  turnElapsedMs?: number;
  
  // 子 agent 归属
  parentToolUseId?: string;
};

type ToolMessage = {
  role: "tool";
  content: Array<{
    type: "tool_result";
    tool_use_id: string;
    content: ContentBlock[];
    is_error?: boolean;
    run?: ToolRun;                      // 完整运行状态（见 tools/architecture.md）
  }>;
};

type InfoMessage = {
  role: "info";
  content: Array<{
    type: "summary" | "notice" | "tool-interrupt" | ...;
    summary?: { type: string; [k: string]: any };
    // ...
  }>;
};
```

---

## ContentBlock

```ts
type ContentBlock =
  | { type: "text"; text: string }
  | { type: "image"; source: { type: "base64"; media_type: string; data: string } }
  | { type: "thinking"; thinking: string; provider?: string }
  | { type: "redacted_thinking"; data: string; provider?: string }
  | { type: "tool_use"; id: string; name: string; input: any }
  | { type: "tool_result"; tool_use_id: string; content: ...; is_error?: boolean };
```

---

## ToolRun（工具执行状态）

```ts
type ToolRun =
  | { status: "in-progress"; progress?: ProgressData }
  | { status: "done"; result: any }
  | { status: "error"; error: { message: string; errorCode?: string } }
  | { status: "cancelled" }
  | { status: "rejected-by-user" }
  | { status: "blocked-on-user" };

type ProgressData = {
  statusMessage?: string;
  threadID?: string;                    // 子 agent 的 thread
  iteration?: number;                   // 子 agent 的轮次
  input?: string;
  output?: string;
  transcript?: Array<{ type: "input" | "output"; content: string }>;
  // 子 agent 专用：
  turns?: Array<TurnRecord>;
  activeTools?: Map<string, ToolInvocation>;
};
```

---

## 关键设计洞察

### 1. **Token usage 每轮 snapshot**

每条 `AssistantMessage.usage` 存当时 inference 的 token 数。`Pc(thread)` 反向遍历找最后一条 usage 就知道当前 context 大小 —— **不用自己跑 tokenizer 重算**。

### 2. **`discoveredGuidanceFiles` 绑在 user message 上**

不是 thread 全局字段，是**每条用户消息的独立快照**。这让 thread replay 时能精确恢复"用户发这句话时，AGENTS.md 里写的是什么"。

对应到 Alva：`RulesContextHooks` 不要只存当前版本的规则，每次变更要 snapshot 到下一条 user message 上。

### 3. **`parentToolUseId` 形成消息树**

子 agent 的所有消息都带 `parentToolUseId`，指向父 agent 里触发它的那个 tool_use。`Pc` 在聚合 usage 时**跳过**这些（因为 parent 的 usage 里已经包含了 child 的成本）。

### 4. **`fromAggman` / `fromExecutorThreadID` 区分消息来源**

在 Aggman + DTW 架构下，一条 user message 可能来自：
- 用户直接输入（无 fromXxx 标记）
- Aggman orchestrator 注入（`fromAggman: true`）
- 另一个 execution thread 回调（`fromExecutorThreadID: "T-yyy"`）

### 5. **`userState` 快照让 replay 忠实**

`activeEditor` / `selectionRange` / `cursorLocation` 等 IDE 上下文都存在 user message 上。重开 thread 时 UI 能恢复"当时用户指的是哪一行代码"。

### 6. **State machine 字段**

`AssistantMessage.state` 是状态机（streaming → complete+stopReason）。streaming 中断的消息会在 `state.type === "streaming"` 时被 `RIR(thread)` 函数**删除**（见 `prompts/assembly-pipeline.md`）。

### 7. **Queued messages 独立字段**

`thread.queuedMessages` 存**用户打字但还没发出去的 message**。UI 可以显示 "3 queued"。

---

## Message 消费端的常见操作

```js
// 1. 反向找最后一条 usage
function Pc(thread) {
  for (let R = thread.messages.length - 1; R >= 0; R--) {
    let m = thread.messages[R];
    if (m?.parentToolUseId) continue;    // skip subagent
    if (m?.role === "info") { ... continue; }
    if (m?.role === "assistant" && m.usage?.totalInputTokens > 0) {
      return m.usage;
    }
  }
}

// 2. 删除末尾 streaming 状态的 assistant
function RIR(thread) {
  while (lastMsg?.role === "assistant" && lastMsg.state.type === "streaming") {
    thread.messages.pop();
  }
}

// 3. 过滤非 anthropic 的 thinking blocks（跨模型切换时清理）
function NBT(messages) {
  return messages.map(msg => ({
    ...msg,
    content: msg.content.filter(b => {
      if (b.type !== "thinking" && b.type !== "redacted_thinking") return true;
      return !b.provider || b.provider === "anthropic";
    })
  }));
}

// 4. 消息 hash（用于 UI diff 渲染）
function nZ(msg) {
  // 返回能唯一识别这条 message 状态的 hash key
  // 包含 role / content 摘要 / usage 等
}
```

---

## 对 Alva 的启发

你们 `alva-kernel-abi::AgentSession` + `InMemorySession` 已经是对应的抽象。建议对照：

1. **`usage` 字段**：确认每条 assistant message 存了 Anthropic 返回的 usage（含 cache tokens），不要只存你们自己算的 token 估计。

2. **`discoveredGuidanceFiles` 快照**：`RulesContextHooks` 每次变更时 snapshot。

3. **`parentToolUseId`**：确认 `SubAgentExtension` 的子 agent 消息都带父 tool_use_id，聚合统计时能正确跳过。

4. **`InfoMessage` 独立 role**：Amp 有 `role: "info"` 类型的 message（tool interrupt / summary / notice 等），不是 user 也不是 assistant。你们 `Message` enum 里加不加这个？
