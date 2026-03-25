use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::router::tool::ToolRouter,
    handler::server::wrapper::Parameters,
    model::{ServerCapabilities, ServerInfo, Implementation},
    tool, tool_handler, tool_router,
    transport::io::stdio,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:9229";

// ---------------------------------------------------------------------------
// Parameter types
// ---------------------------------------------------------------------------

/// Parameters for the `alva_inspect` tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct InspectParams {
    /// The view name to inspect.
    pub view: String,
}

/// Parameters for the `alva_action` tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ActionParams {
    /// Target view or component for the action.
    pub target: String,
    /// Method name to invoke on the target.
    pub method: String,
    /// Arguments to pass to the method.
    pub args: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Server struct
// ---------------------------------------------------------------------------

/// MCP server that proxies tool calls to the alva-app-debug HTTP API.
#[derive(Debug, Clone)]
pub struct AlvaDevtools {
    client: reqwest::Client,
    base_url: String,
    tool_router: ToolRouter<Self>,
}

impl AlvaDevtools {
    pub fn new(base_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url,
            tool_router: Self::tool_router(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

#[tool_router]
impl AlvaDevtools {
    /// List all registered views and their methods.
    #[tool(name = "alva_views", description = "List all registered views and their methods")]
    async fn alva_views(&self) -> Result<String, String> {
        self.client
            .get(format!("{}/api/inspect/views", self.base_url))
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {e}"))?
            .text()
            .await
            .map_err(|e| format!("Failed to read response body: {e}"))
    }

    /// Get the current state of a view.
    #[tool(name = "alva_inspect", description = "Get current state of a view")]
    async fn alva_inspect(&self, Parameters(params): Parameters<InspectParams>) -> Result<String, String> {
        self.client
            .get(format!(
                "{}/api/inspect/state?view={}",
                self.base_url, params.view
            ))
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {e}"))?
            .text()
            .await
            .map_err(|e| format!("Failed to read response body: {e}"))
    }

    /// Execute an action on a view.
    #[tool(name = "alva_action", description = "Execute an action on a view")]
    async fn alva_action(&self, Parameters(params): Parameters<ActionParams>) -> Result<String, String> {
        let body = serde_json::json!({
            "target": params.target,
            "method": params.method,
            "args": params.args,
        });

        self.client
            .post(format!("{}/api/action", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {e}"))?
            .text()
            .await
            .map_err(|e| format!("Failed to read response body: {e}"))
    }

    /// Take a screenshot of the app window.
    #[tool(name = "alva_screenshot", description = "Take a screenshot of the app window")]
    async fn alva_screenshot(&self) -> Result<String, String> {
        self.client
            .post(format!("{}/api/screenshot", self.base_url))
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {e}"))?
            .text()
            .await
            .map_err(|e| format!("Failed to read response body: {e}"))
    }

    /// Gracefully shutdown the app.
    #[tool(name = "alva_shutdown", description = "Gracefully shutdown the app")]
    async fn alva_shutdown(&self) -> Result<String, String> {
        self.client
            .post(format!("{}/api/shutdown", self.base_url))
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {e}"))?
            .text()
            .await
            .map_err(|e| format!("Failed to read response body: {e}"))
    }
}

// ---------------------------------------------------------------------------
// ServerHandler implementation (delegates tool routing to the macro-generated router)
// ---------------------------------------------------------------------------

#[tool_handler(router = self.tool_router)]
impl ServerHandler for AlvaDevtools {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "alva-app-devtools-mcp",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "MCP server for inspecting and controlling a running alva application. \
                 Use alva_views to discover available views, alva_inspect to read state, \
                 alva_action to trigger actions, alva_screenshot to capture the UI, and \
                 alva_shutdown to stop the app.",
            )
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    // Send tracing output to stderr so it doesn't interfere with MCP stdio.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let base_url = std::env::var("ALVA_DEBUG_URL")
        .unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());

    let server = AlvaDevtools::new(base_url);
    let service = server.serve(stdio()).await.expect("failed to start MCP server");

    service.waiting().await.expect("MCP server terminated unexpectedly");
}
