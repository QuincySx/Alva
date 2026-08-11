// INPUT:  gpui-component, AgentService, SQLite sessions, skills/MCP/tool catalogs
// OUTPUT: ALVA Desktop native chat shell with sessions, settings Sheet and inspector views
// POS:    Single-engine GPUI reference product aligned with the Hub component design system.

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
    alert::Alert,
    avatar::Avatar,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    group_box::{GroupBox, GroupBoxVariants as _},
    input::{Input, InputEvent, InputState},
    scroll::ScrollableElement,
    searchable_list::SearchableVec,
    select::{Select, SelectState},
    sidebar::{
        Sidebar, SidebarCollapsible, SidebarFooter, SidebarGroup, SidebarHeader, SidebarMenu,
        SidebarMenuItem,
    },
    spinner::Spinner,
    tag::Tag,
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

enum UiEvent {
    Message(ChatMessage),
    Approval(alva_host_native::middleware::ApprovalRequest),
}

#[derive(Clone, Copy, PartialEq)]
enum View {
    Chat,
    Skills,
    Mcp,
    Inspector,
}

/// Fallback provider list when ~/.alva/config.json has no providers.
const FALLBACK_PROVIDERS: [&str; 3] = ["anthropic", "openai-responses", "gemini"];

/// Provider choices: keys from ~/.alva/config.json (profiles + legacy
/// kind-keyed entries), falling back to the static list when unconfigured.
fn provider_options() -> Vec<String> {
    let mut from_config: Vec<String> = alva_app_core::config::load()
        .map(|cfg| cfg.providers.keys().cloned().collect())
        .unwrap_or_default();
    if from_config.is_empty() {
        from_config = FALLBACK_PROVIDERS.iter().map(|s| s.to_string()).collect();
    }
    from_config.sort();
    from_config
}

