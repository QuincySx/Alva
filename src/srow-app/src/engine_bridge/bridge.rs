//! EngineBridge — adapter between GPUI UI layer and srow_core::AgentEngine.
//!
//! Creates a real AgentEngine with OpenAI-compatible LLM provider, in-memory storage,
//! and built-in tools.  Engine events are streamed back to GPUI models on the main thread.

use std::sync::Arc;

use futures::StreamExt;
use gpui::{AppContext, Context, Entity};
use srow_core::{
    AgentConfig, AgentEngine, EngineEvent, LLMConfig, LLMProviderKind, LLMMessage,
    SessionStorage,
};
use srow_core::adapters::llm::openai_compat::OpenAICompatProvider;
use srow_core::adapters::storage::memory::MemoryStorage;
use srow_core::ports::tool::ToolRegistry;
use srow_core::agent::runtime::tools::register_all_tools;
use srow_core::domain::session::{Session as CoreSession, SessionStatus};

use crate::models::{AgentModel, ChatModel, SettingsModel};
use crate::types::AgentStatusKind;

/// Bridges engine events into GPUI Model updates.
pub struct EngineBridge;

impl EngineBridge {
    /// Send a user message and run the real AgentEngine loop.
    /// Streams EngineEvents back to ChatModel / AgentModel on the main thread.
    pub fn send_message<V: 'static>(
        session_id: String,
        prompt: String,
        chat_model: Entity<ChatModel>,
        agent_model: Entity<AgentModel>,
        settings_model: Entity<SettingsModel>,
        cx: &mut Context<V>,
    ) {
        // Read settings snapshot
        let settings = settings_model.read(cx).settings.clone();

        if !settings.has_api_key() {
            tracing::warn!("No API key configured, cannot send message");
            chat_model.update(cx, |model, cx| {
                model.push_error_message(
                    &session_id,
                    "No API key configured. Please go to Settings to configure your LLM API key.".to_string(),
                    cx,
                );
            });
            return;
        }

        // Set proxy environment variables if configured so that reqwest picks them up
        if settings.proxy.enabled && !settings.proxy.url.is_empty() {
            // TODO: Passing proxy to OpenAICompatProvider directly requires changes to
            // rig-core's HTTP client. For now, set environment variables that reqwest
            // reads automatically.
            std::env::set_var("HTTPS_PROXY", &settings.proxy.url);
            std::env::set_var("ALL_PROXY", &settings.proxy.url);
        }

        // Mark agent as running
        agent_model.update(cx, |model, cx| {
            model.set_status(&session_id, AgentStatusKind::Running, cx);
        });

        let sid = session_id.clone();

        // Create a futures channel to bridge background engine -> GPUI main thread
        let (mut tx, mut rx) = futures::channel::mpsc::channel::<EngineEvent>(256);

        let engine_sid = sid.clone();
        let engine_prompt = prompt.clone();
        let api_key = settings.llm.api_key.clone();
        let base_url = settings.llm.base_url.clone();
        let model_name = settings.llm.model.clone();

        // Spawn the engine on a background thread; it sends events through `tx`
        cx.background_spawn(async move {
            // Create a dedicated tokio runtime for the engine
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Failed to create tokio runtime");

            rt.block_on(async move {
                run_engine(
                    &engine_sid,
                    &engine_prompt,
                    &api_key,
                    &base_url,
                    &model_name,
                    &mut tx,
                )
                .await
            })
        })
        .detach();

        // Spawn an async task on the GPUI foreground to consume events one-by-one
        cx.spawn(async move |_this, cx| {
            let chat = chat_model.clone();
            let agent = agent_model.clone();
            let sid2 = sid.clone();

            while let Some(event) = rx.next().await {
                let is_terminal = matches!(
                    &event,
                    EngineEvent::Completed { .. } | EngineEvent::Error { .. }
                );

                let chat = chat.clone();
                let agent = agent.clone();
                let sid2 = sid2.clone();

                cx.update(|cx| {
                    match &event {
                        EngineEvent::TextDelta { text, .. } => {
                            chat.update(cx, |model, cx| {
                                model.append_text_delta(&sid2, text, cx);
                            });
                        }
                        EngineEvent::ThinkingDelta { text, .. } => {
                            chat.update(cx, |model, cx| {
                                model.append_thinking_delta(&sid2, text, cx);
                            });
                        }
                        EngineEvent::ToolCallStarted {
                            tool_name,
                            tool_call_id,
                            ..
                        } => {
                            // Finalize any in-progress stream before tool call
                            chat.update(cx, |model, cx| {
                                model.finalize_stream(&sid2, cx);
                                model.push_tool_call_start(
                                    &sid2,
                                    tool_name.clone(),
                                    tool_call_id.clone(),
                                    cx,
                                );
                            });
                        }
                        EngineEvent::ToolCallCompleted {
                            tool_call_id,
                            output,
                            is_error,
                            ..
                        } => {
                            chat.update(cx, |model, cx| {
                                model.push_tool_call_end(
                                    &sid2,
                                    tool_call_id.clone(),
                                    output.clone(),
                                    *is_error,
                                    cx,
                                );
                            });
                        }
                        EngineEvent::Completed { .. } => {
                            chat.update(cx, |model, cx| {
                                model.finalize_stream(&sid2, cx);
                            });
                            agent.update(cx, |model, cx| {
                                model.set_status(&sid2, AgentStatusKind::Idle, cx);
                            });
                        }
                        EngineEvent::Error { error, .. } => {
                            chat.update(cx, |model, cx| {
                                model.finalize_stream(&sid2, cx);
                                model.push_error_message(&sid2, error.clone(), cx);
                            });
                            agent.update(cx, |model, cx| {
                                model.set_status(&sid2, AgentStatusKind::Error, cx);
                            });
                        }
                        EngineEvent::TokenUsage { .. } => {
                            // Could track token usage in a model; skip for now
                        }
                        EngineEvent::WaitingForHuman { .. } => {
                            agent.update(cx, |model, cx| {
                                model.set_status(&sid2, AgentStatusKind::WaitingHitl, cx);
                            });
                        }
                    }
                })
                .ok();

                if is_terminal {
                    break;
                }
            }
        })
        .detach();
    }
}

