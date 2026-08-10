// INPUT:  gpui + gpui-component, AgentService (service.rs)
// OUTPUT: ALVA Desktop Rust-native shell — P0: chat view with streaming
//         responses and provider/model/key inputs.
// POS:    GPUI port of alva-app-tauri (React Home.tsx). Later batches add
//         sessions (SQLite), settings, skills/mcp, tools and inspector.

mod mcp;
mod service;
mod skills;
mod sqlite_session;
mod tools;

use alva_kernel_abi::agent_session::AgentSession as _;

use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants as _},
    input::{Input, InputEvent, InputState},
    scroll::ScrollableElement,
    searchable_list::SearchableVec,
    select::{Select, SelectState},
    ActiveTheme, *,
};

use service::{AgentService, SendRequest};
use sqlite_session::{SessionSummary, SqliteEvalSessionManager};

#[derive(Clone)]
enum ChatRole {
    User,
    Assistant,
}

#[derive(Clone)]
struct ChatMessage {
    role: ChatRole,
    text: String,
}

#[derive(Clone, Copy, PartialEq)]
enum View {
    Chat,
    Skills,
    Mcp,
    Inspector,
}

const PROVIDERS: [&str; 3] = ["anthropic", "openai-responses", "gemini"];

struct ChatApp {
    service: Arc<tokio::sync::Mutex<AgentService>>,
    session_manager: Arc<SqliteEvalSessionManager>,
    sessions: Vec<SessionSummary>,
    active_session_id: Option<String>,
    tokio: tokio::runtime::Handle,
    provider: Entity<SelectState<SearchableVec<&'static str>>>,
    model: Entity<InputState>,
    api_key: Entity<InputState>,
    prompt: Entity<InputState>,
    messages: Vec<ChatMessage>,
    busy: bool,
    error: Option<String>,
    // Settings surface.
    settings_open: bool,
    settings_model: Entity<InputState>,
    settings_key: Entity<InputState>,
    settings_base_url: Entity<InputState>,
    settings_status: Option<String>,
    // P3 surfaces.
    view: View,
    skills_list: Vec<crate::skills::SkillInfo>,
    mcp_list: Vec<crate::mcp::McpServerInfo>,
    // P4: tool picker.
    tools_open: bool,
    tool_catalog: Vec<crate::tools::PluginToolInfo>,
    selected_tools: std::collections::HashSet<String>,
    // Inspector.
    inspector_record: Option<alva_app_core::session_projection::RunRecord>,
}

