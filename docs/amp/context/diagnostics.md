# Context Diagnostics —— `amp context` 命令 + 缓存监控

> 让开发者能**精确看到**上下文被什么占了，以及 prompt caching 工作得怎么样。

---

## `amp context` 子命令

CLI 子命令，输出当前线程的 context 分析：

```
Context Usage Analysis
──────────────────────────────────────────────────
Model: claude-sonnet-4.6 (200,000 context)
  System prompt         1,234  (0.6%)
  AGENTS.md             2,345  (1.2%)
    (agents.md a)       1,100
    (agents.md b)       1,245
  Tools                12,456  (6.2%)
  Thread history      145,678 (72.8%)
    (user messages)    20,345
    (assistant)       100,000
    (tool results)     25,333
Used:  160,000 tokens (80.0%)
Free:   40,000 tokens

Tools: 42 (18 builtin, 4 toolbox, 20 MCP)

──────────────────────────────────────────────────
Comparison with last inference:
  Last inference:   150,234 tokens
    (input: 10,234, cache-create: 20,000, cache-read: 120,000)
  Current analysis: 160,000 tokens
  Difference:       +9,766 tokens
```

---

## 实现

```js
async function runContextCommand(threadID, options) {
  let a = v2Worker;
  let analysis = a 
    ? await fetchAnalysisFromV2Worker({ ampURL, configService, threadID, workerURL })
    : await SFT(buildSystemPromptDeps, thread);    // 本地计算
  
  write(chalk.bold("Context Usage Analysis\n"));
  write(chalk.dim("─".repeat(50)));
  if (a) write(chalk.dim(`Source: v2 worker\n`));
  
  write(`Model: ${analysis.modelDisplayName} (${formatTokens(analysis.maxContextTokens)} context)\n`);
  
  // 分 section 打印
  let nameWidth = Math.max(...sections.map(s => s.name.length));
  for (let sec of analysis.sections) {
    write(`  ${sec.name.padEnd(nameWidth + 2)}${formatTokens(sec.tokens).padStart(8)} (${sec.percentage.toFixed(1)}%)\n`);
    for (let child of sec.children ?? []) {
      write(chalk.dim(`    ${child.name.padEnd(nameWidth)}${formatTokens(child.tokens).padStart(8)} (${child.percentage.toFixed(1)}%)\n`));
    }
  }
  
  let usedPct = (analysis.totalTokens / analysis.maxContextTokens * 100).toFixed(1);
  write(`Used:  ${formatTokens(analysis.totalTokens, true)} tokens (${usedPct}% used)\n`);
  write(`Free:  ${formatTokens(analysis.freeSpace, true)} tokens\n`);
  
  // 工具来源细分
  let sources = [`${analysis.toolCounts.builtin} builtin`];
  if (analysis.toolCounts.toolbox > 0) sources.push(`${analysis.toolCounts.toolbox} toolbox`);
  if (analysis.toolCounts.mcp > 0) sources.push(`${analysis.toolCounts.mcp} MCP`);
  write(chalk.dim(`Tools: ${analysis.toolCounts.total} (${sources.join(", ")})\n`));
  
  // 和上次 inference 对比
  let lastMsg = await getLastAssistantMessage(threadID);
  let lastUsage = lastMsg?.usage;
  if (lastUsage?.totalInputTokens) {
    let diff = analysis.totalTokens - lastUsage.totalInputTokens;
    write(chalk.dim("\n" + "─".repeat(50) + "\nComparison with last inference:\n"));
    write(chalk.dim(`  Last inference:   ${formatTokens(lastUsage.totalInputTokens, true).padStart(8)} tokens\n`));
    if (lastUsage.cacheCreationInputTokens || lastUsage.cacheReadInputTokens) {
      write(chalk.dim(`    (input: ${formatTokens(lastUsage.inputTokens)}, cache-create: ${formatTokens(lastUsage.cacheCreationInputTokens ?? 0)}, cache-read: ${formatTokens(lastUsage.cacheReadInputTokens ?? 0)})\n`));
    }
    write(chalk.dim(`  Current analysis: ${formatTokens(analysis.totalTokens, true).padStart(8)} tokens\n`));
    let sign = diff >= 0 ? "+" : "-";
    write(chalk.dim(`  Difference:       ${sign}${formatTokens(Math.abs(diff), true).padStart(7)} tokens\n`));
  }
}
```

---

## 数据源

两条路径（代码里明显看到）：

### A) 本地计算

