// INPUT:  alva_types, async_trait, schemars, serde, serde_json, std::io
// OUTPUT: AskHumanTool
// POS:    Requests input from the human user via stdin in CLI mode.
//         Supports structured questions with multiple choice options and metadata.
//! ask_human — request input from the user
//!
//! In CLI mode, this reads from stdin.
//! In GUI mode (Tauri), the engine event WaitingForHuman would be used instead.

use alva_types::{AgentError, Tool, ToolContent, ToolExecutionContext, ToolOutput};
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
            if multi_select {
                // Parse comma-separated numbers
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
                    // Fall back to raw answer
                    answer
                } else {
                    selected.join(", ")
                }
            } else {
                // Single selection
                if let Ok(idx) = answer.parse::<usize>() {
                    if idx >= 1 && idx <= options.len() {
                        let opt = &options[idx - 1];
                        opt.value.clone().unwrap_or_else(|| opt.label.clone())
                    } else {
                        answer
                    }
                } else {
                    answer
                }
            }
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
