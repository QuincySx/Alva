// INPUT:  gpui, gpui_component (Input, InputState, InputEvent, Button, ButtonVariants, Disableable),
//         crate::models (AgentModel, ChatModel, SettingsModel, WorkspaceModel), crate::chat (GpuiChat, GpuiChatConfig),
//         crate::theme, crate::types::AgentStatusKind, srow_core, srow_ai
// OUTPUT: pub struct InputBox
// POS:    Chat input widget using gpui-component Input/Button, Enter-to-send via InputEvent subscription.
use std::sync::Arc;

use gpui::{prelude::*, Context, Entity, Render, Subscription, Window, div};

use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{InputEvent, InputState, Input};
use gpui_component::{Disableable, Sizable};

use crate::chat::GpuiChatConfig;
use crate::models::{AgentModel, ChatModel, SettingsModel, WorkspaceModel};
use crate::theme::Theme;
use crate::types::AgentStatusKind;

use srow_core::adapters::llm::openai::OpenAILanguageModel;
use srow_core::adapters::storage::memory::MemoryStorage;
use srow_core::domain::agent::AgentConfig;
use srow_core::ports::provider::language_model::LanguageModel;
use srow_core::ports::tool::ToolRegistry;
use srow_core::agent::runtime::tools::register_all_tools;
use srow_ai::transport::DirectChatTransport;

pub struct InputBox {
    input_state: Entity<InputState>,
    workspace_model: Entity<WorkspaceModel>,
    chat_model: Entity<ChatModel>,
    agent_model: Entity<AgentModel>,
    settings_model: Entity<SettingsModel>,
    _subscriptions: Vec<Subscription>,
}

impl InputBox {
    pub fn new(
        workspace_model: Entity<WorkspaceModel>,
        chat_model: Entity<ChatModel>,
        agent_model: Entity<AgentModel>,
        settings_model: Entity<SettingsModel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input_state = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Type a message...")
        });

        let mut subscriptions = Vec::new();

        // Subscribe to InputEvent from the input state
        subscriptions.push(cx.subscribe_in(
            &input_state,
            window,
            |this, _state, event: &InputEvent, window, cx| {
                match event {
                    InputEvent::PressEnter { secondary } => {
                        if !secondary {
                            this.send_message(window, cx);
                        }
                    }
                    _ => {}
                }
            },
        ));

        // Subscribe to agent model to update send button state
        subscriptions.push(cx.subscribe(&agent_model, |_this, _model, _event, cx| {
            cx.notify();
        }));

        Self {
            input_state,
            workspace_model,
            chat_model,
            agent_model,
            settings_model,
            _subscriptions: subscriptions,
        }
    }

    fn send_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        tracing::info!("action_dispatch: send_message");
        let text = self.input_state.read(cx).value().to_string();
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }

        let session_id = {
            let ws = self.workspace_model.read(cx);
            match ws.selected_session_id.clone() {
                Some(id) => id,
                None => return,
            }
        };

        // Check if agent is already running for this session
        {
            let agent = self.agent_model.read(cx);
            if let Some(status) = agent.get_status(&session_id) {
                if status.kind == AgentStatusKind::Running {
                    return; // Don't send while running
                }
            }
        }

        // Read settings
        let settings = self.settings_model.read(cx).settings.clone();

        // Ensure the GpuiChat exists for this session
        let chat_entity = {
            let needs_create = {
                let cm = self.chat_model.read(cx);
                cm.get_chat(&session_id).is_none()
            };

            if needs_create {
                // Build transport from settings
                let transport = self.build_transport(&settings);
                let config = GpuiChatConfig {
                    session_id: session_id.clone(),
                    transport,
                };
                self.chat_model.update(cx, |model, cx| {
                    model.get_or_create_chat(&session_id, config, cx)
                })
            } else {
                self.chat_model
                    .read(cx)
                    .get_chat(&session_id)
                    .unwrap()
                    .clone()
            }
        };

        // Mark agent as running
        self.agent_model.update(cx, |model, cx| {
            model.set_status(&session_id, AgentStatusKind::Running, cx);
        });

        // Send via GpuiChat
        chat_entity.read(cx).send_message(&text);

        // Subscribe to chat events to update agent status
        let agent_model = self.agent_model.clone();
        let sid = session_id.clone();
        cx.subscribe(&chat_entity, move |_this, chat, _event, cx| {
            let chat = chat.read(cx);
            let status = chat.status();
            match status {
                srow_core::ui_message_stream::ChatStatus::Ready => {
                    agent_model.update(cx, |model, cx| {
                        model.set_status(&sid, AgentStatusKind::Idle, cx);
                    });
                }
                srow_core::ui_message_stream::ChatStatus::Error => {
                    agent_model.update(cx, |model, cx| {
                        model.set_status(&sid, AgentStatusKind::Error, cx);
                    });
                }
                _ => {}
            }
        })
        .detach();

        // Clear input
        self.input_state.update(cx, |state, cx| {
            state.set_value("", window, cx);
        });
        cx.notify();
    }

    fn build_transport(
        &self,
        settings: &crate::models::AppSettings,
    ) -> Box<dyn srow_ai::transport::ChatTransport> {
        let api_key = settings.llm.api_key.clone();
        let base_url = settings.llm.base_url.clone();
        let model_name = settings.llm.model.clone();

        // Build LLM provider (Provider V4)
        let llm: Arc<dyn LanguageModel> =
            if base_url == "https://api.openai.com/v1" || base_url.is_empty() {
                Arc::new(OpenAILanguageModel::new(&api_key, &model_name))
            } else {
                Arc::new(OpenAILanguageModel::with_base_url(
                    &api_key, &base_url, &model_name,
                ))
            };

        // Build tool registry with all built-in tools
        let mut registry = ToolRegistry::new();
        register_all_tools(&mut registry);
        let tools = Arc::new(registry);

        // Build in-memory storage
        let storage = Arc::new(MemoryStorage::new());

        // Build agent config
        let config = Arc::new(AgentConfig::default());

        Box::new(DirectChatTransport::new(llm, tools, storage, config))
    }

    fn stop_agent(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        tracing::info!("action_dispatch: stop_agent");
        let session_id = {
            let ws = self.workspace_model.read(cx);
            match ws.selected_session_id.clone() {
                Some(id) => id,
                None => return,
            }
        };

        // Stop the chat if it exists
        {
            let cm = self.chat_model.read(cx);
            if let Some(chat) = cm.get_chat(&session_id) {
                chat.read(cx).stop();
            }
        }

        // Mark agent as idle
        self.agent_model.update(cx, |model, cx| {
            model.set_status(&session_id, AgentStatusKind::Idle, cx);
        });
    }

    fn is_agent_running(&self, cx: &Context<Self>) -> bool {
        let ws = self.workspace_model.read(cx);
        if let Some(ref sid) = ws.selected_session_id {
            let agent = self.agent_model.read(cx);
            if let Some(status) = agent.get_status(sid) {
                return status.kind == AgentStatusKind::Running;
            }
        }
        false
    }
}