impl ChatApp {
    fn new(tokio: tokio::runtime::Handle, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let db_path = std::env::var("ALVA_APP_DB").unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            format!("{home}/Library/Application Support/com.alva.app-gpui/alva.db")
        });
        if let Some(parent) = std::path::Path::new(&db_path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let session_manager = Arc::new(
            SqliteEvalSessionManager::open(std::path::PathBuf::from(db_path))
                .expect("open session db"),
        );
        let sessions = session_manager.list_sessions();
        Self {
            service: Arc::new(tokio::sync::Mutex::new(AgentService::new())),
            session_manager,
            sessions,
            active_session_id: None,
            tokio,
            provider: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(PROVIDERS.to_vec()),
                    Some(IndexPath::new(0)),
                    window,
                    cx,
                )
            }),
            model: cx.new(|cx| {
                InputState::new(window, cx).placeholder("model (e.g. claude-sonnet-4-5)")
            }),
            api_key: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("API key (or leave empty for env)")
                    .masked(true)
            }),
            prompt: cx.new(|cx| {
                InputState::new(window, cx)
                    .multi_line(true)
                    .placeholder("Ask Alva to inspect, change, or explain this project…")
            }),
            messages: Vec::new(),
            busy: false,
            error: None,
            settings_open: false,
            settings_model: cx.new(|cx| InputState::new(window, cx).placeholder("default model")),
            settings_key: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("api key")
                    .masked(true)
            }),
            settings_base_url: cx
                .new(|cx| InputState::new(window, cx).placeholder("base url (optional)")),
            settings_status: None,
            view: View::Chat,
            skills_list: Vec::new(),
            mcp_list: Vec::new(),
            tools_open: false,
            tool_catalog: crate::tools::all_default_tools(),
            selected_tools: std::collections::HashSet::new(),
            inspector_record: None,
        }
    }

    fn refresh_catalog(&mut self, cx: &mut Context<Self>) {
        self.mcp_list = crate::mcp::load_mcp_servers();
        let sources = crate::skills::discover_skill_sources();
        let weak = cx.entity().downgrade();
        let handle = self.tokio.clone();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Vec<crate::skills::SkillInfo>>();
        handle.spawn(async move {
            let mut all = Vec::new();
            for source in sources {
                let infos = crate::skills::scan_skills(std::path::Path::new(&source.path)).await;
                all.extend(infos);
            }
            let _ = tx.send(all);
        });
        cx.spawn(async move |_this, cx| {
            while let Some(skills) = rx.recv().await {
                if let Some(hub) = weak.upgrade() {
                    hub.update(cx, |hub, cx| {
                        hub.skills_list = skills;
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    fn load_settings_into_inputs(&self, window: &mut Window, cx: &mut Context<Self>) {
        let provider = self
            .provider
            .read(cx)
            .selected_value()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "anthropic".into());
        let entry =
            alva_app_core::config::load().and_then(|cfg| cfg.providers.get(&provider).cloned());
        let (model, key, base_url) = match entry {
            Some(entry) => (
                entry.model.clone().unwrap_or_default(),
                entry.api_key.clone(),
                entry.base_url.clone().unwrap_or_default(),
            ),
            None => (String::new(), String::new(), String::new()),
        };
        self.settings_model.update(cx, |state, cx| {
            state.set_value(model, window, cx);
        });
        self.settings_key.update(cx, |state, cx| {
            state.set_value(key, window, cx);
        });
        self.settings_base_url.update(cx, |state, cx| {
            state.set_value(base_url, window, cx);
        });
    }

    fn save_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let provider = self
            .provider
            .read(cx)
            .selected_value()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "anthropic".into());
        let model = self.settings_model.read(cx).value().to_string();
        let key = self.settings_key.read(cx).value().to_string();
        let base_url = self.settings_base_url.read(cx).value().to_string();
        let mut cfg = alva_app_core::config::load().unwrap_or_default();
        cfg.providers.insert(
            provider.clone(),
            alva_app_core::config::ProviderEntry {
                api_key: key,
                model: Some(model),
                base_url: Some(base_url),
                kind: None,
            },
        );
        cfg.active = Some(provider.clone());
        self.settings_status = match alva_app_core::config::save(&cfg) {
            Ok(path) => Some(format!("Saved to {}", path.display())),
            Err(error) => Some(format!("Save failed: {error}")),
        };
        let _ = window;
        cx.notify();
    }

    fn subscribe_prompt(&self, window: &mut Window, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        let _subscription = cx.subscribe_in(
            &self.prompt,
            window,
            move |_view, _state, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    if let Some(entity) = weak.upgrade() {
                        entity.update(cx, |hub, cx| {
                            hub.send(window, cx);
                        });
                    }
                }
            },
        );
    }

    fn request(&self, cx: &Context<Self>) -> SendRequest {
        SendRequest {
            provider: self
                .provider
                .read(cx)
                .selected_value()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "anthropic".into()),
            model: self.model.read(cx).value().to_string(),
            api_key: Some(self.api_key.read(cx).value().to_string()),
            base_url: None,
            system_prompt: None,
            workspace: None,
            reasoning_effort: None,
            tool_names: if self.selected_tools.is_empty() {
                None
            } else {
                Some(self.selected_tools.iter().cloned().collect())
            },
        }
    }

    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let text = self.prompt.read(cx).value().to_string();
        if text.trim().is_empty() {
            return;
        }
        let req = self.request(cx);
        self.prompt.update(cx, |state, cx| {
            state.set_value("", window, cx);
        });
        self.messages.push(ChatMessage {
            role: ChatRole::User,
            text: text.clone(),
        });
        self.busy = true;
        self.error = None;
        cx.notify();

        let session_id = self
            .active_session_id
            .clone()
            .unwrap_or_else(|| "session-1".to_string());
        let service = Arc::clone(&self.service);
        let weak = cx.entity().downgrade();
        let handle = self.tokio.clone();
        let (ui_tx, mut ui_rx) = tokio::sync::mpsc::unbounded_channel::<ChatMessage>();
        handle.spawn(async move {
            let mut service = service.lock().await;
            let result = service.send(req, &session_id, &text).await;
            let mut receiver = match result {
                Ok(rx) => rx,
                Err(error) => {
                    let _ = ui_tx.send(ChatMessage {
                        role: ChatRole::Assistant,
                        text: format!("Error: {error}"),
                    });
                    return;
                }
            };
            while let Some(event) = receiver.recv().await {
                use alva_kernel_core::event::AgentEvent;
                match event {
                    AgentEvent::MessageStart { message } => {
                        let text = extract_text(&message);
                        let _ = ui_tx.send(ChatMessage {
                            role: ChatRole::Assistant,
                            text,
                        });
                    }
                    AgentEvent::MessageUpdate { message, .. } => {
                        let text = extract_text(&message);
                        let _ = ui_tx.send(ChatMessage {
                            role: ChatRole::Assistant,
                            text,
                        });
                    }
                    AgentEvent::MessageEnd { message } => {
                        let text = extract_text(&message);
                        let _ = ui_tx.send(ChatMessage {
                            role: ChatRole::Assistant,
                            text,
                        });
                    }
                    AgentEvent::AgentEnd { error: Some(error) } => {
                        let _ = ui_tx.send(ChatMessage {
                            role: ChatRole::Assistant,
                            text: format!("Error: {error}"),
                        });
                    }
                    AgentEvent::AgentEnd { error: None } => {}
                    _ => {}
                }
            }
        });
        cx.spawn(async move |_this, cx| {
            while let Some(message) = ui_rx.recv().await {
                if let Some(hub) = weak.upgrade() {
                    hub.update(cx, |hub, cx| {
                        hub.upsert_assistant(message);
                        cx.notify();
                    });
                }
            }
            if let Some(hub) = weak.upgrade() {
                hub.update(cx, |hub, cx| {
                    hub.busy = false;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Streaming assistant messages upsert the last assistant card until a
    /// new user turn or completion replaces it.
    fn upsert_assistant(&mut self, message: ChatMessage) {
        if matches!(
            self.messages.last().map(|m| &m.role),
            Some(ChatRole::Assistant)
        ) {
            if let Some(last) = self.messages.last_mut() {
                last.text = message.text;
            }
        } else {
            self.messages.push(message);
        }
    }

    fn settings_panel(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = self.settings_status.clone();
        let weak = cx.entity().downgrade();
        v_flex()
            .gap_2()
            .px_4()
            .py_3()
            .border_b_1()
            .border_color(cx.theme().colors.border)
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight(600.))
                    .child("Provider settings"),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().colors.muted_foreground)
                            .child("Model"),
                    )
                    .child(Input::new(&self.settings_model).small()),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().colors.muted_foreground)
                            .child("API key"),
                    )
                    .child(Input::new(&self.settings_key).small()),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().colors.muted_foreground)
                            .child("Base URL (optional)"),
                    )
                    .child(Input::new(&self.settings_base_url).small()),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Button::new("save-settings")
                            .primary()
                            .small()
                            .label("Save")
                            .on_click(move |_, window, cx| {
                                if weak.upgrade().is_some() {
                                    let handle = window.window_handle();
                                    let _ = handle.update(cx, |root, window, cx| {
                                        if let Ok(view) = root.downcast::<ChatApp>() {
                                            view.update(cx, |hub, cx| {
                                                hub.save_settings(window, cx);
                                            });
                                        }
                                    });
                                }
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().colors.muted_foreground)
                            .child(status.unwrap_or_default()),
                    ),
            )
    }

    fn tool_picker(&self, cx: &Context<Self>) -> impl IntoElement {
        let catalog = self.tool_catalog.clone();
        let weak = cx.entity().downgrade();
        let mut rows = v_flex().gap_1().max_h(px(240.)).overflow_y_scrollbar();
        for tool in &catalog {
            let name = tool.name.clone();
            let description = tool.description.clone();
            let checked = self.selected_tools.contains(&name);
            let weak = weak.clone();
            rows = rows.child(
                div()
                    .id(SharedString::from(format!("tool-{name}")))
                    .v_flex()
                    .gap_0()
                    .cursor_pointer()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .size_3()
                                    .rounded_sm()
                                    .when(checked, |this| this.bg(cx.theme().colors.primary))
                                    .border_1()
                                    .border_color(cx.theme().colors.border),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight(600.))
                                    .child(name.clone()),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().colors.muted_foreground)
                            .child(description),
                    )
                    .on_click(move |_, window, cx| {
                        if weak.upgrade().is_some() {
                            let handle = window.window_handle();
                            let _ = handle.update(cx, |root, _window, cx| {
                                if let Ok(view) = root.downcast::<ChatApp>() {
                                    view.update(cx, |hub, cx| {
                                        if !hub.selected_tools.insert(name.clone()) {
                                            hub.selected_tools.remove(&name);
                                        }
                                        cx.notify();
                                    });
                                }
                            });
                        }
                    }),
            );
        }
        rows
    }

    fn chat_area(&self, cx: &Context<Self>) -> impl IntoElement {
        let messages = self.messages.clone();
        div()
            .flex_1()
            .size_full()
            .v_flex()
            .overflow_y_scrollbar()
            .px_4()
            .py_3()
            .gap_2()
            .children(if messages.is_empty() {
                vec![v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .child(div().text_lg().font_weight(FontWeight(700.)).child("ALVA"))
                    .child(
                        div()
                            .text_color(cx.theme().colors.muted_foreground)
                            .child("A helpful coding assistant, running natively."),
                    )
                    .into_any_element()]
            } else {
                messages
                    .iter()
                    .map(|message| {
                        let is_user = matches!(message.role, ChatRole::User);
                        let role_label = if is_user { "You" } else { "Alva" };
                        let card = div()
                            .id(SharedString::from(format!(
                                "msg-{}-{}",
                                if is_user { "u" } else { "a" },
                                message.text.len()
                            )))
                            .v_flex()
                            .gap_1()
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .when(is_user, |this| this.bg(cx.theme().colors.button))
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight(600.))
                                    .text_color(cx.theme().colors.muted_foreground)
                                    .child(role_label),
                            )
                            .child(div().text_sm().child(message.text.clone()));
                        card.into_any_element()
                    })
                    .collect::<Vec<_>>()
            })
    }

    fn composer(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.busy;
        let weak = cx.entity().downgrade();
        v_flex()
            .gap_2()
            .px_4()
            .py_3()
            .border_t_1()
            .border_color(cx.theme().colors.border)
            .child(
                h_flex().gap_2().items_center().child(
                    Button::new("tools-toggle")
                        .ghost()
                        .small()
                        .label(if self.tools_open {
                            "Hide tools".to_string()
                        } else {
                            format!(
                                "Tools{}",
                                if self.selected_tools.is_empty() {
                                    String::new()
                                } else {
                                    format!(" ({})", self.selected_tools.len())
                                }
                            )
                        })
                        .on_click({
                            let weak = cx.entity().downgrade();
                            move |_, _, cx| {
                                if let Some(entity) = weak.upgrade() {
                                    entity.update(cx, |hub, cx| {
                                        hub.tools_open = !hub.tools_open;
                                        cx.notify();
                                    });
                                }
                            }
                        }),
                ),
            )
            .when(self.tools_open, |this| this.child(self.tool_picker(cx)))
            .child(Input::new(&self.prompt))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().colors.muted_foreground)
                            .child(if busy {
                                "Alva is working…"
                            } else {
                                "Enter to send"
                            }),
                    )
                    .child(
                        Button::new("send")
                            .primary()
                            .label(if busy { "Running…" } else { "Send" })
                            .disabled(busy)
                            .on_click(move |_, window, cx| {
                                if weak.upgrade().is_some() {
                                    let handle = window.window_handle();
                                    let _ = handle.update(cx, |root, window, cx| {
                                        if let Ok(view) = root.downcast::<ChatApp>() {
                                            view.update(cx, |hub, cx| {
                                                hub.send(window, cx);
                                            });
                                        }
                                    });
                                }
                            }),
                    ),
            )
    }

    fn topbar(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_2()
            .px_4()
            .py_3()
            .border_b_1()
            .border_color(cx.theme().colors.border)
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().colors.muted_foreground)
                                    .child("Provider"),
                            )
                            .child(Select::new(&self.provider).small()),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().colors.muted_foreground)
                                    .child("Model"),
                            )
                            .child(Input::new(&self.model).small()),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().colors.muted_foreground)
                                    .child("API key"),
                            )
                            .child(Input::new(&self.api_key).small()),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("settings")
                            .ghost()
                            .label(if self.settings_open {
                                "Close settings"
                            } else {
                                "Settings"
                            })
                            .on_click({
                                let weak = cx.entity().downgrade();
                                move |_, window, cx| {
                                    if weak.upgrade().is_some() {
                                        let handle = window.window_handle();
                                        let _ = handle.update(cx, |root, window, cx| {
                                            if let Ok(view) = root.downcast::<ChatApp>() {
                                                view.update(cx, |hub, cx| {
                                                    hub.settings_open = !hub.settings_open;
                                                    if hub.settings_open {
                                                        hub.load_settings_into_inputs(window, cx);
                                                    }
                                                    cx.notify();
                                                });
                                            }
                                        });
                                    }
                                }
                            }),
                    ),
            )
    }
}

