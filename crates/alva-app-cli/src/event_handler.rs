// INPUT:  alva_app_core, alva_agent_runtime, alva_types, output
// OUTPUT: run_print_mode, run_prompt, handle_approval
// POS:    AgentEvent processing — streaming output, tool execution display, and interactive approval handling

use std::io::{self, BufRead, Write};

use alva_app_core::{AgentEvent, AgentMessage, BaseAgent, PermissionDecision};
use alva_agent_runtime::middleware::security::ApprovalRequest;
use tokio::sync::mpsc;

use crate::output;

/// Run a single prompt in non-interactive print mode.
/// Streams only the assistant's text to stdout, then exits.
pub(crate) async fn run_print_mode(agent: &BaseAgent, prompt: &str) -> i32 {
    let mut rx = agent.prompt_text(prompt);
    let mut exit_code = 0;

    while let Some(event) = rx.recv().await {
        match event {
            AgentEvent::MessageUpdate { delta, .. } => {
                if let alva_types::StreamEvent::TextDelta { text } = &delta {
                    print!("{}", text);
                    io::stdout().flush().ok();
                }
            }
            AgentEvent::MessageEnd { .. } => {
                println!(); // final newline
            }
            AgentEvent::AgentEnd { error } => {
                if let Some(e) = &error {
                    eprintln!("Error: {}", e);
                    exit_code = 1;
                }
                break;
            }
            _ => {}
        }
    }

    exit_code
}

/// Run a single prompt, handling both agent events and approval requests concurrently.
/// Returns (input_tokens, output_tokens) accumulated during this prompt.
pub(crate) async fn run_prompt(
    agent: &BaseAgent,
    prompt: &str,
    approval_rx: &mut mpsc::UnboundedReceiver<ApprovalRequest>,
) -> (u64, u64) {
    let mut event_rx = agent.prompt_text(prompt);

    let mut total_input_tokens: u64 = 0;
    let mut total_output_tokens: u64 = 0;

    loop {
        tokio::select! {
            event = event_rx.recv() => {
                match event {
                    Some(AgentEvent::MessageStart { .. }) => {}
                    Some(AgentEvent::MessageUpdate { delta, .. }) => {
                        if let alva_types::StreamEvent::TextDelta { text } = &delta {
                            output::print_assistant_text(text);
                        }
                    }
                    Some(AgentEvent::MessageEnd { message }) => {
                        println!();
                        if let AgentMessage::Standard(msg) = &message {
                            if let Some(usage) = &msg.usage {
                                total_input_tokens += usage.input_tokens as u64;
                                total_output_tokens += usage.output_tokens as u64;
                            }
                        }
                    }
                    Some(AgentEvent::ToolExecutionStart { tool_call }) => {
                        output::print_tool_start(&tool_call.name);
                    }
                    Some(AgentEvent::ToolExecutionEnd { tool_call, result }) => {
                        output::print_tool_end(&tool_call.name, result.is_error, &result.model_text());
                    }
                    Some(AgentEvent::AgentEnd { error }) => {
                        if let Some(e) = error {
                            output::print_error(&e);
                        }
                        if total_input_tokens > 0 || total_output_tokens > 0 {
                            output::print_usage(total_input_tokens, total_output_tokens);
                        }
                        break;
                    }
                    Some(_) => {}
                    None => break,
                }
            }
            approval = approval_rx.recv() => {
                if let Some(req) = approval {
                    handle_approval(agent, req).await;
                }
            }
        }
    }

    (total_input_tokens, total_output_tokens)
}

/// Handle a single approval request: prompt the user and resolve the permission.
pub(crate) async fn handle_approval(agent: &BaseAgent, req: ApprovalRequest) {
    output::print_approval_prompt(&req.tool_name, &req.arguments);

    let mut input = String::new();
    let _ = io::stdin().lock().read_line(&mut input);

    let decision = match input.trim().to_lowercase().as_str() {
        "y" | "yes" | "" => PermissionDecision::AllowOnce,
        "a" | "always" => PermissionDecision::AllowAlways,
        "d" | "deny" => PermissionDecision::RejectAlways,
        _ => PermissionDecision::RejectOnce,
    };

    agent
        .resolve_permission(&req.request_id, &req.tool_name, decision)
        .await;
}
