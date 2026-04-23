# Handoff 工具 —— 跨线程 Recap

> 取代自动 compaction 的思路：让 LLM 自己开新 thread + 带 first-person recap。

---

## 背景

详见 [`../context/handoff.md`](../context/handoff.md)。

核心洞察：**Amp 没有自动 compaction**。当 context 撑不下时，LLM 自己调 `handoff` 工具开新 thread。这比 "harness 按阈值触发自动压缩" 更好：

- 判断时机更准（LLM 感知自身 degrading）
- 用户感知更自然（像"新开对话"，不像"被吞了"）
- 可追溯（旧 thread 保留）

---

## 建议的 Rust 实现

### Step 1：加一个 `handoff` tool

```rust
// alva-agent-extension-builtin/src/tools/handoff.rs

use alva_kernel_abi::{Tool, ToolInput, ToolResult};

pub struct HandoffTool {
    thread_service: Arc<dyn ThreadService>,
    handoff_context_tool: Arc<HandoffContextTool>,
}

impl Tool for HandoffTool {
    fn name(&self) -> &str { "handoff" }
    
    fn description(&self) -> String {
        r#"Create a new thread to continue this work with a fresh context window.

When to use:
- The current thread is getting too long and context is degrading
- You want to start a new focused task while preserving key context
- The current thread's context window is near capacity

When you call this tool:
1. A new thread will be created with relevant context from this thread
2. The new thread will start running with the provided goal
3. The current thread continues to exist (not deleted)

If `follow: true`, the UI switches to the new thread. Otherwise, the new
thread runs in the background.
"#.into()
    }
    
    fn input_schema(&self) -> JsonSchema {
        json_schema! {
            "type": "object",
            "properties": {
                "goal": {
                    "type": "string",
                    "description": "A short description of the next task to accomplish. 
                                    Single sentence or short paragraph. Focus on what 
                                    needs to be done next, not what was already completed."
                },
                "follow": {
                    "type": "boolean",
                    "default": false,
                    "description": "If true, navigate to the new thread after creation."
                },
                "mode": {
                    "type": "string",
                    "description": "Agent mode for the new thread (smart/deep/rush/...)"
                }
            },
            "required": ["goal"]
        }
    }
    
    async fn call(&self, args: ToolInput, ctx: ToolContext) -> ToolResult {
        let goal = args.get_string("goal").ok_or_else(|| anyhow!("goal required"))?;
        let follow = args.get_bool("follow").unwrap_or(false);
        let mode = args.get_string("mode");
        
        // 1. Call handoff_context tool to get recap + files
        let HandoffContext { relevant_information, relevant_files } = 
            self.handoff_context_tool.extract(&ctx).await?;
        
        // 2. Create new thread
        let parent_thread_id = ctx.thread_id.clone();
        let new_thread = self.thread_service.create_thread(CreateThreadArgs {
            parent_thread_id: Some(parent_thread_id.clone()),
            agent_mode: mode.or_else(|| ctx.agent_mode.clone()),
            initial_env: ctx.env.clone(),
        }).await?;
        
        // 3. Build initial user message (goal + recap + relevant files)
        let mut initial_content = format!(
            "{}\n\n<prior_context>\n{}\n</prior_context>",
            goal.trim(), relevant_information.trim()
        );
        
        for file_path in &relevant_files {
            match ctx.filesystem.read_to_string(file_path).await {
                Ok(content) => {
                    initial_content.push_str(&format!(
                        "\n\n## {}\n\n```\n{}\n```",
                        file_path, content
                    ));
                }
                Err(_) => continue,
            }
        }
        
        // 4. Post message to new thread
        self.thread_service.append_message(
            &new_thread.id,
            Message::user(initial_content),
        ).await?;
        
        // 5. Start the new thread running (async / in background)
        tokio::spawn(async move {
            // run_agent 新 thread
        });
        
        // 6. Notify UI if follow
        if follow {
            ctx.steer_to_thread(&new_thread.id).await?;
        }
        
        Ok(ToolResult::Done(json!({
            "new_thread_id": new_thread.id,
            "new_thread_url": format!("alva://threads/{}", new_thread.id),
            "following": follow,
        })))
    }
}
```

### Step 2：`handoff_context` 辅助工具

