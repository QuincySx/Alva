// INPUT:  gpui, gpui_component (Input, InputState, InputEvent, Button, ButtonVariants), crate::models (SettingsModel, etc.), crate::theme
// OUTPUT: pub struct SettingsPanel
// POS:    Categorized settings panel with left navigation and right content area.
use gpui::{prelude::*, Context, Entity, FontWeight, Hsla, Render, Subscription, Window, div, px};

use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{Icon, IconName};
use gpui_component::input::{InputEvent, InputState, Input};
use gpui_component::{Disableable, Sizable};

use crate::models::{AppSettings, LlmSettings, ProxySettings, SettingsModel, ThemeMode};
use crate::theme::Theme;

/// Settings category.
#[derive(Debug, Clone, Copy, PartialEq)]
enum SettingsCategory {
    General,
    AgentEngine,
    Model,
    ImBot,
    Email,
    Memory,
    Agent,
    Shortcuts,
    About,
}

impl SettingsCategory {
    fn icon(&self) -> IconName {
        match self {
            Self::General => IconName::Settings,
            Self::AgentEngine => IconName::SquareTerminal,
            Self::Model => IconName::Bot,
            Self::ImBot => IconName::Inbox,
            Self::Email => IconName::Bell,
            Self::Memory => IconName::BookOpen,
            Self::Agent => IconName::User,
            Self::Shortcuts => IconName::LayoutDashboard,
            Self::About => IconName::Info,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::General => "通用",
            Self::AgentEngine => "Agent 引擎",
            Self::Model => "模型",
            Self::ImBot => "IM 机器人",
            Self::Email => "邮箱",
            Self::Memory => "记忆",
            Self::Agent => "Agent",
            Self::Shortcuts => "快捷键",
            Self::About => "关于",
        }
    }

    fn all() -> &'static [Self] {
        &[
            Self::General,
            Self::AgentEngine,
            Self::Model,
            Self::ImBot,
            Self::Email,
            Self::Memory,
            Self::Agent,
            Self::Shortcuts,
            Self::About,
        ]
    }
}

pub struct SettingsPanel {
    settings_model: Entity<SettingsModel>,
    active_category: SettingsCategory,
    // General settings
    draft_proxy_enabled: bool,
    draft_theme: ThemeMode,
    draft_auto_start: bool,
    draft_prevent_sleep: bool,
    // Model settings
    api_key_input: Entity<InputState>,
    base_url_input: Entity<InputState>,
    model_input: Entity<InputState>,
    proxy_url_input: Entity<InputState>,
    dirty: bool,
    _subscriptions: Vec<Subscription>,
}

