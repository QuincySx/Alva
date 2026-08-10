// INPUT:  provider/model/api-key selection + user prompt
// OUTPUT: BaseAgent construction and streamed prompt execution
// POS:    Migrated from alva-app-tauri/src/agent/run.rs (no tauri): the
//         same BaseAgent assembly (plugin toggles → apply_components →
//         checkpoint callback) and the same provider-config resolution.

use std::collections::HashMap;
use std::sync::Arc;

use alva_app_core::extension::ApprovalPlugin;
use alva_app_core::{AlvaPaths, BaseAgent};
use alva_llm_provider::{build_provider_registry, ProviderConfig};

/// What the user chose for one send: provider, model, optional key.
#[derive(Debug, Clone)]
pub struct SendRequest {
    pub provider: String,
    pub model: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub system_prompt: Option<String>,
    pub workspace: Option<String>,
    pub reasoning_effort: Option<String>,
    /// Manual tool allow-list; None = auto mode (all agent tools).
    pub tool_names: Option<Vec<String>>,
}

pub struct AgentService {
    pub agent: Option<Arc<BaseAgent>>,
    agent_key: Option<String>,
}

fn default_plugin_state() -> HashMap<String, bool> {
    let mut m: HashMap<String, bool> = alva_app_core::components::COMPONENTS
        .iter()
        .map(|c| (c.id.to_string(), c.default_on))
        .collect();
    m.insert("approval".into(), true);
    m
}

fn plugin_config_hash(plugins: &HashMap<String, bool>) -> String {
    let mut sorted: Vec<(String, bool)> = plugins.iter().map(|(k, v)| (k.clone(), *v)).collect();
    sorted.sort();
    let mut s = String::new();
    for (k, v) in sorted {
        s.push_str(&format!("{k}={v};"));
    }
    s
}

pub fn build_provider_config(req: &SendRequest) -> Result<ProviderConfig, String> {
    let provider_env_key = match req.provider.as_str() {
        "anthropic" => "ANTHROPIC_API_KEY",
        "openai-responses" | "openai-chat" => "OPENAI_API_KEY",
        "gemini" => "GEMINI_API_KEY",
        _ => "OPENAI_API_KEY",
    };
    let file_provider =
        alva_app_core::config::load().and_then(|cfg| cfg.providers.get(&req.provider).cloned());
    let api_key = req
        .api_key
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var(provider_env_key)
                .ok()
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            if req.provider == "gemini" {
                std::env::var("GOOGLE_API_KEY")
                    .ok()
                    .filter(|s| !s.is_empty())
            } else {
                None
            }
        })
        .or_else(|| {
            file_provider
                .as_ref()
                .map(|e| e.api_key.clone())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_default();
    if api_key.is_empty() {
        return Err(format!(
            "missing api_key for provider '{}'. Set it in the UI, via {provider_env_key}, \
             or add ~/.alva/config.json providers.{}.api_key",
            req.provider, req.provider
        ));
    }
    let base_url = req
        .base_url
        .clone()
        .or_else(|| file_provider.as_ref().and_then(|e| e.base_url.clone()))
        .unwrap_or_else(|| match req.provider.as_str() {
            "anthropic" => "https://api.anthropic.com".into(),
            "openai-responses" => "https://api.openai.com".into(),
            "gemini" => "https://generativelanguage.googleapis.com".into(),
            _ => "https://api.openai.com/v1".into(),
        });
    let model_id = if !req.model.is_empty() {
        req.model.clone()
    } else if let Some(m) = file_provider
        .as_ref()
        .and_then(|e| e.model.clone())
        .filter(|s| !s.is_empty())
    {
        m
    } else {
        return Err(format!(
            "missing model for provider '{}'. Set it in the UI or add ~/.alva/config.json",
            req.provider
        ));
    };
    Ok(ProviderConfig {
        api_key,
        base_url,
        model: model_id,
        max_tokens: 0,
        custom_headers: Default::default(),
        kind: Some(req.provider.clone()),
    })
}

fn build_model(
    req: &SendRequest,
) -> Result<(Arc<dyn alva_kernel_abi::LanguageModel>, ProviderConfig), String> {
    let config = build_provider_config(req)?;
    let model =
        alva_llm_provider::build_language_model(Some(req.provider.as_str()), config.clone());
    Ok((model, config))
}

pub fn default_workspace_for(session_id: &str) -> Result<std::path::PathBuf, String> {
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| "cannot determine home directory".to_string())?;
    Ok(home.join(".alva/workspaces").join(session_id))
}

