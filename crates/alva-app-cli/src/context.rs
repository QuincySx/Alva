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
        Some("analytics") => run_analytics().await,
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
    eprintln!("  alva context analytics    Aggregate from .alva/analytics.jsonl");
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
                        ContentBlock::Reasoning { text, .. } => {
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
            ContentBlock::Reasoning { text, .. } => counter.count_tokens(text),
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

// ---------------------------------------------------------------------------
// alva context analytics — aggregate from .alva/analytics.jsonl
// ---------------------------------------------------------------------------

async fn run_analytics() -> i32 {
    let workspace = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let path = workspace.join(".alva").join("analytics.jsonl");
    if !path.exists() {
        eprintln!("No analytics data at {}", path.display());
        eprintln!();
        eprintln!("Telemetry is collected by `AnalyticsExtension` (default-on in the");
        eprintln!("CLI; toggle 'analytics' feature flag in Tauri). Run a session to");
        eprintln!("populate the file, then re-run this command.");
        return 0;
    }
    match analytics::aggregate_jsonl(&path) {
        Ok(report) => {
            analytics::render_report(&report, &workspace);
            0
        }
        Err(e) => {
            eprintln!("Failed to read {}: {}", path.display(), e);
            1
        }
    }
}

mod analytics {
    use std::collections::HashMap;
    use std::fs::File;
    use std::io::{BufRead, BufReader};
    use std::path::Path;

    use alva_kernel_abi::AnalyticsEvent;

    /// Aggregated view across all events in one analytics.jsonl file.
    #[derive(Debug, Default)]
    pub struct Report {
        pub total_events: usize,
        pub bad_lines: usize,
        pub by_model: HashMap<(String, String), ModelStats>,
        pub by_tool: HashMap<String, ToolStats>,
        pub sessions: Vec<SessionStats>,
    }

    #[derive(Debug, Default, Clone)]
    pub struct ModelStats {
        pub calls: u64,
        pub input_tokens: u64,
        pub output_tokens: u64,
        pub cache_read: u64,
        pub cache_write: u64,
        pub cost_usd: f64,
        pub latency_ms_total: u64,
    }

    #[derive(Debug, Default, Clone)]
    pub struct ToolStats {
        pub calls: u64,
        pub ok: u64,
        pub err: u64,
        pub latency_ms_samples: Vec<u64>,
    }

    #[derive(Debug, Default, Clone)]
    pub struct SessionStats {
        pub session_id: String,
        pub duration_ms: u64,
        pub tool_calls: u64,
        pub llm_calls: u64,
        pub cost_usd: f64,
    }

    /// Parse the JSONL file into a Report. Bad lines are counted but
    /// don't abort — telemetry shouldn't break diagnostics.
    pub fn aggregate_jsonl(path: &Path) -> std::io::Result<Report> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut report = Report::default();
        let mut session_index: HashMap<String, usize> = HashMap::new();
        let mut session_order: Vec<String> = Vec::new();

        for line in reader.lines() {
            let line = match line {
                Ok(s) => s,
                Err(_) => {
                    report.bad_lines += 1;
                    continue;
                }
            };
            if line.trim().is_empty() {
                continue;
            }
            let event: AnalyticsEvent = match serde_json::from_str(&line) {
                Ok(e) => e,
                Err(_) => {
                    report.bad_lines += 1;
                    continue;
                }
            };
            report.total_events += 1;
            apply_event(&mut report, &mut session_index, &mut session_order, event);
        }

        Ok(report)
    }

    fn apply_event(
        report: &mut Report,
        session_index: &mut HashMap<String, usize>,
        session_order: &mut Vec<String>,
        ev: AnalyticsEvent,
    ) {
        match ev {
            AnalyticsEvent::SessionStart { session_id, .. } => {
                ensure_session(report, session_index, session_order, &session_id);
            }
            AnalyticsEvent::SessionEnd {
                session_id,
                duration_ms,
                ..
            } => {
                let idx = ensure_session(report, session_index, session_order, &session_id);
                report.sessions[idx].duration_ms = duration_ms;
            }
            AnalyticsEvent::ToolCallStart { .. } => {}
            AnalyticsEvent::ToolCallEnd {
                session_id,
                tool,
                latency_ms,
                ok,
                ..
            } => {
                let idx = ensure_session(report, session_index, session_order, &session_id);
                report.sessions[idx].tool_calls += 1;
                let entry = report.by_tool.entry(tool).or_default();
                entry.calls += 1;
                if ok {
                    entry.ok += 1;
                } else {
                    entry.err += 1;
                }
                entry.latency_ms_samples.push(latency_ms);
            }
            AnalyticsEvent::LlmCall {
                session_id,
                provider,
                model,
                input_tokens,
                output_tokens,
                cache_read,
                cache_write,
                cost_usd,
                latency_ms,
                ..
            } => {
                let idx = ensure_session(report, session_index, session_order, &session_id);
                report.sessions[idx].llm_calls += 1;
                report.sessions[idx].cost_usd += cost_usd;

                let entry = report
                    .by_model
                    .entry((provider, model))
                    .or_default();
                entry.calls += 1;
                entry.input_tokens += input_tokens as u64;
                entry.output_tokens += output_tokens as u64;
                entry.cache_read += cache_read as u64;
                entry.cache_write += cache_write as u64;
                entry.cost_usd += cost_usd;
                entry.latency_ms_total += latency_ms;
            }
        }
    }

    fn ensure_session(
        report: &mut Report,
        session_index: &mut HashMap<String, usize>,
        session_order: &mut Vec<String>,
        session_id: &str,
    ) -> usize {
        if let Some(&idx) = session_index.get(session_id) {
            return idx;
        }
        let idx = report.sessions.len();
        report.sessions.push(SessionStats {
            session_id: session_id.to_string(),
            ..Default::default()
        });
        session_index.insert(session_id.to_string(), idx);
        session_order.push(session_id.to_string());
        idx
    }

    pub fn render_report(report: &Report, workspace: &Path) {
        println!();
        println!("Analytics — {}", workspace.display());
        println!("{}", "─".repeat(72));
        println!(
            "Total events: {}  ({} unparseable lines)",
            report.total_events, report.bad_lines
        );

        // ── Token / cost breakdown by model ─────────────────────────────
        println!();
        println!("Token usage by provider/model:");
        if report.by_model.is_empty() {
            println!("  (no LLM calls recorded)");
        } else {
            println!(
                "  {:<14}  {:<28}  {:>6}  {:>10}  {:>10}  {:>10}  {:>9}",
                "provider", "model", "calls", "in_tok", "out_tok", "cache_rd", "cost_usd"
            );
            let mut entries: Vec<_> = report.by_model.iter().collect();
            // Cost desc (or input tokens if cost is 0)
            entries.sort_by(|a, b| {
                b.1.cost_usd
                    .partial_cmp(&a.1.cost_usd)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| b.1.input_tokens.cmp(&a.1.input_tokens))
            });
            for ((provider, model), s) in entries {
                println!(
                    "  {:<14}  {:<28}  {:>6}  {:>10}  {:>10}  {:>10}  {:>9.4}",
                    truncate(provider, 14),
                    truncate(model, 28),
                    s.calls,
                    s.input_tokens,
                    s.output_tokens,
                    s.cache_read,
                    s.cost_usd
                );
            }
        }

        // ── Top tools by latency (avg + p95) ────────────────────────────
        println!();
        println!("Top 5 tools by avg latency:");
        if report.by_tool.is_empty() {
            println!("  (no tool calls recorded)");
        } else {
            let mut tools: Vec<_> = report
                .by_tool
                .iter()
                .map(|(name, s)| {
                    let avg = if s.calls > 0 {
                        s.latency_ms_samples.iter().sum::<u64>() / s.calls
                    } else {
                        0
                    };
                    let p95 = percentile(&s.latency_ms_samples, 0.95);
                    (name.clone(), s.clone(), avg, p95)
                })
                .collect();
            tools.sort_by(|a, b| b.2.cmp(&a.2));
            println!(
                "  {:<28}  {:>6}  {:>4}/{:<4}  {:>9}  {:>9}",
                "tool", "calls", "ok", "err", "avg_ms", "p95_ms"
            );
            for (name, s, avg, p95) in tools.iter().take(5) {
                println!(
                    "  {:<28}  {:>6}  {:>4}/{:<4}  {:>9}  {:>9}",
                    truncate(name, 28),
                    s.calls,
                    s.ok,
                    s.err,
                    avg,
                    p95
                );
            }
        }

        // ── Recent sessions ─────────────────────────────────────────────
        println!();
        println!("Last 5 sessions:");
        if report.sessions.is_empty() {
            println!("  (none)");
        } else {
            println!(
                "  {:<40}  {:>10}  {:>6}  {:>6}  {:>9}",
                "session_id", "duration", "tools", "llm", "cost_usd"
            );
            let n = report.sessions.len();
            let take_from = n.saturating_sub(5);
            for s in &report.sessions[take_from..n] {
                println!(
                    "  {:<40}  {:>10}  {:>6}  {:>6}  {:>9.4}",
                    truncate(&s.session_id, 40),
                    format_duration_ms(s.duration_ms),
                    s.tool_calls,
                    s.llm_calls,
                    s.cost_usd
                );
            }
        }

        println!();
    }

    fn percentile(samples: &[u64], p: f64) -> u64 {
        if samples.is_empty() {
            return 0;
        }
        let mut sorted: Vec<u64> = samples.to_vec();
        sorted.sort_unstable();
        let idx = ((sorted.len() as f64) * p).ceil() as usize;
        let idx = idx.min(sorted.len()) - 1;
        sorted[idx]
    }

    fn truncate(s: &str, max: usize) -> String {
        if s.chars().count() <= max {
            s.to_string()
        } else {
            let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
            t.push('…');
            t
        }
    }

    fn format_duration_ms(ms: u64) -> String {
        if ms == 0 {
            "-".into()
        } else if ms < 1000 {
            format!("{ms}ms")
        } else if ms < 60_000 {
            format!("{:.1}s", ms as f64 / 1000.0)
        } else {
            format!("{:.1}m", ms as f64 / 60_000.0)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::Write;

        #[test]
        fn aggregates_basic_event_mix() {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("a.jsonl");
            let mut f = File::create(&path).unwrap();
            // SessionStart
            writeln!(f, r#"{{"type":"session_start","session_id":"s1","workspace":"/w","ts":{{"secs_since_epoch":0,"nanos_since_epoch":0}}}}"#).unwrap();
            // ToolCallEnd ok
            writeln!(f, r#"{{"type":"tool_call_end","session_id":"s1","tool":"edit_file","call_id":"c1","latency_ms":42,"ok":true,"ts":{{"secs_since_epoch":0,"nanos_since_epoch":0}}}}"#).unwrap();
            // ToolCallEnd err
            writeln!(f, r#"{{"type":"tool_call_end","session_id":"s1","tool":"edit_file","call_id":"c2","latency_ms":120,"ok":false,"error":"bang","ts":{{"secs_since_epoch":0,"nanos_since_epoch":0}}}}"#).unwrap();
            // LlmCall
            writeln!(f, r#"{{"type":"llm_call","session_id":"s1","provider":"anthropic","model":"claude","input_tokens":1000,"output_tokens":200,"cache_read":800,"cache_write":50,"cost_usd":0.012,"latency_ms":700,"ts":{{"secs_since_epoch":0,"nanos_since_epoch":0}}}}"#).unwrap();
            // SessionEnd
            writeln!(f, r#"{{"type":"session_end","session_id":"s1","duration_ms":5000,"ts":{{"secs_since_epoch":0,"nanos_since_epoch":0}}}}"#).unwrap();
            // junk line
            writeln!(f, "not json at all").unwrap();
            drop(f);

            let r = aggregate_jsonl(&path).expect("aggregate ok");
            assert_eq!(r.total_events, 5, "5 valid events parsed");
            assert_eq!(r.bad_lines, 1);
            // by_model: 1 entry
            let model_stats = r.by_model.get(&("anthropic".into(), "claude".into())).unwrap();
            assert_eq!(model_stats.calls, 1);
            assert_eq!(model_stats.input_tokens, 1000);
            assert_eq!(model_stats.cache_read, 800);
            assert!((model_stats.cost_usd - 0.012).abs() < 1e-9);
            // by_tool: edit_file 2 calls (1 ok / 1 err)
            let tool_stats = r.by_tool.get("edit_file").unwrap();
            assert_eq!(tool_stats.calls, 2);
            assert_eq!(tool_stats.ok, 1);
            assert_eq!(tool_stats.err, 1);
            assert_eq!(tool_stats.latency_ms_samples, vec![42, 120]);
            // sessions: 1 (s1)
            assert_eq!(r.sessions.len(), 1);
            assert_eq!(r.sessions[0].session_id, "s1");
            assert_eq!(r.sessions[0].duration_ms, 5000);
            assert_eq!(r.sessions[0].tool_calls, 2);
            assert_eq!(r.sessions[0].llm_calls, 1);
            assert!((r.sessions[0].cost_usd - 0.012).abs() < 1e-9);
        }

        #[test]
        fn missing_optional_fields_handled() {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("a.jsonl");
            let mut f = File::create(&path).unwrap();
            // LlmCall missing optional fields — must default to 0.
            writeln!(f, r#"{{"type":"llm_call","session_id":"s","provider":"p","model":"m","latency_ms":10,"ts":{{"secs_since_epoch":0,"nanos_since_epoch":0}}}}"#).unwrap();
            drop(f);

            let r = aggregate_jsonl(&path).unwrap();
            let s = r.by_model.get(&("p".into(), "m".into())).unwrap();
            assert_eq!(s.input_tokens, 0);
            assert_eq!(s.output_tokens, 0);
        }

        #[test]
        fn empty_file_returns_empty_report() {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("a.jsonl");
            File::create(&path).unwrap();
            let r = aggregate_jsonl(&path).unwrap();
            assert_eq!(r.total_events, 0);
            assert!(r.by_model.is_empty());
            assert!(r.by_tool.is_empty());
            assert!(r.sessions.is_empty());
        }

        #[test]
        fn percentile_basic() {
            assert_eq!(percentile(&[], 0.95), 0);
            assert_eq!(percentile(&[10, 20, 30, 40, 50], 0.95), 50);
            assert_eq!(percentile(&[10, 20, 30, 40, 50], 0.5), 30);
        }
    }
}
