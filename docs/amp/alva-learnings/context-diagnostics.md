# `alva context` —— Context 诊断 CLI

> 让开发者一眼看清 context 被什么占了，以及 prompt caching 工作情况。

---

## 背景

详见 [`../context/diagnostics.md`](../context/diagnostics.md)。

核心价值：

1. 用户问 "我的 token 费用怎么这么贵" —— 这个命令能秒回答
2. 开发者调 prompt 时能精确看到"改这一行让基础 prompt 变了多少 tokens"
3. 生产期排查 "cache miss 率突然变高" 有迹可循

---

## 输出目标

```
$ alva context --thread T-xxx

Context Usage Analysis
──────────────────────────────────────────────────
Model: claude-sonnet-4.5 (200,000 context)

Sections:
  System prompt         1,234  (0.6%)
  AGENTS.md             2,345  (1.2%)
    agents/core         1,100
    agents/skills       1,245
  Tools                12,456  (6.2%)
    builtin             5,678
    mcp                 4,500
    toolbox             2,278
  Skills index            542  (0.3%)
  Thread history      145,678 (72.8%)
    user messages      20,345
    assistant         100,000
    tool_results       25,333

Used:  162,255 tokens (81.1%)
Free:   37,745 tokens

──────────────────────────────────────────────────
Caching (from last inference):
  Total input:         152,100 tokens
    Plain input:         2,345
    Cache create:        5,000
    Cache read:        144,755  ✓ 95.2% hit rate
  
  Compared to current analysis: +10,155 tokens new
```

---

## Rust 实现骨架

### Step 1：CLI 子命令

```rust
// alva-app-cli/src/cli/context.rs

#[derive(Parser)]
pub struct ContextCmd {
    /// Thread ID (defaults to most recent)
    pub thread_id: Option<String>,
    
    /// Output as JSON
    #[arg(long)]
    pub json: bool,
    
    /// Show per-message token breakdown (verbose)
    #[arg(long, short = 'v')]
    pub verbose: bool,
}

pub async fn handle_context_cmd(args: ContextCmd, app: &App) -> Result<()> {
    let thread_id = args.thread_id.unwrap_or_else(|| app.last_thread_id());
    let analysis = analyze_context(&thread_id, app).await?;
    
    if args.json {
        println!("{}", serde_json::to_string_pretty(&analysis)?);
    } else {
        render_analysis(&analysis, args.verbose);
    }
    
    Ok(())
}
```

### Step 2：分析实现

```rust
// alva-app-cli/src/cli/context_analysis.rs

#[derive(Serialize)]
pub struct ContextAnalysis {
    pub thread_id: String,
    pub model: ModelInfo,
    pub max_context_tokens: u64,
    pub total_tokens: u64,
    pub free_space: u64,
    pub sections: Vec<Section>,
    pub tool_counts: ToolCounts,
    pub last_usage: Option<Usage>,
}

#[derive(Serialize)]
pub struct Section {
    pub name: String,
    pub tokens: u64,
    pub percentage: f64,
    pub children: Vec<Section>,
}

pub async fn analyze_context(thread_id: &str, app: &App) -> Result<ContextAnalysis> {
    let agent = app.load_thread(thread_id).await?;
    let thread = agent.session().current_thread().await?;
    
    // 1. 重新装配 system prompt
    let system_prompt = agent.inner().build_system_prompt().await?;
    
    // 2. 拿 tools
    let tools = agent.inner().available_tools().await?;
    
    // 3. 分 section 算 tokens
    let counter = HeuristicTokenCounter::new();  // 或从 LanguageModel 拿
    
    let system_tokens = count_block_tokens(&counter, &system_prompt.base);
    let agents_md_tokens = count_block_tokens(&counter, &system_prompt.agents_md);
    let env_tokens = count_block_tokens(&counter, &system_prompt.environment);
    let skills_tokens = count_block_tokens(&counter, &system_prompt.skills_index);
    
    let tools_tokens = tools.iter()
        .map(|t| counter.count_json_tool_def(t))
        .sum::<u64>();
    
    let history_tokens = count_messages(&counter, &thread.messages);
    
    let total = system_tokens + agents_md_tokens + env_tokens 
              + skills_tokens + tools_tokens + history_tokens;
    
    let model = agent.inner().current_model();
    let max = model.context_window();
    
    // 4. 构建 sections
    let sections = vec![
        Section::new("System prompt", system_tokens, total),
        Section::new_with_children("AGENTS.md", agents_md_tokens, total, 
            split_agents_md_by_source(&system_prompt.agents_md, &counter)),
        Section::new("Environment", env_tokens, total),
        Section::new("Skills index", skills_tokens, total),
        Section::new_with_children("Tools", tools_tokens, total,
            split_tools_by_source(&tools, &counter)),
        Section::new_with_children("Thread history", history_tokens, total,
            split_history_by_role(&thread.messages, &counter)),
    ];
    
    // 5. 拿 last usage（读 last assistant message 的 usage 字段）
    let last_usage = thread.messages.iter().rev()
        .find_map(|m| {
            if let Message::Assistant { usage: Some(u), .. } = m {
                if m.parent_tool_use_id.is_none() && u.total_input_tokens > 0 {
                    return Some(u.clone());
                }
            }
            None
        });
    
    Ok(ContextAnalysis {
        thread_id: thread_id.to_string(),
        model: model.info(),
        max_context_tokens: max,
        total_tokens: total,
        free_space: max.saturating_sub(total),
        sections,
        tool_counts: compute_tool_counts(&tools),
        last_usage,
    })
}
```

### Step 3：渲染

