// INPUT:  JsonFileSessionManager, alva_kernel_abi::{AgentMessage, HeuristicTokenCounter, TokenCounter}
// OUTPUT: run(args): dispatcher for `alva context ...` subcommand
// POS:    CLI-only context diagnostics — shows the user what's eating their token budget.

//! `alva context` — answer "why is my token bill so big" without a Slack thread.
//!
//! # Subcommands
//!
//! - `alva context` — analyze the latest session in the workspace
//! - `alva context <session_id>` — analyze a specific session
//! - `alva context --list` — show every session's rough token size
//!
//! # What this command reports
//!
//! - Per-role message counts + token estimates (user / assistant / tool result)
//! - Total estimated thread token cost
//! - Last captured assistant `UsageMetadata` (if present), including prompt
//!   cache hit rate
//!
//! # What this command does NOT report
//!
//! System prompt + tool definitions are rebuilt at agent start and aren't
//! stored in the session file, so they're invisible here. What you see is
//! the conversation history's contribution — which is the growing part
//! anyway (system+tools are roughly fixed per agent build).

use std::path::Path;

use alva_kernel_abi::base::message::{AgentMessage, MessageRole, UsageMetadata};
use alva_kernel_abi::model::{HeuristicTokenCounter, TokenCounter};
use alva_kernel_abi::{AgentSession, ContentBlock};

use crate::session::JsonFileSessionManager;

pub async fn run(args: &[String]) -> i32 {
    match args.first().map(|s| s.as_str()) {
        Some("--help") | Some("-h") | Some("help") => {
            print_help();
            0
        }
        Some("--list") => run_list().await,
        Some(session_id) => run_analyze(Some(session_id)).await,
        None => run_analyze(None).await,
    }
}

fn print_help() {
    eprintln!("alva context — diagnose token usage in a session");
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("  alva context              Analyze the latest session");
    eprintln!("  alva context <id>         Analyze a specific session");
    eprintln!("  alva context --list       List all sessions with size estimates");
    eprintln!();
    eprintln!("Note: system prompt + tool definitions are built fresh each agent");
    eprintln!("run and aren't recorded in session files, so they're not counted.");
    eprintln!("What you see is the conversation history — which is the part");
    eprintln!("that grows and drives your token bill.");
}

// ---------------------------------------------------------------------------
// --list: every session's rough size
// ---------------------------------------------------------------------------

async fn run_list() -> i32 {
    let workspace = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let mgr = JsonFileSessionManager::for_workspace(&workspace);
    let entries = mgr.list();
    if entries.is_empty() {
        println!("No sessions in {}", workspace.display());
        return 0;
    }
    println!("{:<40}  {:>8}  {:>8}  preview", "session_id", "events", "~tokens");
    println!("{}", "─".repeat(80));
    let counter = HeuristicTokenCounter::new(200_000);
    for e in &entries {
        let tokens = match mgr.load(&e.session_id).await {
            Some(sess) => estimate_thread_tokens(&*sess, &counter).await,
            None => 0,
        };
        let preview = if e.preview.len() > 30 {
            format!("{}…", &e.preview[..30.min(e.preview.len())])
        } else {
            e.preview.clone()
        };
        println!(
            "{:<40}  {:>8}  {:>8}  {}",
            e.session_id,
            e.event_count,
            format_tokens(tokens),
            preview
        );
    }
    0
}

// ---------------------------------------------------------------------------
// Analyze one session
// ---------------------------------------------------------------------------

async fn run_analyze(session_id: Option<&str>) -> i32 {
    let workspace = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let mgr = JsonFileSessionManager::for_workspace(&workspace);

    let sid = match session_id {
        Some(s) => s.to_string(),
        None => match mgr.latest() {
            Some(s) => s,
            None => {
                eprintln!("No sessions found in {}", workspace.display());
                return 0;
            }
        },
    };

    let session = match mgr.load(&sid).await {
        Some(s) => s,
        None => {
            eprintln!("Session not found: {sid}");
            return 1;
        }
    };

    let counter = HeuristicTokenCounter::new(200_000);
    let messages = session.messages().await;
    let analysis = analyze(&messages, &counter);

    render(&sid, &workspace, &analysis);
    0
}

// ---------------------------------------------------------------------------
// Analysis
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Analysis {
    user_count: usize,
    user_tokens: usize,
    assistant_count: usize,
    assistant_text_tokens: usize,
    assistant_reasoning_tokens: usize,
    tool_use_count: usize,
    tool_use_tokens: usize,
    tool_result_count: usize,
    tool_result_tokens: usize,
    system_count: usize,
    system_tokens: usize,
    total_events: usize,
    last_usage: Option<UsageMetadata>,
}

impl Analysis {
    fn total_message_tokens(&self) -> usize {
        self.user_tokens
            + self.assistant_text_tokens
            + self.assistant_reasoning_tokens
            + self.tool_use_tokens
            + self.tool_result_tokens
            + self.system_tokens
    }
}

