// INPUT:  alva_kernel_abi, async_trait, schemars, serde, serde_json, std::io
// OUTPUT: AskHumanTool
// POS:    Requests input from the human user via stdin in CLI mode.
//         Supports structured questions with multiple choice options and metadata.
//! ask_human — request input from the user
//!
//! In CLI mode, this reads from stdin.
//! In GUI mode (Tauri), the engine event WaitingForHuman would be used instead.

use alva_kernel_abi::{AgentError, Tool, ToolContent, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

/// A choice option for structured questions.
#[derive(Debug, Deserialize, JsonSchema)]
struct ChoiceOption {
    /// Display label for this option.
    label: String,
    /// Value returned if this option is selected (defaults to label).
    #[serde(default)]
    value: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// The question to ask the user.
    question: String,
    /// Multiple choice options (2-4 items).
    #[serde(default)]
    options: Option<Vec<ChoiceOption>>,
    /// Allow selecting multiple options (only with options, default false).
    #[serde(default)]
    multi_select: Option<bool>,
    /// Header or context text displayed before the question.
    #[serde(default)]
    header: Option<String>,
    /// Arbitrary metadata to pass through.
    #[serde(default)]
    metadata: Option<Value>,
}

#[derive(Tool)]
#[tool(
    name = "ask_human",
    description = "Ask the human user a question and wait for their response. \
        Supports free-form questions and structured multiple choice (2-4 options). \
        Use this when you need clarification, confirmation, or additional information.",
    input = Input,
    read_only,
)]
pub struct AskHumanTool;

impl AskHumanTool {
    async fn execute_impl(
        &self,
        params: Input,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let multi_select = params.multi_select.unwrap_or(false);

        // Display header if provided
        if let Some(ref header) = params.header {
            eprintln!("\n{}", header);
            eprintln!("---");
        }

        // In CLI mode: print question and read from stdin
        eprintln!("\n[ask_human] {}", params.question);

        // Display options if provided
        if let Some(ref options) = params.options {
            if options.len() < 2 || options.len() > 4 {
                return Ok(ToolOutput::error(format!(
                    "options must have 2-4 items, got {}",
                    options.len()
                )));
            }

            for (i, opt) in options.iter().enumerate() {
                eprintln!("  {}) {}", i + 1, opt.label);
            }

            if multi_select {
                eprintln!("(Enter numbers separated by commas, e.g., 1,3)");
            } else {
                eprintln!("(Enter a number)");
            }
        }

        eprint!("> ");

        let answer = tokio::task::spawn_blocking(|| {
            let mut buf = String::new();
            std::io::stdin()
                .read_line(&mut buf)
                .map(|_| buf.trim().to_string())
        })
        .await
        .map_err(|e| AgentError::ToolError { tool_name: "ask_human".into(), message: e.to_string() })?
        .map_err(|e| AgentError::ToolError { tool_name: "ask_human".into(), message: e.to_string() })?;

        // Process answer through options if provided
        let result = if let Some(ref options) = params.options {
            parse_choice_answer(&answer, options, multi_select)
        } else {
            answer
        };

        let mut details = json!({ "raw_answer": &result });
        if let Some(ref metadata) = params.metadata {
            details["metadata"] = metadata.clone();
        }
        if params.options.is_some() {
            details["structured"] = json!(true);
        }

        Ok(ToolOutput {
            content: vec![ToolContent::text(&result)],
            is_error: false,
            details: Some(details),
        })
    }
}

