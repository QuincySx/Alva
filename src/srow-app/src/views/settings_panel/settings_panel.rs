use gpui::{prelude::*, Context, Entity, FocusHandle, Focusable, FontWeight, Modifiers, Render, Window, div, px};

use crate::models::{AppSettings, LlmSettings, ProxySettings, SettingsModel, ThemeMode};
use crate::theme::Theme;

/// Which field is currently being edited
#[derive(Debug, Clone, Copy, PartialEq)]
enum EditingField {
    ApiKey,
    BaseUrl,
    Model,
    ProxyUrl,
}

pub struct SettingsPanel {
    focus_handle: FocusHandle,
    settings_model: Entity<SettingsModel>,
    /// Local draft of settings being edited
    draft_api_key: String,
    draft_base_url: String,
    draft_model: String,
    draft_proxy_enabled: bool,
    draft_proxy_url: String,
    draft_theme: ThemeMode,
    /// Which text field is currently being edited (for keyboard input)
    editing: Option<EditingField>,
    /// Whether draft has unsaved changes
    dirty: bool,
}

impl SettingsPanel {
    pub fn new(settings_model: Entity<SettingsModel>, cx: &mut Context<Self>) -> Self {
        let settings = settings_model.read(cx).settings.clone();

        cx.subscribe(&settings_model, |this, model, _event, cx| {
            let settings = model.read(cx).settings.clone();
            this.load_from_settings(&settings);
            cx.notify();
        })
        .detach();

        let mut panel = Self {
            focus_handle: cx.focus_handle(),
            settings_model,
            draft_api_key: String::new(),
            draft_base_url: String::new(),
            draft_model: String::new(),
            draft_proxy_enabled: false,
            draft_proxy_url: String::new(),
            draft_theme: ThemeMode::System,
            editing: None,
            dirty: false,
        };
        panel.load_from_settings(&settings);
        panel
    }

