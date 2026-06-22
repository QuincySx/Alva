// INPUT:  alva_app_core, alva_host_native, alva_kernel_abi, output
// OUTPUT: run_print_mode, run_prompt, handle_approval
// POS:    AgentEvent processing — streaming output, tool execution display, and interactive approval handling

use std::io::{self, BufRead, Write};

use alva_app_core::{AgentEvent, AgentMessage, BaseAgent, PermissionDecision};
use alva_host_native::middleware::ApprovalRequest;
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
                if let alva_kernel_abi::StreamEvent::TextDelta { text } = &delta {
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
                        if let alva_kernel_abi::StreamEvent::TextDelta { text } = &delta {
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

    let decision = parse_approval_input(&input);

    agent
        .resolve_permission(&req.request_id, &req.tool_name, decision)
        .await;
}

/// Map a raw approval-prompt response into a `PermissionDecision`.
///
/// Extracted from `handle_approval` so the (security-critical)
/// "user text → permission" mapping is unit-testable without going
/// through stdin / a BaseAgent. Behavior:
///
/// - "y" / "yes" / "" (empty)        → AllowOnce  (default accept)
/// - "a" / "always"                  → AllowAlways
/// - "d" / "deny"                    → RejectAlways
/// - anything else                   → RejectOnce
///
/// Input is trimmed of surrounding whitespace and lowercased before
/// matching, so "Y\n", "  YES  ", "Always" all behave identically
/// to the lowercase forms above.
pub(crate) fn parse_approval_input(input: &str) -> PermissionDecision {
    match input.trim().to_lowercase().as_str() {
        "y" | "yes" | "" => PermissionDecision::AllowOnce,
        "a" | "always" => PermissionDecision::AllowAlways,
        "d" | "deny" => PermissionDecision::RejectAlways,
        _ => PermissionDecision::RejectOnce,
    }
}

#[cfg(test)]
mod tests {
    //! Tests for parse_approval_input — the HITL permission text-to-
    //! decision mapping. Misrouting here flips user intent: a user
    //! typing "deny" might get AllowOnce, which is a security
    //! regression with no compile-time signal. All variants + the
    //! trim + lowercase normalization are pinned.
    use super::*;

    #[test]
    fn lowercase_y_is_allow_once() {
        assert_eq!(parse_approval_input("y"), PermissionDecision::AllowOnce);
    }

    #[test]
    fn lowercase_yes_is_allow_once() {
        assert_eq!(parse_approval_input("yes"), PermissionDecision::AllowOnce);
    }

    #[test]
    fn empty_string_is_allow_once_default() {
        // "Just press Enter" is the default-accept affordance — pin
        // that the empty string maps to AllowOnce, NOT RejectOnce.
        assert_eq!(parse_approval_input(""), PermissionDecision::AllowOnce);
    }

    #[test]
    fn lowercase_a_is_allow_always() {
        assert_eq!(parse_approval_input("a"), PermissionDecision::AllowAlways);
    }

    #[test]
    fn lowercase_always_is_allow_always() {
        assert_eq!(
            parse_approval_input("always"),
            PermissionDecision::AllowAlways
        );
    }

    #[test]
    fn lowercase_d_is_reject_always() {
        assert_eq!(parse_approval_input("d"), PermissionDecision::RejectAlways);
    }

    #[test]
    fn lowercase_deny_is_reject_always() {
        assert_eq!(
            parse_approval_input("deny"),
            PermissionDecision::RejectAlways
        );
    }

    #[test]
    fn unknown_input_is_reject_once_not_allow() {
        // Pin: anything outside the explicit set defaults to REJECT
        // (RejectOnce), not Allow. This is the safe-by-default
        // direction — a typo should NOT grant access.
        assert_eq!(
            parse_approval_input("maybe"),
            PermissionDecision::RejectOnce
        );
        assert_eq!(parse_approval_input("?"), PermissionDecision::RejectOnce);
        assert_eq!(parse_approval_input("nope"), PermissionDecision::RejectOnce);
    }

    #[test]
    fn input_is_trimmed_before_matching() {
        // Trailing newlines come from read_line; leading whitespace
        // can arrive via terminal noise. Both must normalize away.
        assert_eq!(parse_approval_input("y\n"), PermissionDecision::AllowOnce);
        assert_eq!(
            parse_approval_input("  yes  "),
            PermissionDecision::AllowOnce
        );
        assert_eq!(
            parse_approval_input("\tdeny\n"),
            PermissionDecision::RejectAlways
        );
    }

    #[test]
    fn matching_is_case_insensitive() {
        assert_eq!(parse_approval_input("Y"), PermissionDecision::AllowOnce);
        assert_eq!(parse_approval_input("YES"), PermissionDecision::AllowOnce);
        assert_eq!(
            parse_approval_input("Always"),
            PermissionDecision::AllowAlways
        );
        assert_eq!(
            parse_approval_input("DENY"),
            PermissionDecision::RejectAlways
        );
    }
}