/// Map a raw user answer to a structured choice value.
///
/// Single-select: parse `answer` as a 1-based index into `options`. On
/// success return the option's `value` (falling back to `label` when
/// `value` is None). On parse failure or out-of-range, return the raw
/// answer unchanged.
///
/// Multi-select: split by comma, parse each token as a 1-based index,
/// map valid ones to `value || label`, then join with ", ". If no token
/// resolves, fall back to the raw answer.
///
/// Extracted from `execute_impl` so the choice mapping logic can be
/// tested without stdin.
fn parse_choice_answer(answer: &str, options: &[ChoiceOption], multi_select: bool) -> String {
    if multi_select {
        let selected: Vec<String> = answer
            .split(',')
            .filter_map(|s| {
                let idx: usize = s.trim().parse().ok()?;
                if idx >= 1 && idx <= options.len() {
                    let opt = &options[idx - 1];
                    Some(opt.value.clone().unwrap_or_else(|| opt.label.clone()))
                } else {
                    None
                }
            })
            .collect();

        if selected.is_empty() {
            answer.to_string()
        } else {
            selected.join(", ")
        }
    } else {
        if let Ok(idx) = answer.parse::<usize>() {
            if idx >= 1 && idx <= options.len() {
                let opt = &options[idx - 1];
                opt.value.clone().unwrap_or_else(|| opt.label.clone())
            } else {
                answer.to_string()
            }
        } else {
            answer.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;

    use super::*;
    use alva_kernel_abi::{CancellationToken, Tool};

    struct TestContext {
        cancel: CancellationToken,
    }

    impl ToolExecutionContext for TestContext {
        fn cancel_token(&self) -> &CancellationToken {
            &self.cancel
        }
        fn session_id(&self) -> &str {
            "test-session"
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    fn ctx() -> TestContext {
        TestContext {
            cancel: CancellationToken::new(),
        }
    }

    fn opt(label: &str, value: Option<&str>) -> ChoiceOption {
        ChoiceOption {
            label: label.into(),
            value: value.map(String::from),
        }
    }

    // -- ToolOutput::error path (reachable WITHOUT stdin) -------------------

    #[tokio::test]
    async fn options_length_outside_2_to_4_is_tool_output_error() {
        let tool = AskHumanTool;

        // 1 option (below min) — should reject BEFORE reading stdin
        let out = tool
            .execute(
                json!({
                    "question": "Pick one",
                    "options": [{ "label": "only" }],
                }),
                &ctx(),
            )
            .await
            .expect("execute returns Ok(error output), not Err");
        assert!(out.is_error, "1 option should be rejected");
        assert!(out.model_text().contains("2-4"), "expected '2-4' constraint msg: {}", out.model_text());

        // 5 options (above max) — also reject before stdin
        let out2 = tool
            .execute(
                json!({
                    "question": "Pick one",
                    "options": [
                        { "label": "a" }, { "label": "b" }, { "label": "c" },
                        { "label": "d" }, { "label": "e" },
                    ],
                }),
                &ctx(),
            )
            .await
            .expect("execute returns Ok(error output), not Err");
        assert!(out2.is_error, "5 options should be rejected");
    }

    // -- Schema-level errors ------------------------------------------------

    #[tokio::test]
    async fn missing_question_field_returns_input_error() {
        let tool = AskHumanTool;
        let err = tool
            .execute(json!({}), &ctx())
            .await
            .expect_err("missing required `question` should error");

        let msg = format!("{err}");
        assert!(
            msg.contains("invalid input") || msg.contains("question"),
            "expected invalid-input error mentioning `question`, got: {msg}"
        );
    }

    // -- Pure-function: parse_choice_answer ---------------------------------

    #[test]
    fn parse_single_select_valid_index_returns_value_preferring_label() {
        let opts = vec![
            opt("Yes", Some("y")),
            opt("No", None), // no `value` → fallback to label
        ];
        // index 1 → opt[0] which has `value` "y"
        assert_eq!(parse_choice_answer("1", &opts, false), "y");
        // index 2 → opt[1] which has no value → label "No"
        assert_eq!(parse_choice_answer("2", &opts, false), "No");
    }

    #[test]
    fn parse_single_select_invalid_or_out_of_range_falls_back_to_raw() {
        let opts = vec![opt("A", None), opt("B", None)];
        // Out of range → raw answer
        assert_eq!(parse_choice_answer("3", &opts, false), "3");
        assert_eq!(parse_choice_answer("0", &opts, false), "0");
        // Non-numeric → raw answer (user typed free-form text)
        assert_eq!(parse_choice_answer("dunno", &opts, false), "dunno");
    }

    #[test]
    fn parse_multi_select_mixed_valid_joins_with_comma() {
        let opts = vec![
            opt("Red", Some("r")),
            opt("Green", Some("g")),
            opt("Blue", None), // value missing → label "Blue"
        ];
        // 1,3 → "r, Blue"; whitespace tolerated; invalid 9 silently dropped
        assert_eq!(parse_choice_answer("1,3,9", &opts, true), "r, Blue");
        assert_eq!(parse_choice_answer(" 2 , 1 ", &opts, true), "g, r");
    }

    #[test]
    fn parse_multi_select_all_invalid_falls_back_to_raw() {
        let opts = vec![opt("A", None), opt("B", None)];
        // No token resolves to a valid index → return raw answer
        assert_eq!(parse_choice_answer("none of them", &opts, true), "none of them");
        assert_eq!(parse_choice_answer("9,10", &opts, true), "9,10");
    }

    #[test]
    fn is_read_only_is_always_true() {
        let tool = AskHumanTool;
        // ask_human is declared `read_only` via macro attr — verify
        assert!(tool.is_read_only(&json!({ "question": "hi" })));
        assert!(tool.is_read_only(&json!({ "question": "x", "options": [
            { "label": "a" }, { "label": "b" }
        ] })));
    }
}