    fn load_from_settings(&mut self, settings: &AppSettings) {
        self.draft_api_key = settings.llm.api_key.clone();
        self.draft_base_url = settings.llm.base_url.clone();
        self.draft_model = settings.llm.model.clone();
        self.draft_proxy_enabled = settings.proxy.enabled;
        self.draft_proxy_url = settings.proxy.url.clone();
        self.draft_theme = settings.theme;
        self.dirty = false;
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let new_settings = AppSettings {
            llm: LlmSettings {
                api_key: self.draft_api_key.trim().to_string(),
                base_url: self.draft_base_url.trim().to_string(),
                model: self.draft_model.trim().to_string(),
            },
            proxy: ProxySettings {
                enabled: self.draft_proxy_enabled,
                url: self.draft_proxy_url.trim().to_string(),
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

    fn handle_key_input(&mut self, input: &str, cx: &mut Context<Self>) {
        if let Some(field) = self.editing {
            let target = match field {
                EditingField::ApiKey => &mut self.draft_api_key,
                EditingField::BaseUrl => &mut self.draft_base_url,
                EditingField::Model => &mut self.draft_model,
                EditingField::ProxyUrl => &mut self.draft_proxy_url,
            };
            target.push_str(input);
            self.mark_dirty(cx);
        }
    }

    fn handle_backspace(&mut self, cx: &mut Context<Self>) {
        if let Some(field) = self.editing {
            let target = match field {
                EditingField::ApiKey => &mut self.draft_api_key,
                EditingField::BaseUrl => &mut self.draft_base_url,
                EditingField::Model => &mut self.draft_model,
                EditingField::ProxyUrl => &mut self.draft_proxy_url,
            };
            target.pop();
            self.mark_dirty(cx);
        }
    }

    fn handle_space(&mut self, cx: &mut Context<Self>) {
        if let Some(field) = self.editing {
            let target = match field {
                EditingField::ApiKey => &mut self.draft_api_key,
                EditingField::BaseUrl => &mut self.draft_base_url,
                EditingField::Model => &mut self.draft_model,
                EditingField::ProxyUrl => &mut self.draft_proxy_url,
            };
            target.push(' ');
            self.mark_dirty(cx);
        }
    }
}

impl Focusable for SettingsPanel {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SettingsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window);
        let text_color = theme.text;
        let text_muted = theme.text_muted;
        let accent = theme.accent;
        let accent_hover = theme.accent_hover;
        let surface_hover = theme.surface_hover;

        let api_key_display = if self.draft_api_key.is_empty() {
            "Click to set API key...".to_string()
        } else {
            let key = &self.draft_api_key;
            if self.editing == Some(EditingField::ApiKey) {
                // Show full key when editing
                key.clone()
            } else if key.len() > 8 {
                format!("{}...{}", &key[..4], &key[key.len() - 4..])
            } else {
                "*".repeat(key.len())
            }
        };

        let base_url_display = if self.draft_base_url.is_empty() {
            "https://api.openai.com/v1".to_string()
        } else {
            self.draft_base_url.clone()
        };

        let model_display = if self.draft_model.is_empty() {
            "gpt-4o".to_string()
        } else {
            self.draft_model.clone()
        };

        let proxy_url_display = if self.draft_proxy_url.is_empty() {
            "http://127.0.0.1:7890".to_string()
        } else {
            self.draft_proxy_url.clone()
        };

        let editing = self.editing;
        let dirty = self.dirty;
        let proxy_enabled = self.draft_proxy_enabled;
        let current_theme = self.draft_theme;

        let is_editing_api_key = editing == Some(EditingField::ApiKey);
        let is_editing_base_url = editing == Some(EditingField::BaseUrl);
        let is_editing_model = editing == Some(EditingField::Model);
        let is_editing_proxy_url = editing == Some(EditingField::ProxyUrl);

        div()
            .id("settings-panel")
            .track_focus(&self.focus_handle)
            .flex()
            .flex_col()
            .flex_1()
            .p_3()
            .gap_4()
            .overflow_y_scroll()
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _window, cx| {
                let key = &event.keystroke.key;
                if key == "backspace" {
                    this.handle_backspace(cx);
                } else if key == "enter" || key == "escape" {
                    this.editing = None;
                    cx.notify();
                } else if key == "space" && event.keystroke.modifiers == Modifiers::none() {
                    this.handle_space(cx);
                } else {
                    if let Some(ref key_char) = event.keystroke.key_char {
                        this.handle_key_input(key_char, cx);
                    } else if key.len() == 1 && event.keystroke.modifiers == Modifiers::none() {
                        this.handle_key_input(key, cx);
                    }
                }
            }))
            // -- LLM Configuration Section --
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::BOLD)
                    .text_color(text_color)
                    .child("LLM Configuration"),
            )
            // API Key
            .child(self.render_field(
                "API Key",
                &api_key_display,
                is_editing_api_key,
                EditingField::ApiKey,
                &theme,
                cx,
            ))
            // Base URL
            .child(self.render_field(
                "Base URL",
                &base_url_display,
                is_editing_base_url,
                EditingField::BaseUrl,
                &theme,
                cx,
            ))
            // Model
            .child(self.render_field(
                "Model",
                &model_display,
                is_editing_model,
                EditingField::Model,
                &theme,
                cx,
            ))
            // -- Proxy Section --
            .child(
                div()
                    .mt_2()
                    .text_sm()
                    .font_weight(FontWeight::BOLD)
                    .text_color(text_color)
                    .child("Proxy"),
            )
            // Proxy enabled toggle
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id("proxy-toggle")
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(theme.border)
                            .cursor_pointer()
                            .text_xs()
                            .text_color(text_color)
                            .when(proxy_enabled, |el| el.bg(accent).text_color(gpui::white()))
                            .when(!proxy_enabled, |el| el.bg(theme.surface))
                            .child(if proxy_enabled { "ON" } else { "OFF" })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.draft_proxy_enabled = !this.draft_proxy_enabled;
                                this.mark_dirty(cx);
                            })),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(text_muted)
                            .child("Enable HTTP/SOCKS5 proxy"),
                    ),
            )
            // Proxy URL (only shown when enabled)
            .when(proxy_enabled, |el: gpui::Stateful<gpui::Div>| {
                el.child(self.render_field(
                    "Proxy URL",
                    &proxy_url_display,
                    is_editing_proxy_url,
                    EditingField::ProxyUrl,
                    &theme,
                    cx,
                ))
            })
            // -- Theme Section --
            .child(
                div()
                    .mt_2()
                    .text_sm()
                    .font_weight(FontWeight::BOLD)
                    .text_color(text_color)
                    .child("Appearance"),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_1()
                    .child(self.render_theme_button("System", ThemeMode::System, current_theme, &theme, cx))
                    .child(self.render_theme_button("Light", ThemeMode::Light, current_theme, &theme, cx))
                    .child(self.render_theme_button("Dark", ThemeMode::Dark, current_theme, &theme, cx)),
            )
            // -- Save Button --
            .child(
                div()
                    .mt_4()
                    .child(
                        div()
                            .id("save-btn")
                            .px_4()
                            .py_2()
                            .rounded_lg()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(dirty, |el| {
                                el.bg(accent)
                                    .text_color(gpui::white())
                                    .hover(move |style| style.bg(accent_hover))
                            })
                            .when(!dirty, |el| {
                                el.bg(surface_hover)
                                    .text_color(text_muted)
                                    .opacity(0.6)
                            })
                            .child(if dirty { "Save Settings" } else { "Settings Saved" })
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.dirty {
                                    this.save(cx);
                                }
                            })),
                    ),
            )
    }
}