```rust
fn render_analysis(a: &ContextAnalysis, verbose: bool) {
    println!("{}", style("Context Usage Analysis").bold());
    println!("{}", "─".repeat(50));
    println!("Model: {} ({} context)", 
        style(&a.model.display_name).bold(),
        format_tokens(a.max_context_tokens));
    println!();
    
    println!("{}", style("Sections:").bold());
    let max_name_width = a.sections.iter()
        .flat_map(|s| std::iter::once(&s.name).chain(s.children.iter().map(|c| &c.name)))
        .map(|n| n.len())
        .max().unwrap_or(20);
    
    for section in &a.sections {
        println!("  {:width$} {:>8} {:>8}",
            section.name,
            format_tokens(section.tokens),
            format!("({:.1}%)", section.percentage),
            width = max_name_width + 2);
        
        if verbose {
            for child in &section.children {
                println!("{}",
                    style(format!("    {:width$} {:>8}",
                        child.name,
                        format_tokens(child.tokens),
                        width = max_name_width)).dim());
            }
        }
    }
    println!();
    
    let used_pct = a.total_tokens as f64 / a.max_context_tokens as f64 * 100.0;
    println!("Used:  {:>8} tokens ({:.1}% used)", 
        format_tokens(a.total_tokens), used_pct);
    println!("Free:  {:>8} tokens", format_tokens(a.free_space));
    println!();
    
    // Tools 细分
    let sources: Vec<String> = vec![
        format!("{} builtin", a.tool_counts.builtin),
        format!("{} mcp", a.tool_counts.mcp),
        format!("{} toolbox", a.tool_counts.toolbox),
        format!("{} plugin", a.tool_counts.plugin),
    ].into_iter().filter(|s| !s.starts_with("0 ")).collect();
    
    println!("{}: {} ({})", 
        style("Tools").dim(),
        a.tool_counts.total,
        sources.join(", "));
    
    // Cache stats
    if let Some(u) = &a.last_usage {
        println!();
        println!("{}", "─".repeat(50));
        println!("{}", style("Caching (from last inference):").bold());
        
        let total_input = u.input_tokens + u.cache_creation_input_tokens.unwrap_or(0) 
                        + u.cache_read_input_tokens.unwrap_or(0);
        println!("  Total input:         {:>8} tokens", format_tokens(total_input));
        println!("    Plain input:         {:>8}", format_tokens(u.input_tokens));
        if let Some(cc) = u.cache_creation_input_tokens {
            println!("    Cache create:        {:>8}", format_tokens(cc));
        }
        if let Some(cr) = u.cache_read_input_tokens {
            let hit_rate = cr as f64 / total_input as f64 * 100.0;
            let mark = if hit_rate > 80.0 { "✓" } else { "⚠" };
            println!("    Cache read:        {:>8}  {} {:.1}% hit rate",
                format_tokens(cr), mark, hit_rate);
        }
        
        let diff = a.total_tokens as i64 - total_input as i64;
        let sign = if diff >= 0 { "+" } else { "-" };
        println!();
        println!("  Compared to current analysis: {}{} tokens new",
            sign, format_tokens(diff.unsigned_abs()));
    }
}

fn format_tokens(n: u64) -> String {
    if n >= 1000 {
        format!("{:.1}K", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}
```

---

## 集成点

- 新 CLI 模块：`alva-app-cli/src/cli/context.rs`
- 可能需要在 `BaseAgent` / `Agent` 上暴露：
  - `build_system_prompt()` 返回结构化 blocks
  - `available_tools()` 拿所有工具列表
  - `current_model()` / `session().current_thread()`
- 如果 `LanguageModel::usage()` 还没存到 assistant message，这是前置 prerequisite

---

## 前置依赖

要让 `amp context` 有用，需要先做：

1. **`Message::Assistant.usage` 字段** —— 每次 LLM 调用存 Anthropic / OpenAI 返回的完整 usage（含 cache tokens）
2. **`build_system_prompt()` 返回结构化** —— 不能只是一个 string，要是 `Vec<Block>` 带 name
3. **`HeuristicTokenCounter`** —— 你们 `alva-kernel-abi` 已有，确认对每个 provider 准确度

---

## 进阶 feature（可以后续加）

### SHA 分片对比（见 [`../prompts/assembly-pipeline.md`](../prompts/assembly-pipeline.md)）

```
--show-changes    # 对比上一次 prompt 装配，显示哪些 block 变了
```

```
Context Build Diff (vs previous build)
──────────────────────────────────────────────────
  AGENTS.md:        changed
    Before:  a1b2c3d4e5f6a7b8
    After:   87654321fedcba09
    
  Tools:            unchanged (cached)
  System prompt:    unchanged (cached)
  
Expected cache miss due to:
  - AGENTS.md/project-notes.md modified at 15:30
```

### 历史趋势

```
$ alva context --thread T-xxx --history

Turn  Total    Cache Hit
1     10,234    n/a
2     15,678    85.2%
3     25,123    92.1%
...
```

### 和预估比较

```
--predict-next    # 如果加一条 user message 会增加多少 tokens
```

---

## 优先级

**高**。这是 Alva 差异化的一个维度：

- Claude Code CLI 没有这个
- Cursor 没有这个
- Aider 没有这个

Amp 有（说明有价值），你们抄了就有"市场上唯一有这个诊断能力的 coding agent"的宣传点。

---

## 工作量估算

**1-2 天** MVP 版本：

- Day 1：基础结构 + token counting + 基本输出
- Day 2：cache stats 对比 + verbose mode + 测试

后续进阶 feature 可按用户反馈加。