本地重新跑 `YwR()` 装配 prompt → 用 tokenizer 对每 section 计 tokens。

```js
function SFT(deps, thread) {
  let systemPrompt = await buildSystemPrompt(deps, thread);
  // 对 systemPrompt.blocks 逐个算 tokens
  // 对 thread.messages 逐个算 tokens
  // 聚合输出
}
```

### B) V2 Worker（服务端）

向 ampcode.com 的 worker 请求分析（可能是云端的 tokenizer 更准）。

```js
async function AKT({ ampURL, configService, threadID, workerURL }) {
  let response = await fetch(`${ampURL}/api/context-analysis/${threadID}`, {
    headers: { Authorization: `Bearer ${token}` }
  });
  return response.json();
}
```

---

## `Pc(thread)` —— 高效读最后 inference 的 usage

```js
function Pc(thread) {
  for (let R = thread.messages.length - 1; R >= 0; R--) {
    let t = thread.messages[R];
    if (t?.parentToolUseId) continue;      // 跳过子 agent 的
    if (t?.role === "info") {
      let r = t.content[0];
      if (r?.type === "summary" && r?.summary.type === "message") return;
    }
    if (t?.role === "assistant" && t.usage) {
      if (t.usage.totalInputTokens === 0) continue;   // 跳过零的
      return t.usage;
    }
  }
  return;
}
```

**关键洞察**：Amp **不自己重算** token count。每次 LLM inference 时 Anthropic / OpenAI 返回的 `usage` 字段直接存到 assistant message。要当前 context 占用时就反向遍历找最后一条 usage。

**好处**：
- 免费（不用跑 tokenizer）
- 准确（是 API 方认可的数字）
- 自带 cache stats（能监控缓存命中率）

---

## prompt caching 监控

`usage` 字段包含：

```ts
type Usage = {
  inputTokens: number,                 // 本轮 user input + assistant context
  outputTokens: number,
  cacheCreationInputTokens?: number,   // 本轮"写入缓存"的 tokens（一次性贵）
  cacheReadInputTokens?: number,       // 本轮"命中缓存"的 tokens（便宜 10x）
  model?: string,
  totalInputTokens: number,            // input + cache-create + cache-read
}
```

**理想状态**：
- 稳定对话里，每轮 `cacheReadInputTokens` 占 80%+
- 只有新增的几条消息需要 `inputTokens` 真的算钱

**异常信号**：
- `cacheReadInputTokens` 突然大降 → cache 失效了
- 连续几轮 `cacheCreationInputTokens` 都很高 → cache 持续失效

结合 `zmT`/`FmT` 的 SHA 分片对比（见 `../prompts/assembly-pipeline.md`），Amp 能**精确定位**是哪个 block 变了导致 cache miss。

---

## SHA 分片对比 log

运行期 debug log 示例：

```
[DEBUG] System prompt build complete (CHANGES DETECTED)
  threadID: T-abc123
  changedKeys: ["contextBlock_3"]
  changedValues:
    contextBlock_3: "# Workspace Projects\n- new-proj: alice/new-proj"
```

用户看到这条 log，就知道"cache miss 是因为切换了 workspace 的 project 列表"。

---

## 对 Alva 的启发

你们 CLI 加一个 `alva context` 子命令，读 `BaseAgent::inner().build_system_prompt()` + `session.messages`，对每 section 用 `HeuristicTokenCounter` 算 tokens。

实现要点：

```rust
// alva-app-cli/src/cli/context.rs
pub async fn context_command(args: ContextArgs) -> Result<()> {
    let agent = BaseAgent::load(args.thread_id).await?;
    let prompt = agent.inner().build_system_prompt().await?;
    let msgs = agent.session().messages().await?;
    
    let counter = HeuristicTokenCounter::new();
    
    let sections = vec![
        ("System prompt", counter.count(&prompt.base)),
        ("AGENTS.md",     counter.count(&prompt.agents_md)),
        ("Tools",         counter.count_tools(&prompt.tools)),
        ("Thread history", counter.count_messages(&msgs)),
    ];
    
    render_table(sections);
    
    if let Some(last_usage) = last_inference_usage(&msgs) {
        render_cache_stats(last_usage);
    }
    
    Ok(())
}
```

配合你们 `LanguageModel::usage()` 返回的 cache stats 数据，就能做到和 Amp 同级的可观测性。

详见 [`../alva-learnings/context-diagnostics.md`](../alva-learnings/context-diagnostics.md)。
