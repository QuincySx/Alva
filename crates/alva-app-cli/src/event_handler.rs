// INPUT:  alva_app_core, alva_host_native, alva_kernel_abi, output
// OUTPUT: run_print_mode, run_prompt, handle_approval
// POS:    AgentEvent processing — streaming output, tool execution display, and interactive approval handling

use std::io::{self, BufRead, Write};

use alva_app_core::{AgentEvent, AgentMessage, BaseAgent, PermissionDecision};
use alva_host_native::middleware::ApprovalRequest;
use tokio::sync::mpsc;

use crate::output;

/// Format a terminal agent error for non-interactive (`-p`) mode.
///
/// In headless mode the only consumer of an error is another program (often an
/// AI). A bare "Error: X" is a dead end — so we always echo the raw error AND
/// append a targeted next step (*why* + *how to fix*) when the text matches a
/// known failure class, falling back to a generic-but-actionable hint.
pub(crate) fn format_print_mode_error(raw: &str) -> String {
    let lower = raw.to_lowercase();
    let hint = if lower.contains("401")
        || lower.contains("unauthorized")
        || lower.contains("api key")
        || lower.contains("api_key")
        || lower.contains("apikey")
        || lower.contains("authentication")
        || lower.contains("403")
        || lower.contains("forbidden")
    {
        "Looks like an authentication problem. Check your provider credentials: \
         set ALVA_API_KEY (and ALVA_MODEL / ALVA_BASE_URL / ALVA_PROVIDER_KIND) or run \
         `alva settings set <provider> --api-key ... --model ...`."
    } else if lower.contains("429")
        || lower.contains("rate limit")
        || lower.contains("rate_limit")
        || lower.contains("quota")
        || lower.contains("overloaded")
    {
        "The provider rate-limited or is overloaded. Wait a few seconds and retry, \
         reduce concurrency, or switch model/provider via `alva settings`."
    } else if lower.contains("denied")
        || lower.contains("approval")
        || lower.contains("permission")
        || lower.contains("not allowed")
        || lower.contains("blocked")
    {
        "A tool was blocked by the permission policy. In -p mode tools needing approval \
         are denied by default. Re-run with `--permission-mode accept-shell` (safe shell \
         auto-approved) or `--permission-mode bypass` (allow everything; sandbox only). \
         See `alva --help`."
    } else if lower.contains("timeout") || lower.contains("timed out") || lower.contains("deadline")
    {
        "The request or a tool timed out. Check network/provider reachability (ALVA_BASE_URL), \
         retry, or the tool may be hanging — re-run with RUST_LOG=debug to see where."
    } else if lower.contains("connect")
        || lower.contains("dns")
        || lower.contains("network")
        || lower.contains("transport")
    {
        "Network/transport error reaching the provider. Verify ALVA_BASE_URL and connectivity, \
         then retry."
    } else {
        "Re-run with RUST_LOG=debug for a full trace, or `alva --help` for options. If a tool \
         was blocked, try `--permission-mode accept-shell`."
    };
    format!("Error: {raw}\nHint: {hint}")
}

/// User-facing notice when a tool requires approval during non-interactive
/// (`-p`) mode. There is no human to answer, so the call is denied; the
/// message names the tool and points at `--permission-mode` to opt in.
pub(crate) fn print_mode_denial_notice(tool_name: &str) -> String {
    format!(
        "[permission] '{tool_name}' needs approval but -p mode is non-interactive — denied. \
         Re-run with --permission-mode accept-shell (or bypass) to allow it."
    )
}

/// Run a single prompt in non-interactive print mode.
/// Streams only the assistant's text to stdout, then exits.
///
/// Approval requests are drained and **fail-closed denied** (with a stderr
/// notice) so a tool that needs a human prompt cannot hang the run until the
/// 300s approval timeout. Use `--permission-mode accept-shell|bypass` to let
/// such tools run without a prompt.
pub(crate) async fn run_print_mode(
    agent: &BaseAgent,
    prompt: &str,
    approval_rx: &mut mpsc::UnboundedReceiver<ApprovalRequest>,
) -> i32 {
    run_print_mode_with(
        agent,
        prompt,
        approval_rx,
        &mut io::stdout(),
        &mut io::stderr(),
    )
    .await
}