impl SettingsPanel {
    pub fn new(
        settings_model: Entity<SettingsModel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let settings = settings_model.read(cx).settings.clone();

        let api_key_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Enter API key...")
                .default_value(&settings.llm.api_key)
        });

        let base_url_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("https://api.openai.com/v1")
                .default_value(&settings.llm.base_url)
        });

        let model_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("gpt-4o")
                .default_value(&settings.llm.model)
        });

        let proxy_url_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("http://127.0.0.1:7890")
                .default_value(&settings.proxy.url)
        });

        let mut subscriptions = Vec::new();

        // Track input changes to mark dirty
        for input in [&api_key_input, &base_url_input, &model_input, &proxy_url_input] {
            let input_clone = input.clone();
            subscriptions.push(cx.subscribe_in(
                &input_clone,
                window,
                |this, _state, event: &InputEvent, _window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.mark_dirty(cx);
                    }
                },
            ));
        }

        // Subscribe to settings model changes
        subscriptions.push(cx.subscribe(&settings_model, |this, model, _event, cx| {
            let settings = model.read(cx).settings.clone();
            this.load_from_settings(&settings);
            cx.notify();
        }));

        Self {
            settings_model,
            active_category: SettingsCategory::General,
            draft_proxy_enabled: settings.proxy.enabled,
            draft_theme: settings.theme,
            draft_auto_start: false,
            draft_prevent_sleep: false,
            api_key_input,
            base_url_input,
            model_input,
            proxy_url_input,
            dirty: false,
            _subscriptions: subscriptions,
        }
    }

    fn load_from_settings(&mut self, settings: &AppSettings) {
        self.draft_proxy_enabled = settings.proxy.enabled;
        self.draft_theme = settings.theme;
        self.dirty = false;
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let api_key = self.api_key_input.read(cx).value().trim().to_string();
        let base_url = self.base_url_input.read(cx).value().trim().to_string();
        let model = self.model_input.read(cx).value().trim().to_string();
        let proxy_url = self.proxy_url_input.read(cx).value().trim().to_string();

        let new_settings = AppSettings {
            llm: LlmSettings { api_key, base_url, model },
            proxy: ProxySettings {
                enabled: self.draft_proxy_enabled,
                url: proxy_url,
            },
            theme: self.draft_theme,
        };
        self.settings_model.update(cx, |model, cx| {
            model.update_settings(new_settings, cx);
        });
        self.dirty = false;
        cx.notify();
    }

    fn mark_dirty(&mut self, cx: &mut Context<Self>) {
        self.dirty = true;
        cx.notify();
    }

    /// Render a nav item in the settings left sidebar.
    fn render_nav_item(
        &self,
        category: SettingsCategory,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_active = self.active_category == category;
        let accent = theme.accent;
        let selected_text = theme.selected_text;
        let text_color = theme.text;
        let text_muted = theme.text_muted;
        let surface_hover = theme.surface_hover;

        let icon_color: Hsla = if is_active {
            selected_text.into()
        } else {
            text_muted.into()
        };

        div()
            .id(gpui::ElementId::Name(format!("settings-nav-{:?}", category).into()))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .px(px(12.))
            .py(px(8.))
            .rounded(px(6.))
            .cursor_pointer()
            .text_sm()
            .when(is_active, move |el: gpui::Stateful<gpui::Div>| {
                el.bg(accent).text_color(selected_text)
            })
            .when(!is_active, move |el: gpui::Stateful<gpui::Div>| {
                el.text_color(text_muted)
                    .hover(move |s| s.bg(surface_hover).text_color(text_color))
            })
            .child(
                Icon::new(category.icon())
                    .small()
                    .text_color(icon_color),
            )
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .child(category.label()),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.active_category = category;
                cx.notify();
            }))
    }

    /// Render toggle button for boolean settings.
    fn render_toggle(
        id: &'static str,
        enabled: bool,
        theme: &Theme,
        on_toggle: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let accent = theme.accent;
        let surface_hover = theme.surface_hover;
        let border = theme.border;

        div()
            .id(gpui::ElementId::Name(id.into()))
            .flex()
            .items_center()
            .w(px(44.))
            .h(px(24.))
            .rounded(px(12.))
            .cursor_pointer()
            .when(enabled, move |el: gpui::Stateful<gpui::Div>| {
                el.bg(accent)
            })
            .when(!enabled, move |el: gpui::Stateful<gpui::Div>| {
                el.bg(surface_hover).border_1().border_color(border)
            })
            .child(
                div()
                    .size(px(18.))
                    .rounded_full()
                    .bg(gpui::rgb(0xFFFFFF))
                    .when(enabled, |el| el.ml(px(20.)))
                    .when(!enabled, |el| el.ml(px(2.))),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                on_toggle(this, cx);
            }))
    }

    /// Render a setting row with label, description, and control.
    fn render_setting_row<E: IntoElement>(
        label: &str,
        description: &str,
        theme: &Theme,
        control: E,
    ) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .py(px(12.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .flex_1()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .child(label.to_string()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.text_muted)
                            .child(description.to_string()),
                    ),
            )
            .child(control)
    }

    /// Render General settings category content.
    fn render_general(&mut self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let proxy_enabled = self.draft_proxy_enabled;
        let current_theme = self.draft_theme;
        let auto_start = self.draft_auto_start;
        let prevent_sleep = self.draft_prevent_sleep;

        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            // Section title
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme.text)
                    .pb(px(8.))
                    .child("通用"),
            )
            // Auto-start
            .child(Self::render_setting_row(
                "开机自启动",
                "系统启动时自动运行应用",
                theme,
                Self::render_toggle("toggle-autostart", auto_start, theme, |this, cx| {
                    this.draft_auto_start = !this.draft_auto_start;
                    this.mark_dirty(cx);
                }, cx),
            ))
            // Prevent sleep
            .child(Self::render_setting_row(
                "防止休眠",
                "防止系统在应用运行时进入睡眠模式",
                theme,
                Self::render_toggle("toggle-sleep", prevent_sleep, theme, |this, cx| {
                    this.draft_prevent_sleep = !this.draft_prevent_sleep;
                    this.mark_dirty(cx);
                }, cx),
            ))
            // System proxy
            .child(Self::render_setting_row(
                "使用系统代理",
                "开启后网络请求将跟随系统代理（保存后生效）",
                theme,
                Self::render_toggle("toggle-proxy", proxy_enabled, theme, |this, cx| {
                    this.draft_proxy_enabled = !this.draft_proxy_enabled;
                    this.mark_dirty(cx);
                }, cx),
            ))
            // Proxy URL (conditional)
            .when(proxy_enabled, |el| {
                el.child(
                    div()
                        .pl(px(4.))
                        .pt(px(4.))
                        .child(self.render_input_field("代理地址", &self.proxy_url_input.clone(), theme)),
                )
            })
            // Appearance
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .pt(px(12.))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .child("外观"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(8.))
                            .child(self.render_theme_card("浅色", ThemeMode::Light, current_theme, theme, cx))
                            .child(self.render_theme_card("深色", ThemeMode::Dark, current_theme, theme, cx))
                            .child(self.render_theme_card("跟随系统", ThemeMode::System, current_theme, theme, cx)),
                    ),
            )
    }

    /// Render Model settings category content.
    fn render_model(&self, theme: &Theme) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme.text)
                    .pb(px(4.))
                    .child("模型配置"),
            )
            .child(self.render_input_field("API Key", &self.api_key_input.clone(), theme))
            .child(self.render_input_field("Base URL", &self.base_url_input.clone(), theme))
            .child(self.render_input_field("Model", &self.model_input.clone(), theme))
    }

    /// Render a placeholder category content.
    fn render_placeholder(title: &str, description: &str, theme: &Theme) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme.text)
                    .child(title.to_string()),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.text_muted)
                    .child(description.to_string()),
            )
    }

    fn render_input_field(
        &self,
        label: &str,
        input_state: &Entity<InputState>,
        theme: &Theme,
    ) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_muted)
                    .child(label.to_string()),
            )
            .child(Input::new(input_state))
    }

    fn render_theme_card(
        &self,
        label: &str,
        mode: ThemeMode,
        current: ThemeMode,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_selected = current == mode;
        let accent = theme.accent;
        let card_bg = theme.card_bg;
        let card_border = theme.card_border;
        let text_color = theme.text;

        div()
            .id(gpui::ElementId::Name(format!("theme-{:?}", mode).into()))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .w(px(100.))
            .h(px(64.))
            .rounded(px(8.))
            .border_2()
            .cursor_pointer()
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .when(is_selected, move |el: gpui::Stateful<gpui::Div>| {
                el.border_color(accent)
                    .bg(card_bg)
                    .text_color(accent)
            })
            .when(!is_selected, move |el: gpui::Stateful<gpui::Div>| {
                el.border_color(card_border)
                    .bg(card_bg)
                    .text_color(text_color)
                    .hover(move |s| s.border_color(accent))
            })
            .child(label.to_string())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.draft_theme = mode;
                this.mark_dirty(cx);
            }))
    }
}