impl Render for InputBox {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window, cx);
        let has_text = !self.input_state.read(cx).value().trim().is_empty();
        let has_session = self.workspace_model.read(cx).selected_session_id.is_some();
        let is_running = self.is_agent_running(cx);

        let can_send = has_text && has_session && !is_running;

        // Attach button — disabled placeholder
        let attach_button = Button::new("attach-btn")
            .label("Attach")
            .outline()
            .small()
            .disabled(true);

        // Agent selector — simple label for now
        let agent_label = div()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .py_1()
            .rounded_md()
            .text_xs()
            .text_color(theme.text_muted)
            .child("Main Agent");

        // Send / Stop button
        let action_button = if is_running {
            Button::new("stop-btn")
                .label("Stop")
                .outline()
                .small()
                .on_click(cx.listener(srow_debug::traced_listener!("input:stop_agent", |this, _, window, cx| {
                    this.stop_agent(window, cx);
                })))
        } else {
            Button::new("send-btn")
                .label("Send")
                .primary()
                .small()
                .disabled(!can_send)
                .on_click(cx.listener(srow_debug::traced_listener!("input:send_message", |this, _, window, cx| {
                    this.send_message(window, cx);
                })))
        };

        div()
            .flex()
            .flex_col()
            .w_full()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.background)
            // Multi-line input area
            .child(
                div()
                    .flex_1()
                    .p_3()
                    .child(
                        Input::new(&self.input_state)
                            .disabled(is_running)
                    )
            )
            // Toolbar
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .px_3()
                    .py_2()
                    .gap_2()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(attach_button)
                    .child(div().flex_1()) // spacer
                    .child(agent_label)
                    .child(action_button)
            )
            // Hint text
            .child(
                div()
                    .px_3()
                    .pb_1()
                    .text_xs()
                    .text_color(theme.text_muted)
                    .child("Enter send \u{00B7} Shift+Enter newline")
            )
    }
}
