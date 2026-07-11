// INPUT:  alva_app_core (components assembly), alva_kernel_abi (LanguageModel stub)
// OUTPUT: pub async fn run — `alva tools list [--output-format json]`
// POS:    Tool discovery for orchestrators: assemble the agent's REAL
//         component set (respecting the user's toggles) against a stub
//         model — no API key needed — and list every registered tool.
//         This is what a planning agent queries before deciding an
//         `--allowed-tools` allowlist for a worker invocation.

use std::sync::Arc;

use alva_app_core::{AlvaPaths, BaseAgent};
use alva_kernel_abi::base::error::AgentError;
use alva_kernel_abi::model::CompletionResponse;
use alva_kernel_abi::{LanguageModel, Message, ModelConfig, StreamEvent, Tool};

/// A model that can never be called. Tool discovery only needs the
/// ASSEMBLY (plugins register their tools at build time); no LLM traffic
/// happens, so no credentials are required.
struct DiscoveryStubModel;

#[async_trait::async_trait]
impl LanguageModel for DiscoveryStubModel {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &ModelConfig,
    ) -> Result<CompletionResponse, AgentError> {
        Err(AgentError::Other(
            "tool-discovery stub model cannot be called".into(),
        ))
    }

    fn stream(
        &self,
        _messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &ModelConfig,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = StreamEvent> + Send>> {
        Box::pin(futures::stream::empty())
    }

    fn model_id(&self) -> &str {
        "tool-discovery-stub"
    }
}

pub async fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("list") => list(args).await,
        other => {
            eprintln!(
                "alva tools: unknown subcommand {:?}\nUsage: alva tools list [--output-format <text|json>]",
                other.unwrap_or("<none>")
            );
            1
        }
    }
}

async fn list(args: &[String]) -> i32 {
    let output_json = match args.iter().position(|a| a == "--output-format") {
        Some(i) => match args.get(i + 1).map(String::as_str) {
            Some("json") => true,
            Some("text") | None => false,
            Some(other) => {
                eprintln!("Error: --output-format expects `text` or `json`, got `{other}`");
                return 1;
            }
        },
        None => false,
    };

    let workspace = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let paths = AlvaPaths::new(&workspace);

    // Same assembly switchboard as the real agent (agent_setup::build_agent),
    // honoring the user's component toggles — minus anything that needs
    // credentials (no provider registry; sub-agents degrade gracefully).
    let shared_cfg = alva_app_core::config::load();
    let toggles: alva_app_core::components::ComponentToggles = shared_cfg
        .as_ref()
        .map(|c| c.components.clone())
        .unwrap_or_default();
    let ctx = alva_app_core::components::ComponentContext {
        workspace: workspace.clone(),
        provider_registry: None,
        skills: Some((
            paths.project_skills_dir(),
            crate::agent_setup::bundled_skill_dir(),
        )),
        mcp_config_paths: vec![paths.global_mcp_config(), paths.project_mcp_config()],
        subagent_depth: shared_cfg
            .as_ref()
            .and_then(|c| c.subagent_depth)
            .unwrap_or(alva_app_core::components::DEFAULT_SUBAGENT_DEPTH),
        subagent_timeout: alva_app_core::components::DEFAULT_SUBAGENT_TIMEOUT,
        subagent_tool_timeout: alva_app_core::components::DEFAULT_SUBAGENT_TOOL_TIMEOUT,
        agent_templates: alva_app_core::extension::agent_templates::resolve_agent_templates(&[
            paths.global_agents_config(),
            paths.project_agents_config(),
        ]),
        hooks_settings: alva_app_core::settings::HooksSettings::default(),
        subprocess_ext_dirs: vec![
            paths.project_extensions_dir(),
            paths.global_extensions_dir(),
        ],
    };
    let builder = alva_app_core::components::apply_components(
        BaseAgent::builder()
            .workspace(&workspace)
            .system_prompt("tool discovery"),
        &toggles,
        &ctx,
    );
    let agent = match builder.build(Arc::new(DiscoveryStubModel)).await {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Error: failed to assemble the agent for discovery: {e}");
            return 1;
        }
    };

    let mut tools = agent.tool_summaries();
    tools.sort_by(|a, b| a.0.cmp(&b.0));

    if output_json {
        let arr: Vec<serde_json::Value> = tools
            .iter()
            .map(|(name, description)| {
                serde_json::json!({ "name": name, "description": description })
            })
            .collect();
        println!("{}", serde_json::json!(arr));
    } else {
        for (name, description) in &tools {
            // First line of the description only — keep the table scannable.
            let first_line = description.lines().next().unwrap_or("");
            println!("{name:<24} {first_line}");
        }
        eprintln!("\n{} tools (honoring your component toggles)", tools.len());
    }
    0
}