fn extract_text(message: &alva_kernel_abi::AgentMessage) -> String {
    match message {
        alva_kernel_abi::AgentMessage::Standard(message) => message.text_content(),
        _ => String::new(),
    }
}

impl ChatApp {
    fn sidebar(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sessions = self.sessions.clone();
        let active = self.active_session_id.clone();
        let weak_new = cx.entity().downgrade();
        let weak = cx.entity().downgrade();
        v_flex()
            .id("app-sidebar")
            .flex_col()
            .gap_2()
            .px_3()
            .py_3()
            .w(px(240.))
            .bg(cx.theme().colors.sidebar)
            .border_r_1()
            .border_color(cx.theme().colors.border)
            .overflow_y_scrollbar()
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_lg().font_weight(FontWeight(700.)).child("ALVA"))
                    .child(
                        Button::new("new-session")
                            .secondary()
                            .xsmall()
                            .label("New")
                            .on_click(move |_, window, app| {
                                if weak_new.upgrade().is_some() {
                                    let handle = window.window_handle();
                                    let _ = handle.update(app, |root, window, cx| {
                                        if let Ok(view) = root.downcast::<ChatApp>() {
                                            view.update(cx, |hub, cx| {
                                                hub.new_session(window, cx);
                                            });
                                        }
                                    });
                                }
                            }),
                    ),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(self.nav_button("Chat", View::Chat, cx))
                    .child(self.nav_button("Skills", View::Skills, cx))
                    .child(self.nav_button("MCP", View::Mcp, cx))
                    .child(self.nav_button("Inspector", View::Inspector, cx)),
            )
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight(650.))
                    .text_color(cx.theme().colors.muted_foreground)
                    .child("Sessions"),
            )
            .child(div().flex_col().gap_1().children(if sessions.is_empty() {
                vec![div()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child("No sessions yet.")
                    .into_any_element()]
            } else {
                sessions
                    .iter()
                    .map(|session| {
                        let weak = weak.clone();
                        let id = session.session_id.clone();
                        let preview = session.preview.clone();
                        let is_active = active.as_deref() == Some(session.session_id.as_str());
                        div()
                            .id(SharedString::from(format!("session-{id}")))
                            .v_flex()
                            .gap_1()
                            .px_2()
                            .py_1_5()
                            .rounded_md()
                            .cursor_pointer()
                            .when(is_active, |this| this.bg(cx.theme().colors.button))
                            .child(div().text_sm().font_weight(FontWeight(600.)).child(preview))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().colors.muted_foreground)
                                    .child(id.chars().take(8).collect::<String>()),
                            )
                            .on_click(move |_, window, cx| {
                                if weak.upgrade().is_some() {
                                    let handle = window.window_handle();
                                    let _ = handle.update(cx, |root, window, cx| {
                                        if let Ok(view) = root.downcast::<ChatApp>() {
                                            view.update(cx, |hub, cx| {
                                                hub.switch_session(&id, window, cx);
                                            });
                                        }
                                    });
                                }
                            })
                            .into_any_element()
                    })
                    .collect::<Vec<_>>()
            }))
    }

    fn nav_button(&self, label: &str, view: View, cx: &Context<Self>) -> impl IntoElement {
        let weak = cx.entity().downgrade();
        let view_key = view;
        Button::new(SharedString::from(format!("nav-{label}")))
            .ghost()
            .small()
            .label(label)
            .selected(self.view == view_key)
            .on_click(move |_, window, cx| {
                if weak.upgrade().is_some() {
                    let handle = window.window_handle();
                    let _ = handle.update(cx, |root, _window, cx| {
                        if let Ok(view) = root.downcast::<ChatApp>() {
                            view.update(cx, |hub, cx| {
                                hub.view = view_key;
                                if hub.view != View::Chat {
                                    hub.refresh_catalog(cx);
                                }
                                cx.notify();
                            });
                        }
                    });
                }
            })
    }

    fn skills_view(&self, cx: &Context<Self>) -> impl IntoElement {
        let skills = self.skills_list.clone();
        div()
            .flex_1()
            .size_full()
            .v_flex()
            .overflow_y_scrollbar()
            .px_4()
            .py_3()
            .gap_2()
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight(600.))
                    .child("Skills"),
            )
            .children(if skills.is_empty() {
                vec![div()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child("No skills discovered yet.")
                    .into_any_element()]
            } else {
                skills
                    .iter()
                    .map(|skill| {
                        v_flex()
                            .gap_1()
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .border_1()
                            .border_color(cx.theme().colors.border)
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight(600.))
                                    .child(skill.name.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().colors.muted_foreground)
                                    .child(skill.description.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().colors.muted_foreground)
                                    .child(format!(
                                        "{} · {}",
                                        skill.kind,
                                        if skill.enabled { "enabled" } else { "disabled" }
                                    )),
                            )
                            .into_any_element()
                    })
                    .collect::<Vec<_>>()
            })
    }

    fn mcp_view(&self, cx: &Context<Self>) -> impl IntoElement {
        let servers = self.mcp_list.clone();
        div()
            .flex_1()
            .size_full()
            .v_flex()
            .overflow_y_scrollbar()
            .px_4()
            .py_3()
            .gap_2()
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight(600.))
                    .child("MCP servers"),
            )
            .children(if servers.is_empty() {
                vec![div()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child("No MCP servers configured.")
                    .into_any_element()]
            } else {
                servers
                    .iter()
                    .map(|server| {
                        v_flex()
                            .gap_1()
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .border_1()
                            .border_color(cx.theme().colors.border)
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight(600.))
                                    .child(server.name.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().colors.muted_foreground)
                                    .child(format!(
                                        "{} · {}",
                                        server.kind,
                                        if server.enabled {
                                            "enabled"
                                        } else {
                                            "disabled"
                                        }
                                    )),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().colors.muted_foreground)
                                    .child(server.command_or_url.clone()),
                            )
                            .into_any_element()
                    })
                    .collect::<Vec<_>>()
            })
    }

    fn load_inspector_record(&mut self, session_id: &str, cx: &mut Context<Self>) {
        let manager = Arc::clone(&self.session_manager);
        let weak = cx.entity().downgrade();
        let handle = self.tokio.clone();
        let (tx, mut rx) =
            tokio::sync::mpsc::unbounded_channel::<alva_app_core::session_projection::RunRecord>();
        let session_id = session_id.to_string();
        handle.spawn(async move {
            use alva_kernel_abi::agent_session::{AgentSession as _, EventQuery};
            if let Some(session) = manager.load_session(&session_id).await {
                let events: Vec<alva_kernel_abi::agent_session::SessionEvent> = session
                    .query(&EventQuery {
                        limit: usize::MAX,
                        ..Default::default()
                    })
                    .await
                    .into_iter()
                    .map(|m| m.event)
                    .collect();
                let record = alva_app_core::session_projection::build_run_record(&events);
                let _ = tx.send(record);
            }
        });
        cx.spawn(async move |_this, cx| {
            while let Some(record) = rx.recv().await {
                if let Some(hub) = weak.upgrade() {
                    hub.update(cx, |hub, cx| {
                        hub.inspector_record = Some(record);
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    fn inspector_view(&self, cx: &Context<Self>) -> impl IntoElement {
        let record = self.inspector_record.clone();
        let sessions = self.sessions.clone();
        let active = self.active_session_id.clone();
        let weak = cx.entity().downgrade();
        div()
            .flex_1()
            .size_full()
            .v_flex()
            .overflow_y_scrollbar()
            .px_4()
            .py_3()
            .gap_2()
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight(600.))
                    .child("Inspector"),
            )
            .children(sessions.iter().map(|session| {
                let weak = weak.clone();
                let id = session.session_id.clone();
                let preview = session.preview.clone();
                let is_active = active.as_deref() == Some(id.as_str());
                Button::new(SharedString::from(format!("inspect-{id}")))
                    .ghost()
                    .small()
                    .label(if is_active {
                        format!("{preview} (active)")
                    } else {
                        preview
                    })
                    .selected(is_active)
                    .on_click(move |_, window, cx| {
                        if weak.upgrade().is_some() {
                            let handle = window.window_handle();
                            let _ = handle.update(cx, |root, _window, cx| {
                                if let Ok(view) = root.downcast::<ChatApp>() {
                                    view.update(cx, |hub, cx| {
                                        hub.load_inspector_record(&id, cx);
                                        cx.notify();
                                    });
                                }
                            });
                        }
                    })
                    .into_any_element()
            }))
            .child(if let Some(record) = record {
                v_flex()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_3()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().colors.muted_foreground)
                                    .child(format!("model: {}", record.config_snapshot.model_id)),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().colors.muted_foreground)
                                    .child(format!("{} turns", record.turns.len())),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().colors.muted_foreground)
                                    .child(format!(
                                        "{}in/{}out tokens",
                                        record.total_input_tokens, record.total_output_tokens
                                    )),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().colors.muted_foreground)
                                    .child(format!("{} ms", record.total_duration_ms)),
                            ),
                    )
                    .children(record.turns.iter().map(|turn| {
                        let stop = turn.llm_call.stop_reason.clone();
                        let input = turn.llm_call.input_tokens;
                        let output = turn.llm_call.output_tokens;
                        let tools = turn
                            .tool_calls
                            .iter()
                            .map(|tc| tc.tool_call.name.clone())
                            .collect::<Vec<_>>()
                            .join(", ");
                        let prompt_text = turn
                            .llm_call
                            .messages_sent
                            .last()
                            .map(|m| m.text_content())
                            .unwrap_or_default();
                        let response = turn
                            .llm_call
                            .response
                            .as_ref()
                            .map(|m| m.text_content())
                            .unwrap_or_default();
                        v_flex()
                            .gap_1()
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .border_1()
                            .border_color(cx.theme().colors.border)
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight(600.))
                                            .child(format!("Turn {}", turn.turn_number)),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().colors.muted_foreground)
                                            .child(format!("{stop} · {input}in/{output}out")),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().colors.muted_foreground)
                                    .child(prompt_text),
                            )
                            .when(!response.is_empty(), |this| {
                                this.child(div().text_sm().child(response))
                            })
                            .when(!tools.is_empty(), |this| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().colors.muted_foreground)
                                        .child(format!("tools: {tools}")),
                                )
                            })
                            .into_any_element()
                    }))
                    .into_any_element()
            } else {
                div()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child("Select a session above to inspect its run record.")
                    .into_any_element()
            })
    }

    fn refresh_sessions(&mut self) {
        self.sessions = self.session_manager.list_sessions();
    }

    fn new_session(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let manager = Arc::clone(&self.session_manager);
        let weak = cx.entity().downgrade();
        let handle = self.tokio.clone();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        handle.spawn(async move {
            let session = manager.create_session("New session").await;
            let _ = tx.send(session.session_id().to_string());
        });
        cx.spawn(async move |_this, cx| {
            while let Some(id) = rx.recv().await {
                if let Some(hub) = weak.upgrade() {
                    hub.update(cx, |hub, cx| {
                        hub.refresh_sessions();
                        hub.active_session_id = Some(id);
                        hub.messages.clear();
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    fn switch_session(&mut self, id: &str, _window: &mut Window, cx: &mut Context<Self>) {
        let id = id.to_string();
        self.active_session_id = Some(id.clone());
        self.messages.clear();
        let manager = Arc::clone(&self.session_manager);
        let weak = cx.entity().downgrade();
        let handle = self.tokio.clone();
        let service = Arc::clone(&self.service);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        let id_spawn = id.clone();
        handle.spawn(async move {
            let _loaded = manager.load_session(&id_spawn).await;
            let _ = service;
            let _ = tx.send(());
        });
        cx.spawn(async move |_this, cx| {
            while rx.recv().await.is_some() {
                if let Some(hub) = weak.upgrade() {
                    hub.update(cx, |hub, cx| {
                        hub.refresh_sessions();
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }
}

impl Render for ChatApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .size_full()
            .bg(cx.theme().colors.background)
            .child(self.sidebar(window, cx))
            .child(
                v_flex()
                    .flex_1()
                    .size_full()
                    .child(self.topbar(window, cx))
                    .when(self.settings_open, |this| {
                        this.child(self.settings_panel(window, cx))
                    })
                    .child(if self.view == View::Skills {
                        self.skills_view(cx).into_any_element()
                    } else if self.view == View::Mcp {
                        self.mcp_view(cx).into_any_element()
                    } else if self.view == View::Inspector {
                        self.inspector_view(cx).into_any_element()
                    } else {
                        v_flex()
                            .flex_1()
                            .size_full()
                            .child(self.chat_area(cx))
                            .child(self.composer(window, cx))
                            .into_any_element()
                    }),
            )
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let tokio_handle = runtime.handle().clone();
    let _guard = runtime.enter();
    gpui_platform::application()
        .with_assets(gpui_component_assets::Assets)
        .run(move |cx| {
            gpui_component::init(cx);
            cx.spawn(async move |cx| {
                let options = WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(80.), px(60.)),
                        size(px(1100.), px(760.)),
                    ))),
                    ..WindowOptions::default()
                };
                cx.open_window(options, |window, cx| {
                    let view = cx.new(|cx| ChatApp::new(tokio_handle.clone(), window, cx));
                    view.update(cx, |app, cx| {
                        app.subscribe_prompt(window, cx);
                    });
                    cx.new(|cx| Root::new(view, window, cx).bg(cx.theme().colors.background))
                })
                .expect("Failed to open window");
            })
            .detach();
        });
}
