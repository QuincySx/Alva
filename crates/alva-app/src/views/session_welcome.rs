// INPUT:  gpui, gpui_component (Button, Icon, IconName, Input, InputState, InputEvent), crate::models, crate::chat, crate::theme
// OUTPUT: pub struct SessionWelcomeView
// POS:    Welcome screen with logo, title, input, and quick action cards for sessions with no conversation yet.
use gpui::{prelude::*, Context, Entity, FontWeight, Render, Subscription, Window, div, px};

use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{Icon, IconName};
use gpui_component::input::{InputEvent, InputState, Input};
use gpui_component::{Disableable as _, Sizable};

use crate::chat::{GpuiChatConfig, GpuiChatEvent};
use crate::models::{AgentModel, ChatModel, SettingsModel, WorkspaceModel};
use crate::theme::Theme;
use crate::types::AgentStatusKind;

/// Quick action card definition.
struct QuickAction {
    id: &'static str,
    icon: IconName,
    label: &'static str,
    prompt: &'static str,
}

fn quick_actions() -> Vec<QuickAction> {
    vec![
        QuickAction {
            id: "qa-slides",
            icon: IconName::ChartPie,
            label: "制作幻灯片",
            prompt: "帮我制作一份关于项目进展的幻灯片",
        },
        QuickAction {
            id: "qa-data",
            icon: IconName::File,
            label: "数据分析",
            prompt: "帮我分析以下数据并生成报告",
        },
        QuickAction {
            id: "qa-learn",
            icon: IconName::BookOpen,
            label: "教育学习",
            prompt: "帮我整理学习资料",
        },
        QuickAction {
            id: "qa-web",
            icon: IconName::Globe,
            label: "创建网站",
            prompt: "帮我创建一个现代化的网站",
        },
    ]
}

pub struct SessionWelcomeView {
    workspace_model: Entity<WorkspaceModel>,
    chat_model: Entity<ChatModel>,
    agent_model: Entity<AgentModel>,
    #[allow(dead_code)]
    settings_model: Entity<SettingsModel>,
    input_state: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl SessionWelcomeView {
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
                .placeholder("分配一个任务或提问任何问题...")
        });

        let mut subscriptions = Vec::new();

        // Enter to send
        subscriptions.push(cx.subscribe_in(
            &input_state,
            window,
            |this, _state, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { secondary } = event {
                    if !secondary {
                        this.send_message(None, window, cx);
                    }
                }
            },
        ));

        Self {
            workspace_model,
            chat_model,
            agent_model,
            settings_model,
            input_state,
            _subscriptions: subscriptions,
        }
    }

    /// Send a message into the current selected session.
    fn send_message(
        &mut self,
        preset_prompt: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = match preset_prompt {
            Some(p) => p.to_string(),
            None => {
                let t = self.input_state.read(cx).value().trim().to_string();
                if t.is_empty() {
                    return;
                }
                t
            }
        };

        // Get the currently selected session
        let session_id = match self.workspace_model.read(cx).selected_session_id.clone() {
            Some(id) => id,
            None => return,
        };

        // Update session name to match the first message
        let task_name = if text.chars().count() > 20 {
            format!("{}...", text.chars().take(20).collect::<String>())
        } else {
            text.clone()
        };
        self.workspace_model.update(cx, |model, cx| {
            // Find and update the session name
            for item in &mut model.sidebar_items {
                match item {
                    crate::models::SidebarItem::GlobalSession(s) if s.id == session_id => {
                        s.name = task_name.clone();
                        s.status_text = Some("运行中".into());
                        break;
                    }
                    crate::models::SidebarItem::Workspace(w) => {
                        for s in &mut w.sessions {
                            if s.id == session_id {
                                s.name = task_name.clone();
                                s.status_text = Some("运行中".into());
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            }
            cx.notify();
        });

        // Create chat and send message
        let config = GpuiChatConfig {
            session_id: session_id.clone(),
        };
        let chat_entity = self.chat_model.update(cx, |model, cx| {
            model.get_or_create_chat(&session_id, config, cx)
        });

        // Mark agent as running
        self.agent_model.update(cx, |model, cx| {
            model.set_status(&session_id, AgentStatusKind::Running, cx);
        });

        // Subscribe to chat events for completion
        {
            let agent_model = self.agent_model.clone();
            let sid = session_id.clone();
            let sub = cx.subscribe(&chat_entity, move |_this, chat, _event: &GpuiChatEvent, cx| {
                let chat = chat.read(cx);
                if !chat.is_running() {
                    agent_model.update(cx, |model, cx| {
                        model.set_status(&sid, AgentStatusKind::Idle, cx);
                    });
                }
                cx.notify();
            });
            self._subscriptions.push(sub);
        }

        // Send
        chat_entity.update(cx, |chat, cx| {
            chat.send_message(&text, cx);
        });

        // Clear input
        self.input_state.update(cx, |state, cx| {
            state.set_value("", window, cx);
        });
        cx.notify();
    }

    /// Render a quick action card.
    fn render_quick_action(
        &self,
        action: &QuickAction,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let card_bg = theme.card_bg;
        let card_border = theme.card_border;
        let text_color = theme.text;
        let accent = theme.accent;
        let accent_subtle = theme.accent_subtle;
        let text_muted_hsla: gpui::Hsla = theme.text_muted.into();
        let prompt = action.prompt.to_string();

        div()
            .id(gpui::ElementId::Name(action.id.into()))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .w(px(120.))
            .h(px(80.))
            .rounded(px(10.))
            .border_1()
            .border_color(card_border)
            .bg(card_bg)
            .cursor_pointer()
            .hover(move |s| s.bg(accent_subtle).border_color(accent))
            .child(
                Icon::new(action.icon.clone())
                    .with_size(gpui_component::Size::Large)
                    .text_color(text_muted_hsla),
            )
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(text_color)
                    .child(action.label),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.send_message(Some(&prompt), window, cx);
            }))
    }
}