```rust
// alva-agent-extension-builtin/src/tools/handoff_context.rs

pub struct HandoffContextTool;

#[derive(Debug, Deserialize)]
pub struct HandoffContext {
    pub relevant_information: String,
    pub relevant_files: Vec<String>,
}

impl HandoffContextTool {
    pub async fn extract(&self, ctx: &ToolContext) -> Result<HandoffContext> {
        // 让 LLM 基于当前 thread 产出 recap
        let prompt = r#"
Extract relevant context from the conversation above for continuing this work.
Write from my perspective (first person: "I did...", "I told you...").

Consider what would be useful to know. Questions that might be relevant:
- What did I just do or implement?
- What instructions did I already give you which are still relevant?
- What files did I already tell you that's important?
- Did I provide a plan or spec that should be included?
- What important technical details did I discover?
- What caveats, limitations, or open questions did I find?

Extract what matters for continuing the task. Don't answer questions that 
aren't relevant.

Focus on capabilities and behavior, not file-by-file changes. Avoid 
excessive implementation details (variable names, storage keys, constants) 
unless critical.

Format: Plain text with bullets. No markdown headers, no bold/italic, no 
code fences. Use workspace-relative paths for files.

Return as JSON:
{
  "relevantInformation": "<first-person recap>",
  "relevantFiles": ["path1", "path2", ...]    // max 10, most important first
}
"#;
        
        // 用当前 LLM 生成（或用便宜模型如 Haiku）
        let response = ctx.llm.call_with_json_output::<HandoffContext>(
            &ctx.thread.messages,
            prompt,
        ).await?;
        
        Ok(response)
    }
}
```

### Step 3：集成到 `BaseAgent`

```rust
// alva-app-core/src/extensions/handoff_extension.rs

pub struct HandoffExtension;

impl Extension for HandoffExtension {
    fn name(&self) -> &str { "handoff" }
    
    async fn configure(&self, ctx: &ExtensionContext) -> Result<()> {
        let thread_service = ctx.bus_reader.require::<Arc<dyn ThreadService>>()?;
        let handoff_context_tool = Arc::new(HandoffContextTool);
        let handoff_tool = Arc::new(HandoffTool {
            thread_service,
            handoff_context_tool,
        });
        ctx.register_tool(handoff_tool)?;
        Ok(())
    }
}

// BaseAgentBuilder 默认装上
impl BaseAgentBuilder {
    pub fn build(self, model: LanguageModel) -> BaseAgent {
        let mut builder = self.inner;
        if !self.has_extension("handoff") {
            builder = builder.extension(Box::new(HandoffExtension));
        }
        // ...
    }
}
```

### Step 4：CLI 命令

```rust
// alva-app-cli/src/cli/handoff.rs

#[derive(Parser)]
pub struct HandoffCmd {
    /// Goal for the new thread
    #[arg(long, short = 'g')]
    pub goal: Option<String>,
    
    /// Print new thread ID instead of entering TUI
    #[arg(long, short = 'p')]
    pub print: bool,
    
    /// Source thread ID (or use most recent)
    pub thread_id: Option<String>,
}

pub async fn handle_handoff_cmd(args: HandoffCmd, app: App) -> Result<()> {
    let goal = match args.goal {
        Some(g) => g,
        None => {
            // 从 stdin 读（支持 pipe）
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            if buf.trim().is_empty() {
                bail!("goal required via --goal or stdin");
            }
            buf
        }
    };
    
    let source_thread_id = args.thread_id.unwrap_or_else(|| app.last_thread_id());
    let agent = app.load_thread(&source_thread_id).await?;
    
    // 调 handoff tool
    let result = agent.call_tool("handoff", json!({
        "goal": goal,
        "follow": !args.print,
    })).await?;
    
    if args.print {
        println!("{}", result["new_thread_id"]);
    } else {
        // 进 TUI，跳转到新 thread
        app.enter_tui_with_thread(&result["new_thread_id"].as_str().unwrap()).await?;
    }
    
    Ok(())
}
```

---

## 关键设计决策

### 1. `handoff` 作为 Tool，不是 Harness 逻辑

让 LLM 自己决定时机（通过 tool description 教会它）。比阈值触发准。

### 2. Two-step 流程（handoff → handoff_context）

分开让：
- `handoff` 是简单的"创建新 thread"
- `handoff_context` 可以独立测试 / 替换（用不同模型 / 不同 prompt）

### 3. 新 thread 记录 `parent_thread_id`

保持可追溯。UI 可以显示 "This thread continues from T-xxx"。

### 4. 默认 `follow: false`

不打扰用户。如果用户说 "new thread and I'll wait"，模型才设 true。

---

## 集成点

- 新 crate / 模块：`alva-agent-extension-builtin::tools::handoff`
- 修改 `BaseAgentBuilder` 默认装上
- 新 CLI：`alva handoff` 子命令
- `ThreadService` trait 要支持 `create_thread(parent_thread_id)` 和 `append_message`

---

## 优先级

**中高**。用户价值显著：

- "对话太长了" 是 agent 产品的高频痛点
- Handoff 体验比 "输出被摘要吞了" 好十倍
- 对话 debug / review 变简单（老 thread 还在）

建议在 context management 成熟后上。

---

## 和 `CompactionExtension` 的关系

你们已有的 `CompactionExtension` 可以继续保留作为**可选**机制：

- 用户主动 `/compact` → 用 CompactionExtension
- LLM 主动 handoff → 用 HandoffExtension

两者不冲突，给用户两种工具。