/// Writer-injected core of [`run_print_mode`] — the seam that makes the
/// headless main loop testable in-process. Production passes stdout/stderr;
/// tests pass `Vec<u8>` buffers and assert the exact byte contract.
pub(crate) async fn run_print_mode_with(
    agent: &BaseAgent,
    prompt: &str,
    approval_rx: &mut mpsc::UnboundedReceiver<ApprovalRequest>,
    out: &mut (dyn Write + Send),
    err: &mut (dyn Write + Send),
) -> i32 {
    let mut rx = agent.prompt_text(prompt);
    let mut exit_code = 0;

    loop {
        tokio::select! {
            event = rx.recv() => {
                match event {
                    Some(AgentEvent::MessageUpdate { delta, .. }) => {
                        if let alva_kernel_abi::StreamEvent::TextDelta { text } = &delta {
                            write!(out, "{}", text).ok();
                            out.flush().ok();
                        }
                    }
                    Some(AgentEvent::MessageEnd { .. }) => {
                        writeln!(out).ok(); // final newline
                    }
                    Some(AgentEvent::AgentEnd { error }) => {
                        if let Some(e) = &error {
                            writeln!(err, "{}", format_print_mode_error(e)).ok();
                            exit_code = 1;
                        }
                        break;
                    }
                    Some(_) => {}
                    None => break,
                }
            }
            approval = approval_rx.recv() => {
                if let Some(req) = approval {
                    writeln!(err, "{}", print_mode_denial_notice(&req.tool_name)).ok();
                    agent
                        .resolve_permission(
                            &req.request_id,
                            &req.tool_name,
                            PermissionDecision::RejectOnce,
                        )
                        .await;
                }
            }
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

    // -- run_print_mode_with: in-process contract tests ---------------------
    //
    // These drive the REAL headless loop against a mock model and assert the
    // stdout/stderr byte contract — the CLI's -p mode promise: stdout carries
    // ONLY the assistant text (+ trailing newline), everything else (errors,
    // permission notices) goes to stderr.

    async fn print_mode_agent(
        model: alva_test::mock_provider::MockLanguageModel,
    ) -> (
        alva_app_core::BaseAgent,
        mpsc::UnboundedReceiver<ApprovalRequest>,
        tempfile::TempDir,
    ) {
        let (approval_ext, approval_rx) = alva_app_core::extension::ApprovalPlugin::with_channel();
        let tmp = tempfile::tempdir().expect("tempdir");
        let agent = alva_app_core::BaseAgent::builder()
            .workspace(tmp.path())
            .system_prompt("test")
            .plugin(Box::new(alva_app_core::extension::ShellPlugin))
            .plugin(Box::new(approval_ext))
            .build(std::sync::Arc::new(model))
            .await
            .expect("agent builds");
        (agent, approval_rx, tmp)
    }

    #[tokio::test]
    async fn print_mode_stdout_is_text_only_and_exit_zero() {
        use alva_test::fixtures::make_assistant_message;
        let model = alva_test::mock_provider::MockLanguageModel::new()
            .with_response(make_assistant_message("hello from -p"));
        let (agent, mut approval_rx, _tmp) = print_mode_agent(model).await;

        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run_print_mode_with(&agent, "hi", &mut approval_rx, &mut out, &mut err).await;

        assert_eq!(code, 0);
        assert_eq!(String::from_utf8(out).unwrap(), "hello from -p\n");
        assert!(
            err.is_empty(),
            "clean run must write nothing to stderr: {}",
            String::from_utf8_lossy(&err)
        );
    }

    #[tokio::test]
    async fn print_mode_error_goes_to_stderr_with_hint_and_exit_one() {
        // Empty response queue → the mock model errors → AgentEnd { error }.
        let model = alva_test::mock_provider::MockLanguageModel::new();
        let (agent, mut approval_rx, _tmp) = print_mode_agent(model).await;

        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run_print_mode_with(&agent, "hi", &mut approval_rx, &mut out, &mut err).await;

        assert_eq!(code, 1, "AgentEnd with error must exit 1");
        let err = String::from_utf8(err).unwrap();
        assert!(err.contains("Error:"), "raw error must be echoed: {err}");
        assert!(
            err.contains("Hint:"),
            "error must carry an actionable hint: {err}"
        );
    }

    #[tokio::test]
    async fn print_mode_denies_approvals_fail_closed_and_finishes() {
        use alva_test::fixtures::{make_assistant_message, tool_use_message};
        // Model asks for a shell command (HITL-gated); -p mode must deny it
        // on stderr WITHOUT hanging, feed the rejection back, and let the
        // model finish its scripted turn.
        let model = alva_test::mock_provider::MockLanguageModel::new()
            .with_response(tool_use_message(
                "sh",
                "execute_shell",
                serde_json::json!({ "command": "echo hi" }),
            ))
            .with_response(make_assistant_message("done without shell"));
        let (agent, mut approval_rx, _tmp) = print_mode_agent(model).await;

        let (mut out, mut err) = (Vec::new(), Vec::new());
        let run = run_print_mode_with(
            &agent,
            "run a command",
            &mut approval_rx,
            &mut out,
            &mut err,
        );
        let code = tokio::time::timeout(std::time::Duration::from_secs(10), run)
            .await
            .expect("fail-closed denial must not hang the run");

        assert_eq!(code, 0, "denied tool is not a run failure");
        let err = String::from_utf8(err).unwrap();
        assert!(
            err.contains("[permission]") && err.contains("execute_shell"),
            "stderr must carry the denial notice: {err}"
        );
        let out = String::from_utf8(out).unwrap();
        assert!(
            out.contains("done without shell"),
            "the run must continue to the final message: {out}"
        );
        assert!(
            !out.contains("[permission]"),
            "stdout must stay clean of permission noise: {out}"
        );
    }

    #[test]
    fn format_print_mode_error_always_includes_raw() {
        let out = format_print_mode_error("some weird provider blew up");
        assert!(
            out.contains("some weird provider blew up"),
            "raw error preserved: {out}"
        );
    }

    #[test]
    fn format_print_mode_error_auth_points_at_credentials() {
        for raw in [
            "HTTP 401 Unauthorized",
            "invalid api key",
            "authentication failed",
        ] {
            let out = format_print_mode_error(raw);
            assert!(
                out.contains("ALVA_API_KEY") || out.contains("alva settings set"),
                "auth error should point at credentials, got: {out}"
            );
        }
    }

    #[test]
    fn format_print_mode_error_rate_limit_says_retry() {
        let out = format_print_mode_error("HTTP 429 rate limit exceeded");
        assert!(
            out.to_lowercase().contains("retry") || out.to_lowercase().contains("rate"),
            "rate-limit hint: {out}"
        );
    }

    #[test]
    fn format_print_mode_error_permission_points_at_flag() {
        let out = format_print_mode_error("tool 'execute_shell' denied by user");
        assert!(
            out.contains("--permission-mode"),
            "permission error should point at the flag: {out}"
        );
    }

    #[test]
    fn format_print_mode_error_unknown_still_actionable() {
        // Even an unrecognized error must give the AI a next step, not a dead end.
        let out = format_print_mode_error("kaboom");
        assert!(out.len() > "kaboom".len(), "appends guidance: {out}");
        assert!(
            out.contains("RUST_LOG") || out.contains("--help"),
            "generic next step present: {out}"
        );
    }

    #[test]
    fn print_mode_denial_notice_names_tool_and_hint() {
        // The notice the user sees when a tool needs approval in non-interactive
        // `-p` mode. Must name the offending tool and point at the escape hatch
        // (--permission-mode) so a headless run is self-explanatory, not a
        // silent denial.
        let notice = print_mode_denial_notice("execute_shell");
        assert!(notice.contains("execute_shell"), "names the tool: {notice}");
        assert!(
            notice.contains("--permission-mode"),
            "points at the flag: {notice}"
        );
    }

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