impl AgentService {
    pub fn new() -> Self {
        Self {
            agent: None,
            agent_key: None,
        }
    }

    /// Build (or reuse) a BaseAgent for this request's provider/model/
    /// workspace combo — the migrated `ensure_agent`.
    pub async fn ensure_agent(
        &mut self,
        req: &SendRequest,
        session_id: &str,
    ) -> Result<Arc<BaseAgent>, String> {
        let plugin_config = default_plugin_state();
        let agent_key = format!(
            "{}:{}:{}|ws={}|{}",
            req.provider,
            req.model,
            req.base_url.as_deref().unwrap_or(""),
            req.workspace.as_deref().unwrap_or(""),
            plugin_config_hash(&plugin_config),
        );
        if self.agent_key.as_deref() == Some(agent_key.as_str()) {
            if let Some(agent) = &self.agent {
                return Ok(Arc::clone(agent));
            }
        }
        let (model, _config) = build_model(req)?;
        let provider_registry = build_provider_registry(&_config);
        let workspace = match &req.workspace {
            Some(ws) => {
                let p = std::path::PathBuf::from(ws);
                if !p.is_dir() {
                    return Err(format!("workspace does not exist: {ws}"));
                }
                p
            }
            None => default_workspace_for(session_id)?,
        };
        let system_prompt = req
            .system_prompt
            .clone()
            .unwrap_or_else(|| "You are Alva, a helpful coding assistant.".to_string());
        let paths = AlvaPaths::new(&workspace);
        let mut builder = BaseAgent::builder();
        builder = builder
            .workspace(&workspace)
            .system_prompt(&system_prompt)
            .max_iterations(20);
        let (approval_ext, _rx) = ApprovalPlugin::with_channel();
        builder = builder.plugin(Box::new(approval_ext));
        let toggles: alva_app_core::components::ComponentToggles = plugin_config;
        let component_ctx = alva_app_core::components::ComponentContext {
            workspace: workspace.clone(),
            provider_registry: Some(provider_registry),
            skills: Some(alva_app_core::components::SkillsDiscovery::from_alva_paths(
                &paths, None,
            )),
            mcp_config_paths: paths.mcp_discovery_paths(),
            subagent_depth: 3,
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
        builder = alva_app_core::components::apply_components(builder, &toggles, &component_ctx);
        let agent = builder
            .build(model)
            .await
            .map_err(|e| format!("build BaseAgent: {e}"))?;
        let agent = Arc::new(agent);
        agent.set_checkpoint_callback(Arc::new(
            alva_app_core::checkpoint::ManagerCheckpointCallback::new(
                alva_app_core::checkpoint::CheckpointManager::new(&workspace),
            ),
        ));
        self.agent = Some(Arc::clone(&agent));
        self.agent_key = Some(agent_key);
        Ok(agent)
    }

    /// Run one prompt and return the event channel (migrated `send_message`
    /// minus session plumbing — sessions arrive in a later batch).
    pub async fn send(
        &mut self,
        req: SendRequest,
        session_id: &str,
        text: &str,
    ) -> Result<tokio::sync::mpsc::UnboundedReceiver<alva_kernel_core::event::AgentEvent>, String>
    {
        let agent = self.ensure_agent(&req, session_id).await?;
        let effort = req
            .reasoning_effort
            .as_deref()
            .and_then(alva_kernel_abi::ReasoningEffort::parse);
        agent.set_reasoning_effort(effort).await;
        agent.set_allowed_tools(req.tool_names.clone()).await;
        Ok(agent.prompt_text(text).await)
    }
}

impl Default for AgentService {
    fn default() -> Self {
        Self::new()
    }
}