impl Render for SessionWelcomeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window, cx);

        div()
            .flex()
            .flex_col()
            .size_full()
            .items_center()
            .justify_center()
            .bg(theme.background)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(24.))
                    .max_w(px(600.))
                    .w_full()
                    .px(px(32.))
                    // Logo placeholder
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(64.))
                            .rounded(px(16.))
                            .bg(theme.accent)
                            .child(
                                div()
                                    .text_2xl()
                                    .text_color(theme.selected_text)
                                    .font_weight(FontWeight::BOLD)
                                    .child("A"),
                            ),
                    )
                    // Title
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(theme.text)
                                    .child("开始协作"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.text_muted)
                                    .child("7x24 小时帮你干活的全场景个人助理 Agent"),
                            ),
                    )
                    // Input box
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .flex_col()
                            .rounded(px(12.))
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.surface)
                            .overflow_hidden()
                            // Text input area
                            .child(
                                div()
                                    .p(px(12.))
                                    .child(Input::new(&self.input_state)),
                            )
                            // Toolbar
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .px(px(12.))
                                    .py(px(8.))
                                    .border_t_1()
                                    .border_color(theme.border_subtle)
                                    // Model selector (left)
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .gap(px(4.))
                                            .px(px(8.))
                                            .py(px(4.))
                                            .rounded(px(6.))
                                            .text_xs()
                                            .text_color(theme.text_muted)
                                            .cursor_pointer()
                                            .hover(|s| s.bg(theme.surface_hover))
                                            .child("Alva Agent")
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(theme.text_subtle)
                                                    .child("\u{25BE}"),
                                            ),
                                    )
                                    // Spacer
                                    .child(div().flex_1())
                                    // Attachment button
                                    .child(
                                        Button::new("welcome-attach-btn")
                                            .icon(Icon::new(IconName::Plus).small())
                                            .ghost()
                                            .small()
                                            .disabled(true),
                                    )
                                    // Skills button
                                    .child(
                                        Button::new("welcome-skills-btn")
                                            .icon(Icon::new(IconName::Star).small())
                                            .ghost()
                                            .small()
                                            .disabled(true),
                                    )
                                    // Send button
                                    .child(
                                        Button::new("welcome-send-btn")
                                            .label("发送")
                                            .primary()
                                            .small()
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.send_message(None, window, cx);
                                            })),
                                    ),
                            ),
                    )
                    // Quick action cards
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_center()
                            .gap(px(12.))
                            .children(
                                quick_actions().iter().map(|action| {
                                    self.render_quick_action(action, &theme, cx).into_any_element()
                                }),
                            ),
                    ),
            )
    }
}