impl SettingsPanel {
    fn render_field(
        &self,
        label: &str,
        display_value: &str,
        is_editing: bool,
        field: EditingField,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let text_color = theme.text;
        let text_muted = theme.text_muted;
        let surface = theme.surface;
        let border = theme.border;
        let accent = theme.accent;

        let label_str = label.to_string();
        let display_str = display_value.to_string();
        let field_id = format!("field-{:?}", field);

        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .text_color(text_muted)
                    .child(label_str),
            )
            .child(
                div()
                    .id(gpui::ElementId::Name(field_id.into()))
                    .px_2()
                    .py_1p5()
                    .rounded_md()
                    .border_1()
                    .when(is_editing, |el| el.border_color(accent))
                    .when(!is_editing, |el| el.border_color(border))
                    .bg(surface)
                    .text_sm()
                    .text_color(text_color)
                    .min_h(px(30.))
                    .cursor_text()
                    .child(
                        div()
                            .when(is_editing, |el| {
                                el.child(format!("{}|", display_str))
                            })
                            .when(!is_editing, |el| {
                                el.child(display_str.clone())
                            }),
                    )
                    .on_click(cx.listener(move |this, _, window, _cx| {
                        this.editing = Some(field);
                        this.focus_handle.focus(window);
                    })),
            )
    }

    fn render_theme_button(
        &self,
        label: &str,
        mode: ThemeMode,
        current: ThemeMode,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_selected = current == mode;
        let accent = theme.accent;
        let surface = theme.surface;
        let text_color = theme.text;
        let label_str = label.to_string();

        div()
            .id(gpui::ElementId::Name(format!("theme-{:?}", mode).into()))
            .px_3()
            .py_1()
            .rounded_md()
            .border_1()
            .cursor_pointer()
            .text_xs()
            .when(is_selected, |el| {
                el.border_color(accent)
                    .bg(accent)
                    .text_color(gpui::white())
            })
            .when(!is_selected, |el| {
                el.border_color(theme.border)
                    .bg(surface)
                    .text_color(text_color)
            })
            .child(label_str)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.draft_theme = mode;
                this.mark_dirty(cx);
            }))
    }
}