/// Run a real AgentEngine session, sending each event through the channel as it arrives.
async fn run_engine(
    session_id: &str,
    prompt: &str,
    api_key: &str,
    base_url: &str,
    model_name: &str,
    event_sink: &mut futures::channel::mpsc::Sender<EngineEvent>,
) {
    use futures::SinkExt;

    // Build LLM provider
    let llm: Arc<dyn srow_core::LLMProvider> = if base_url == "https://api.openai.com/v1"
        || base_url.is_empty()
    {
        Arc::new(OpenAICompatProvider::new(api_key, model_name))
    } else {
        Arc::new(OpenAICompatProvider::with_base_url(
            api_key, base_url, model_name,
        ))
    };

    // Build tool registry with all built-in tools
    let mut registry = ToolRegistry::new();
    register_all_tools(&mut registry);
    let tools = Arc::new(registry);

    // Build in-memory storage
    let storage = Arc::new(MemoryStorage::new());

    // Create a core session in storage
    let core_session = CoreSession {
        id: session_id.to_string(),
        workspace: ".".to_string(),
        agent_config_snapshot: serde_json::Value::Null,
        status: SessionStatus::Idle,
        total_tokens: 0,
        iteration_count: 0,
    };
    if let Err(e) = storage.create_session(&core_session).await {
        tracing::error!("Failed to create core session: {}", e);
        let _ = event_sink
            .send(EngineEvent::Error {
                session_id: session_id.to_string(),
                error: format!("Failed to create session: {}", e),
            })
            .await;
        return;
    }

    // Build agent config
    let config = AgentConfig {
        id: uuid::Uuid::new_v4().to_string(),
        name: "Srow Agent".to_string(),
        system_prompt: "You are Srow Agent, a helpful AI assistant. Answer questions clearly and concisely.".to_string(),
        llm: LLMConfig {
            provider: LLMProviderKind::OpenAI,
            model: model_name.to_string(),
            api_key: api_key.to_string(),
            base_url: Some(base_url.to_string()),
            max_tokens: 8192,
            temperature: None,
        },
        workspace: std::path::PathBuf::from("."),
        allowed_tools: None,
        max_iterations: 20,
        compaction_threshold: 0,
    };

    // Create internal tokio channel for the engine
    let (engine_tx, mut engine_rx) = tokio::sync::mpsc::channel::<EngineEvent>(256);
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

    // Create engine
    let mut engine = AgentEngine::new(config, llm, tools, storage, engine_tx, cancel_rx);

    // Build user message
    let user_msg = LLMMessage::user(prompt);

    // Spawn the engine run in a task
    let sid = session_id.to_string();
    let engine_handle = tokio::spawn(async move {
        if let Err(e) = engine.run(&sid, user_msg).await {
            tracing::error!("Engine run error: {}", e);
        }
    });

    // Forward events from the tokio channel to the futures channel one by one
    while let Some(event) = engine_rx.recv().await {
        let is_terminal = matches!(
            &event,
            EngineEvent::Completed { .. } | EngineEvent::Error { .. }
        );
        if event_sink.send(event).await.is_err() {
            tracing::warn!("GPUI receiver dropped, stopping engine event forwarding");
            break;
        }
        if is_terminal {
            break;
        }
    }

    // Ensure engine task completes
    let _ = engine_handle.await;
    drop(cancel_tx);
}