struct ChatApp {
    service: Arc<tokio::sync::Mutex<AgentService>>,
    session_manager: Arc<SqliteEvalSessionManager>,
    sessions: Vec<SessionSummary>,
    active_session_id: Option<String>,
    tokio: tokio::runtime::Handle,
    provider: Entity<SelectState<SearchableVec<String>>>,
    model: Entity<InputState>,
    api_key: Entity<InputState>,
    prompt: Entity<InputState>,
    messages: Vec<ChatMessage>,
    busy: bool,
    error: Option<String>,
    // Settings surface.
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
    // Gateway.
    gateway_port: Entity<InputState>,
    gateway_abort: Option<tokio::task::AbortHandle>,
    gateway_status: Option<String>,
    // Approvals.
    pending_approvals: Vec<alva_host_native::middleware::ApprovalRequest>,
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
                let options = provider_options();
                let active = alva_app_core::config::load()
                    .and_then(|cfg| cfg.active)
                    .unwrap_or_else(|| "anthropic".to_string());
                let index = options
                    .iter()
                    .position(|option| option == &active)
                    .unwrap_or(0);
                SelectState::new(
                    SearchableVec::new(options),
                    Some(IndexPath::new(index)),
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
            gateway_port: cx.new(|cx| InputState::new(window, cx).placeholder("8080")),
            gateway_abort: None,
            gateway_status: None,
            pending_approvals: Vec::new(),
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

    fn start_gateway(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let req = self.request(cx);
        let port: u16 = self
            .gateway_port
            .read(cx)
            .value()
            .trim()
            .parse()
            .unwrap_or(8080);
        if let Some(old) = self.gateway_abort.take() {
            old.abort();
        }
        let handle = self.tokio.clone();
        let handle_inner = handle.clone();
        let weak = cx.entity().downgrade();
        let (tx, mut rx) =
            tokio::sync::mpsc::unbounded_channel::<Result<tokio::task::AbortHandle, String>>();
        handle.spawn(async move {
            match service::build_provider_config(&req) {
                Ok(config) => {
                    let alias = config.model.clone();
                    let mut router = alva_llm_provider::AliasRouter::new();
                    router.insert(alias, config);
                    let addr = format!("127.0.0.1:{port}");
                    let join = handle_inner.spawn(async move {
                        if let Err(e) = alva_app_gateway::serve(router, &addr).await {
                            tracing::error!(error = %e, "embedded gateway exited with error");
                        }
                    });
                    let _ = tx.send(Ok(join.abort_handle()));
                }
                Err(error) => {
                    let _ = tx.send(Err(error));
                }
            }
        });
        cx.spawn(async move |_this, cx| {
            while let Some(result) = rx.recv().await {
                if let Some(hub) = weak.upgrade() {
                    hub.update(cx, |hub, cx| {
                        match result {
                            Ok(abort) => {
                                hub.gateway_abort = Some(abort);
                                hub.gateway_status =
                                    Some(format!("Gateway running at http://127.0.0.1:{port}"));
                            }
                            Err(error) => {
                                hub.gateway_status = Some(format!("Gateway failed: {error}"));
                            }
                        }
                        cx.notify();
                    });
                }
            }
        })
        .detach();
        self.gateway_status = Some(format!("Starting gateway on 127.0.0.1:{port}…"));
        cx.notify();
    }

    fn stop_gateway(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.gateway_abort.take() {
            handle.abort();
        }
        self.gateway_status = Some("Gateway stopped".to_string());
        cx.notify();
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

    fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.load_settings_into_inputs(window, cx);
        let weak = cx.entity().downgrade();
        window.open_sheet(cx, move |sheet, window, cx| {
            let Some(entity) = weak.upgrade() else {
                return sheet;
            };
            entity.update(cx, |app, cx| {
                sheet
                    .size(px(440.))
                    .title("Provider settings")
                    .child(app.settings_panel(window, cx))
            })
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
        let service_arc = Arc::clone(&self.service);
        let weak = cx.entity().downgrade();
        let handle = self.tokio.clone();
        let handle_inner = handle.clone();
        let (ui_tx, mut ui_rx) = tokio::sync::mpsc::unbounded_channel::<UiEvent>();
        handle.spawn(async move {
            let mut service = service.lock().await;
            let drain_tx = ui_tx.clone();
            let drain_service = Arc::clone(&service_arc);
            handle_inner.spawn(async move {
                let drain_svc = drain_service.lock().await;
                let (approval_tx, mut approval_rx) = tokio::sync::mpsc::unbounded_channel::<
                    alva_host_native::middleware::ApprovalRequest,
                >();
                drain_svc.drain_approvals(approval_tx).await;
                while let Some(request) = approval_rx.recv().await {
                    if drain_tx.send(UiEvent::Approval(request)).is_err() {
                        break;
                    }
                }
            });
            let result = service.send(req, &session_id, &text).await;
            let mut receiver = match result {
                Ok(rx) => rx,
                Err(error) => {
                    let _ = ui_tx.send(UiEvent::Message(ChatMessage {
                        role: ChatRole::Assistant,
                        text: format!("Error: {error}"),
                    }));
                    return;
                }
            };
            while let Some(event) = receiver.recv().await {
                use alva_kernel_core::event::AgentEvent;
                match event {
                    AgentEvent::MessageStart { message } => {
                        let text = extract_text(&message);
                        let _ = ui_tx.send(UiEvent::Message(ChatMessage {
                            role: ChatRole::Assistant,
                            text,
                        }));
                    }
                    AgentEvent::MessageUpdate { message, .. } => {
                        let text = extract_text(&message);
                        let _ = ui_tx.send(UiEvent::Message(ChatMessage {
                            role: ChatRole::Assistant,
                            text,
                        }));
                    }
                    AgentEvent::MessageEnd { message } => {
                        let text = extract_text(&message);
                        let _ = ui_tx.send(UiEvent::Message(ChatMessage {
                            role: ChatRole::Assistant,
                            text,
                        }));
                    }
                    AgentEvent::AgentEnd { error: Some(error) } => {
                        let _ = ui_tx.send(UiEvent::Message(ChatMessage {
                            role: ChatRole::Assistant,
                            text: format!("Error: {error}"),
                        }));
                    }
                    AgentEvent::AgentEnd { error: None } => {}
                    _ => {}
                }
            }
        });
        cx.spawn(async move |_this, cx| {
            while let Some(event) = ui_rx.recv().await {
                if let Some(hub) = weak.upgrade() {
                    hub.update(cx, |hub, cx| {
                        match event {
                            UiEvent::Message(message) => hub.upsert_assistant(message),
                            UiEvent::Approval(request) => {
                                if !hub
                                    .pending_approvals
                                    .iter()
                                    .any(|p| p.request_id == request.request_id)
                                {
                                    hub.pending_approvals.push(request);
                                }
                            }
                        }
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
            .gap_4()
            .child(
                GroupBox::new()
                    .outline()
                    .title("Connection")
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
                                        if let Some(entity) = weak.upgrade() {
                                            entity.update(cx, |hub, cx| {
                                                hub.save_settings(window, cx);
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
                    ),
            )
            .child(
                GroupBox::new()
                    .outline()
                    .title("Embedded gateway")
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(Input::new(&self.gateway_port).small())
                            .child(
                                Button::new("start-gateway")
                                    .secondary()
                                    .small()
                                    .label("Start")
                                    .on_click({
                                        let weak = cx.entity().downgrade();
                                        move |_, window, cx| {
                                            if let Some(entity) = weak.upgrade() {
                                                entity.update(cx, |hub, cx| {
                                                    hub.start_gateway(window, cx);
                                                });
                                            }
                                        }
                                    }),
                            )
                            .child(
                                Button::new("stop-gateway")
                                    .ghost()
                                    .small()
                                    .label("Stop")
                                    .on_click({
                                        let weak = cx.entity().downgrade();
                                        move |_, _, cx| {
                                            if let Some(entity) = weak.upgrade() {
                                                entity.update(cx, |hub, cx| {
                                                    hub.stop_gateway(cx);
                                                });
                                            }
                                        }
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().colors.muted_foreground)
                            .child(self.gateway_status.clone().unwrap_or_else(|| {
                                "Exposes the configured provider via OpenAI-compatible routes."
                                    .to_string()
                            })),
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
                v_flex()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .child(
                        Checkbox::new(SharedString::from(format!("tool-{name}")))
                            .label(name.clone())
                            .checked(checked)
                            .on_click(move |checked, _, cx| {
                                if let Some(entity) = weak.upgrade() {
                                    entity.update(cx, |hub, cx| {
                                        if *checked {
                                            hub.selected_tools.insert(name.clone());
                                        } else {
                                            hub.selected_tools.remove(&name);
                                        }
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().colors.muted_foreground)
                            .child(description),
                    ),
            );
        }
        rows
    }

    fn resolve_approval(&mut self, request_id: &str, allow: bool, cx: &mut Context<Self>) {
        let service = Arc::clone(&self.service);
        let handle = self.tokio.clone();
        let request_id = request_id.to_string();
        let tool_name = self
            .pending_approvals
            .iter()
            .find(|p| p.request_id == request_id)
            .map(|p| p.tool_name.clone())
            .unwrap_or_default();
        self.pending_approvals
            .retain(|p| p.request_id != request_id);
        let weak = cx.entity().downgrade();
        handle.spawn(async move {
            let mut service = service.lock().await;
            let _ = service.resolve(&request_id, &tool_name, allow).await;
            if let Some(hub) = weak.upgrade() {
                let _ = hub;
            }
        });
        cx.notify();
    }

    fn approval_cards(&self, cx: &Context<Self>) -> impl IntoElement {
        let approvals = self.pending_approvals.clone();
        let weak = cx.entity().downgrade();
        div()
            .v_flex()
            .gap_2()
            .children(approvals.iter().map(|request| {
                let weak = weak.clone();
                let request_id = request.request_id.clone();
                let tool_name = request.tool_name.clone();
                let arguments = request
                    .arguments
                    .to_string()
                    .chars()
                    .take(120)
                    .collect::<String>();
                v_flex()
                    .gap_2()
                    .child(
                        Alert::warning(
                            SharedString::from(format!("approval-{request_id}")),
                            arguments,
                        )
                        .small()
                        .title(format!("Approval needed: {tool_name}")),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new(SharedString::from(format!("allow-{request_id}")))
                                    .small()
                                    .label("Allow")
                                    .on_click({
                                        let weak = weak.clone();
                                        let request_id = request_id.clone();
                                        move |_, _, cx| {
                                            if let Some(entity) = weak.upgrade() {
                                                entity.update(cx, |hub, cx| {
                                                    hub.resolve_approval(&request_id, true, cx);
                                                });
                                            }
                                        }
                                    }),
                            )
                            .child(
                                Button::new(SharedString::from(format!("deny-{request_id}")))
                                    .small()
                                    .label("Deny")
                                    .on_click({
                                        let weak = weak.clone();
                                        let request_id = request_id.clone();
                                        move |_, _, cx| {
                                            if let Some(entity) = weak.upgrade() {
                                                entity.update(cx, |hub, cx| {
                                                    hub.resolve_approval(&request_id, false, cx);
                                                });
                                            }
                                        }
                                    }),
                            ),
                    )
                    .into_any_element()
            }))
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
                        h_flex()
                            .gap_1()
                            .items_center()
                            .when(busy, |this| this.child(Spinner::new().xsmall()))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().colors.muted_foreground)
                                    .child(if busy {
                                        "Alva is working…"
                                    } else {
                                        "Enter to send"
                                    }),
                            ),
                    )
                    .child(
                        Button::new("send")
                            .primary()
                            .label("Send")
                            .loading(busy)
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
                            .icon(IconName::Settings)
                            .label("Settings")
                            .tooltip("Open provider and gateway settings")
                            .on_click({
                                let weak = cx.entity().downgrade();
                                move |_, window, cx| {
                                    if let Some(entity) = weak.upgrade() {
                                        entity.update(cx, |hub, cx| {
                                            hub.open_settings(window, cx);
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
        let session_items = if sessions.is_empty() {
            vec![SidebarMenuItem::new("No sessions yet.").disable(true)]
        } else {
            sessions
                .iter()
                .map(|session| {
                    let weak = weak.clone();
                    let id = session.session_id.clone();
                    let short_id = id.chars().take(8).collect::<String>();
                    let preview = session.preview.clone();
                    let is_active = active.as_deref() == Some(session.session_id.as_str());
                    SidebarMenuItem::new(preview)
                        .icon(IconName::Inbox)
                        .active(is_active)
                        .suffix(move |_, _| {
                            Tag::secondary()
                                .small()
                                .rounded_full()
                                .child(short_id.clone())
                        })
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
                })
                .collect::<Vec<_>>()
        };

        Sidebar::new("app-sidebar")
            .w(px(260.))
            .collapsible(SidebarCollapsible::None)
            .header(
                SidebarHeader::new()
                    .child(Avatar::new().name("ALVA").placeholder(IconName::Bot))
                    .child(
                        v_flex()
                            .flex_1()
                            .gap_0()
                            .child(div().font_weight(FontWeight(700.)).child("ALVA"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().colors.muted_foreground)
                                    .child("Native agent desktop"),
                            ),
                    )
                    .child(
                        Button::new("new-session")
                            .secondary()
                            .xsmall()
                            .icon(IconName::Plus)
                            .tooltip("Start a new session")
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
                SidebarGroup::new("Workspace").child(SidebarMenu::new().children([
                    self.nav_item("Chat", IconName::Bot, View::Chat, cx),
                    self.nav_item("Skills", IconName::BookOpen, View::Skills, cx),
                    self.nav_item("MCP", IconName::Network, View::Mcp, cx),
                    self.nav_item("Inspector", IconName::Inspector, View::Inspector, cx),
                ])),
            )
            .child(SidebarGroup::new("Sessions").child(SidebarMenu::new().children(session_items)))
            .footer(
                SidebarFooter::new()
                    .child(Tag::info().small().rounded_full().child("GPUI"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().colors.muted_foreground)
                            .child("Local-first"),
                    ),
            )
    }

    fn nav_item(
        &self,
        label: &str,
        icon: IconName,
        view: View,
        cx: &Context<Self>,
    ) -> SidebarMenuItem {
        let weak = cx.entity().downgrade();
        let view_key = view;
        SidebarMenuItem::new(label)
            .icon(icon)
            .active(self.view == view_key)
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
                        GroupBox::new()
                            .outline()
                            .title(skill.name.clone())
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
                        GroupBox::new()
                            .outline()
                            .title(server.name.clone())
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
                        GroupBox::new()
                            .outline()
                            .title(
                                h_flex()
                                    .gap_2()
                                    .child(format!("Turn {}", turn.turn_number))
                                    .child(
                                        Tag::secondary()
                                            .small()
                                            .rounded_full()
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
                            .child(self.approval_cards(cx))
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
                    window_min_size: Some(size(px(860.), px(600.))),
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
