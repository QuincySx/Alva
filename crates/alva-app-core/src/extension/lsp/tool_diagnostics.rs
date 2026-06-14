// INPUT:  async_trait, serde, alva_kernel_abi::{Tool, ToolExecutionContext, ToolOutput, AgentError}, super::{LspDiagnostic, LspManager}
// OUTPUT: LspDiagnosticsTool
// POS:    Agent-callable tool surface for LSP diagnostics. Reads `dyn LspManager`
//         from the bus on each call; if no manager is registered (i.e. LspPlugin
//         not installed), returns an empty diagnostics list.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use alva_kernel_abi::{AgentError, Tool, ToolContent, ToolExecutionContext, ToolOutput};

use super::{LspDiagnostic, LspManager};

#[derive(Debug, Deserialize)]
struct Input {
    /// Path to the file to query. Absolute or workspace-relative.
    path: String,
}

#[derive(Debug, Serialize)]
struct Output {
    diagnostics: Vec<LspDiagnostic>,
}

pub struct LspDiagnosticsTool;

impl LspDiagnosticsTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LspDiagnosticsTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for LspDiagnosticsTool {
    fn name(&self) -> &str {
        "lsp_diagnostics"
    }

    fn description(&self) -> &str {
        "Query language-server diagnostics (errors, warnings, hints) for a file. \
         Use after editing source code to see what's red without running a full \
         build. Returns an empty list if no LSP server is available for the file's \
         language."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or workspace-relative file path."
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let parsed: Input = serde_json::from_value(input).map_err(|e| AgentError::ToolError {
            tool_name: "lsp_diagnostics".into(),
            message: format!("invalid input: {e}"),
        })?;

        // Resolve relative to workspace.
        let path = std::path::Path::new(&parsed.path);
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else if let Some(ws) = ctx.workspace() {
            ws.join(path)
        } else {
            path.to_path_buf()
        };

        let diagnostics = match ctx.bus().and_then(|b| b.get::<dyn LspManager>()) {
            Some(mgr) => mgr.diagnostics(&resolved),
            None => Vec::new(),
        };

        let body = Output { diagnostics };
        let json = serde_json::to_string(&body).map_err(|e| AgentError::ToolError {
            tool_name: "lsp_diagnostics".into(),
            message: format!("encode output: {e}"),
        })?;
        Ok(ToolOutput {
            content: vec![ToolContent::text(json)],
            is_error: false,
            details: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension::lsp::{LspSeverity, StubLspManager};
    use alva_kernel_abi::Bus;
    use alva_kernel_abi::CancellationToken;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    struct TestCtx {
        bus: Option<alva_kernel_abi::BusHandle>,
        workspace: PathBuf,
        cancel: CancellationToken,
    }

    impl ToolExecutionContext for TestCtx {
        fn cancel_token(&self) -> &CancellationToken {
            &self.cancel
        }
        fn session_id(&self) -> &str {
            "test"
        }
        fn workspace(&self) -> Option<&Path> {
            Some(&self.workspace)
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn bus(&self) -> Option<&alva_kernel_abi::BusHandle> {
            self.bus.as_ref()
        }
    }

    fn diag() -> LspDiagnostic {
        LspDiagnostic {
            severity: LspSeverity::Warning,
            line: 5,
            col: 0,
            message: "unused import".into(),
            source: "rust-analyzer".into(),
        }
    }

    #[tokio::test]
    async fn returns_empty_when_no_manager_on_bus() {
        let bus = Bus::new();
        let ctx = TestCtx {
            bus: Some(bus.handle()),
            workspace: "/tmp".into(),
            cancel: CancellationToken::new(),
        };
        let tool = LspDiagnosticsTool::new();
        let out = tool
            .execute(serde_json::json!({"path": "src/foo.rs"}), &ctx)
            .await
            .expect("ok");
        assert!(!out.is_error);
        let text = match &out.content[0] {
            ToolContent::Text { text } => text,
            _ => panic!("expected text"),
        };
        let parsed: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["diagnostics"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn returns_seeded_diagnostics() {
        let bus = Bus::new();
        let mgr: Arc<dyn LspManager> = Arc::new(StubLspManager::new());
        mgr.set_diagnostics(Path::new("/tmp/src/foo.rs"), vec![diag()]);
        bus.writer().provide::<dyn LspManager>(mgr);

        let ctx = TestCtx {
            bus: Some(bus.handle()),
            workspace: "/tmp".into(),
            cancel: CancellationToken::new(),
        };
        let tool = LspDiagnosticsTool::new();
        let out = tool
            .execute(serde_json::json!({"path": "src/foo.rs"}), &ctx)
            .await
            .expect("ok");
        let text = match &out.content[0] {
            ToolContent::Text { text } => text,
            _ => panic!("expected text"),
        };
        let parsed: serde_json::Value = serde_json::from_str(text).unwrap();
        let arr = parsed["diagnostics"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["severity"], "warning");
        assert_eq!(arr[0]["line"], 5);
    }

    #[tokio::test]
    async fn rejects_invalid_input_shape() {
        let ctx = TestCtx {
            bus: None,
            workspace: "/tmp".into(),
            cancel: CancellationToken::new(),
        };
        let tool = LspDiagnosticsTool::new();
        let res = tool.execute(serde_json::json!({"oops": 1}), &ctx).await;
        assert!(res.is_err(), "missing path should error");
    }
}
