// INPUT:  gpui, crate::models (AgentModel, ChatModel, SettingsModel, WorkspaceModel), crate::chat (GpuiChat, GpuiChatConfig),
//         crate::theme, crate::types::AgentStatusKind, srow_core, srow_ai
// OUTPUT: pub struct InputBox
// POS:    Focusable chat input widget handling keyboard input, draft text, send-on-enter, and agent-busy guard.
use std::sync::Arc;

use gpui::{prelude::*, Context, Entity, FocusHandle, Focusable, FontWeight, Modifiers, Render, Window, div, px};

use crate::chat::{GpuiChatConfig};
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
    focus_handle: FocusHandle,
    draft: String,
    workspace_model: Entity<WorkspaceModel>,
    chat_model: Entity<ChatModel>,
    agent_model: Entity<AgentModel>,
    settings_model: Entity<SettingsModel>,
}

impl InputBox {
    pub fn new(
        workspace_model: Entity<WorkspaceModel>,
        chat_model: Entity<ChatModel>,
        agent_model: Entity<AgentModel>,
        settings_model: Entity<SettingsModel>,
        cx: &mut Context<Self>,
    ) -> Self {
        // Subscribe to agent model to update send button state
        cx.subscribe(&agent_model, |_this, _model, _event, cx| {
            cx.notify();
        })
        .detach();

        Self {
            focus_handle: cx.focus_handle(),
            draft: String::new(),
            workspace_model,
            chat_model,
            agent_model,
            settings_model,
        }
    }

    fn send_message(&mut self, cx: &mut Context<Self>) {
        let text = self.draft.trim().to_string();
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

        // Clear draft
        self.draft.clear();
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

impl Focusable for InputBox {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for InputBox {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window, cx);
        let draft_display = self.draft.clone();
        let has_text = !self.draft.trim().is_empty();
        let has_session = self.workspace_model.read(cx).selected_session_id.is_some();
        let is_running = self.is_agent_running(cx);

        let can_send = has_text && has_session && !is_running;

        let accent = theme.accent;
        let accent_hover = theme.accent_hover;
        let text_muted = theme.text_muted;

        div()
            .flex()
            .flex_row()
            .w_full()
            .p_3()
            .gap_2()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.background)
            .child(
                // Input area with real keyboard input
                div()
                    .id("input-area")
                    .track_focus(&self.focus_handle)
                    .flex_1()
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.surface)
                    .text_sm()
                    .text_color(theme.text)
                    .min_h(px(36.))
                    .cursor_text()
                    .when(draft_display.is_empty(), |el| {
                        el.child(
                            div()
                                .text_color(text_muted)
                                .child(if is_running {
                                    "Agent is running..."
                                } else {
                                    "Type a message... (Enter to send)"
                                }),
                        )
                    })
                    .when(!draft_display.is_empty(), |el| {
                        // Show text with cursor indicator
                        el.child(format!("{}|", draft_display))
                    })
                    .on_click(cx.listener(|this, _, window, _cx| {
                        this.focus_handle.focus(window);
                    }))
                    .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _window, cx| {
                        let key = &event.keystroke.key;
                        if key == "backspace" {
                            this.draft.pop();
                            cx.notify();
                        } else if key == "enter" {
                            if event.keystroke.modifiers.shift {
                                // Shift+Enter = newline
                                this.draft.push('\n');
                                cx.notify();
                            } else {
                                // Enter = send
                                this.send_message(cx);
                            }
                        } else if key == "space" && event.keystroke.modifiers == Modifiers::none() {
                            this.draft.push(' ');
                            cx.notify();
                        } else {
                            // Handle regular text input
                            if let Some(ref key_char) = event.keystroke.key_char {
                                this.draft.push_str(key_char);
                                cx.notify();
                            } else if key.len() == 1 && event.keystroke.modifiers == Modifiers::none() {
                                this.draft.push_str(key);
                                cx.notify();
                            }
                        }
                    })),
            )
            .child(
                // Send button
                div()
                    .id("send-btn")
                    .px_4()
                    .py_2()
                    .rounded_lg()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .cursor_pointer()
                    .when(can_send, |el| {
                        el.bg(accent)
                            .text_color(gpui::white())
                            .hover(move |style| style.bg(accent_hover))
                    })
                    .when(!can_send, |el| {
                        el.bg(theme.surface_hover)
                            .text_color(text_muted)
                            .cursor_not_allowed()
                            .opacity(0.6)
                    })
                    .child(if is_running { "..." } else { "Send" })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.send_message(cx);
                    })),
            )
    }
}
