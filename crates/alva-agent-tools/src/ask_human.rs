// INPUT:  alva_types, async_trait, serde, serde_json, std::io
// OUTPUT: AskHumanTool
// POS:    Requests input from the human user via stdin in CLI mode.
//         Supports structured questions with multiple choice options and metadata.
//! ask_human — request input from the user
//!
//! In CLI mode, this reads from stdin.
//! In GUI mode (Tauri), the engine event WaitingForHuman would be used instead.

use alva_types::{AgentError, Tool, ToolContent, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

/// A choice option for structured questions.
#[derive(Debug, Deserialize)]
struct ChoiceOption {
    /// Display label for this option.
    label: String,
    /// Value returned if this option is selected (defaults to label).
    #[serde(default)]
    value: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Input {
    question: String,
    /// Structured choice options (2-4 options).
    #[serde(default)]
    options: Option<Vec<ChoiceOption>>,
    /// Allow multiple selections (only with options).
    #[serde(default)]
    multi_select: Option<bool>,
    /// Header/context text displayed before the question.
    #[serde(default)]
    header: Option<String>,
    /// Arbitrary metadata to pass through.
    #[serde(default)]
    metadata: Option<Value>,
}

pub struct AskHumanTool;

#[async_trait]
impl Tool for AskHumanTool {
    fn name(&self) -> &str {
        "ask_human"
    }

    fn description(&self) -> &str {
        "Ask the human user a question and wait for their response. \
         Supports free-form questions and structured multiple choice (2-4 options). \
         Use this when you need clarification, confirmation, or additional information."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["question"],
            "properties": {
                "question": {
                    "type": "string",
                    "description": "The question to ask the user"
                },
                "options": {
                    "type": "array",
                    "description": "Multiple choice options (2-4 items)",
                    "items": {
                        "type": "object",
                        "required": ["label"],
                        "properties": {
                            "label": {
                                "type": "string",
                                "description": "Display label for this option"
                            },
                            "value": {
                                "type": "string",
                                "description": "Value returned if selected (defaults to label)"
                            }
                        }
                    },
                    "minItems": 2,
                    "maxItems": 4
                },
                "multi_select": {
                    "type": "boolean",
                    "description": "Allow selecting multiple options (only with options, default false)"
                },
                "header": {
                    "type": "string",
                    "description": "Header or context text displayed before the question"
                },
                "metadata": {
                    "type": "object",
                    "description": "Arbitrary metadata to pass through"
                }
            }
        })
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(&self, input: Value, _ctx: &dyn ToolExecutionContext) -> Result<ToolOutput, AgentError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| AgentError::ToolError { tool_name: "ask_human".into(), message: e.to_string() })?;

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