fn analyze(messages: &[AgentMessage], counter: &HeuristicTokenCounter) -> Analysis {
    let mut a = Analysis::default();
    for m in messages {
        a.total_events += 1;
        let AgentMessage::Standard(msg) = m else {
            continue;
        };
        if let Some(u) = &msg.usage {
            if u.input_tokens > 0 || u.output_tokens > 0 {
                a.last_usage = Some(u.clone());
            }
        }
        match msg.role {
            MessageRole::User => {
                a.user_count += 1;
                a.user_tokens += count_message_body(msg, counter);
            }
            MessageRole::Assistant => {
                a.assistant_count += 1;
                for b in &msg.content {
                    match b {
                        ContentBlock::Text { text } => {
                            a.assistant_text_tokens += counter.count_tokens(text);
                        }
                        ContentBlock::Reasoning { text } => {
                            a.assistant_reasoning_tokens += counter.count_tokens(text);
                        }
                        ContentBlock::ToolUse { name, input, .. } => {
                            a.tool_use_count += 1;
                            a.tool_use_tokens +=
                                counter.count_tokens(name)
                                    + counter.count_tokens(&input.to_string());
                        }
                        _ => {}
                    }
                }
            }
            MessageRole::Tool => {
                for b in &msg.content {
                    if let ContentBlock::ToolResult { content, .. } = b {
                        a.tool_result_count += 1;
                        let body: String = content
                            .iter()
                            .map(|c| c.to_model_string())
                            .collect::<Vec<_>>()
                            .join("\n");
                        a.tool_result_tokens += counter.count_tokens(&body);
                    }
                }
            }
            MessageRole::System => {
                a.system_count += 1;
                a.system_tokens += count_message_body(msg, counter);
            }
        }
    }
    a
}

fn count_message_body(msg: &alva_kernel_abi::Message, counter: &HeuristicTokenCounter) -> usize {
    msg.content
        .iter()
        .map(|b| match b {
            ContentBlock::Text { text } => counter.count_tokens(text),
            ContentBlock::Reasoning { text } => counter.count_tokens(text),
            ContentBlock::Image { .. } => 1000,
            ContentBlock::ToolUse { name, input, .. } => {
                counter.count_tokens(name) + counter.count_tokens(&input.to_string())
            }
            ContentBlock::ToolResult { content, .. } => content
                .iter()
                .map(|c| counter.count_tokens(&c.to_model_string()))
                .sum(),
            // ContentBlock is `#[non_exhaustive]` — unknown future variants
            // contribute zero to the estimate rather than breaking compile.
            _ => 0,
        })
        .sum()
}

async fn estimate_thread_tokens(
    session: &dyn AgentSession,
    counter: &HeuristicTokenCounter,
) -> usize {
    let msgs = session.messages().await;
    analyze(&msgs, counter).total_message_tokens()
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

fn render(session_id: &str, workspace: &Path, a: &Analysis) {
    let total = a.total_message_tokens();
    println!();
    println!("Context Usage — session {session_id}");
    println!("Workspace: {}", workspace.display());
    println!("{}", "─".repeat(60));
    println!();
    println!("Thread history — {} tokens estimated", format_tokens(total));
    println!();
    print_row("user", a.user_count, a.user_tokens, total);
    print_row(
        "assistant (text)",
        a.assistant_count,
        a.assistant_text_tokens,
        total,
    );
    if a.assistant_reasoning_tokens > 0 {
        print_row(
            "assistant (reasoning)",
            0,
            a.assistant_reasoning_tokens,
            total,
        );
    }
    print_row(
        "assistant tool_use",
        a.tool_use_count,
        a.tool_use_tokens,
        total,
    );
    print_row("tool result", a.tool_result_count, a.tool_result_tokens, total);
    if a.system_tokens > 0 {
        print_row("system", a.system_count, a.system_tokens, total);
    }

    println!();
    println!("{} {}", "Total events in session:", a.total_events);

    // Caching stats — only if we captured a usage with cache info.
    if let Some(u) = &a.last_usage {
        let total_input = u.input_tokens as u64
            + u.cache_creation_input_tokens.unwrap_or(0) as u64
            + u.cache_read_input_tokens.unwrap_or(0) as u64;
        println!();
        println!("{}", "─".repeat(60));
        println!("Last LLM call usage (Anthropic-style; others may lack cache fields):");
        println!("  Input tokens:        {}", format_tokens(u.input_tokens as usize));
        if let Some(cc) = u.cache_creation_input_tokens {
            println!("  Cache create:        {}", format_tokens(cc as usize));
        }
        if let Some(cr) = u.cache_read_input_tokens {
            let rate = if total_input > 0 {
                cr as f64 / total_input as f64 * 100.0
            } else {
                0.0
            };
            let mark = if rate > 70.0 {
                "✓"
            } else if rate > 0.0 {
                "⚠"
            } else {
                "·"
            };
            println!(
                "  Cache read:          {}   {mark} {:.1}% hit",
                format_tokens(cr as usize),
                rate
            );
        }
        println!("  Output tokens:       {}", format_tokens(u.output_tokens as usize));
    } else {
        println!();
        println!("(No LLM usage captured in this session — run an inference turn to populate.)");
    }

    println!();
    println!(
        "Note: system prompt + tool definitions aren't recorded in session files."
    );
    println!("      Those are rebuilt at agent start; count above is thread history only.");
}

fn print_row(label: &str, count: usize, tokens: usize, total: usize) {
    let pct = if total > 0 {
        tokens as f64 / total as f64 * 100.0
    } else {
        0.0
    };
    let count_str = if count > 0 {
        format!("{count:>4}×")
    } else {
        "      ".to_string()
    };
    println!(
        "  {label:<22} {count_str}  {:>10}  ({:>5.1}%)",
        format_tokens(tokens),
        pct
    );
}

fn format_tokens(n: usize) -> String {
    if n >= 1000 {
        format!("{:.1}K", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}