impl Render for SettingsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window, cx);
        let dirty = self.dirty;
        let active_category = self.active_category;

        div()
            .flex()
            .flex_col()
            .size_full()
            // Main content: left nav + right content
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .overflow_hidden()
                    // Left navigation
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .w(px(160.))
                            .flex_none()
                            .py(px(8.))
                            .px(px(8.))
                            .border_r_1()
                            .border_color(theme.border)
                            .bg(theme.surface)
                            .gap(px(2.))
                            .children(
                                SettingsCategory::all().iter().map(|cat| {
                                    self.render_nav_item(*cat, &theme, cx).into_any_element()
                                }),
                            ),
                    )
                    // Right content area
                    .child(
                        div()
                            .id("settings-content")
                            .flex()
                            .flex_col()
                            .flex_1()
                            .p(px(20.))
                            .overflow_y_scroll()
                            .child(match active_category {
                                SettingsCategory::General => {
                                    self.render_general(&theme, cx).into_any_element()
                                }
                                SettingsCategory::Model => {
                                    self.render_model(&theme).into_any_element()
                                }
                                SettingsCategory::AgentEngine => {
                                    Self::render_placeholder(
                                        "Agent 引擎",
                                        "引擎配置即将推出",
                                        &theme,
                                    ).into_any_element()
                                }
                                SettingsCategory::ImBot => {
                                    Self::render_placeholder(
                                        "IM 机器人",
                                        "IM 平台对接配置即将推出",
                                        &theme,
                                    ).into_any_element()
                                }
                                SettingsCategory::Email => {
                                    Self::render_placeholder(
                                        "邮箱",
                                        "邮箱通知配置即将推出",
                                        &theme,
                                    ).into_any_element()
                                }
                                SettingsCategory::Memory => {
                                    Self::render_placeholder(
                                        "记忆",
                                        "记忆系统配置即将推出",
                                        &theme,
                                    ).into_any_element()
                                }
                                SettingsCategory::Agent => {
                                    Self::render_placeholder(
                                        "Agent",
                                        "Agent 模板管理即将推出",
                                        &theme,
                                    ).into_any_element()
                                }
                                SettingsCategory::Shortcuts => {
                                    Self::render_placeholder(
                                        "快捷键",
                                        "快捷键配置即将推出",
                                        &theme,
                                    ).into_any_element()
                                }
                                SettingsCategory::About => {
                                    Self::render_placeholder(
                                        "关于",
                                        "Alva Agent v0.1.0",
                                        &theme,
                                    ).into_any_element()
                                }
                            }),
                    ),
            )
            // Footer: Cancel + Save
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_end()
                    .gap(px(8.))
                    .px(px(20.))
                    .py(px(12.))
                    .border_t_1()
                    .border_color(theme.border)
                    .child(
                        Button::new("cancel-btn")
                            .label("取消")
                            .outline()
                            .small(),
                    )
                    .child(
                        Button::new("save-settings-btn")
                            .label(if dirty { "保存" } else { "已保存" })
                            .primary()
                            .small()
                            .disabled(!dirty)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.dirty {
                                    this.save(cx);
                                }
                            })),
                    ),
            )
    }
}
