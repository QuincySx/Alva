// INPUT:  gpui, gpui_component (Input, InputState, InputEvent, Button, ButtonVariants),
//         crate::models (AppSettings, LlmSettings, ProxySettings, SettingsModel, ThemeMode), crate::theme
// OUTPUT: pub struct SettingsPanel
// POS:    Settings form using gpui-component Input fields for LLM config, proxy toggle, and theme selection.
use gpui::{prelude::*, Context, Entity, FontWeight, Render, Subscription, Window, div};

use gpui_component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_component::input::{InputEvent, InputState, Input};
use gpui_component::{Disableable, Sizable};

use crate::models::{AppSettings, LlmSettings, ProxySettings, SettingsModel, ThemeMode};
use crate::theme::Theme;

pub struct SettingsPanel {
    settings_model: Entity<SettingsModel>,
    api_key_input: Entity<InputState>,
    base_url_input: Entity<InputState>,
    model_input: Entity<InputState>,
    proxy_url_input: Entity<InputState>,
    draft_proxy_enabled: bool,
    draft_theme: ThemeMode,
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

        // Create input states for each field
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

        // Subscribe to changes on each input to mark dirty
        subscriptions.push(cx.subscribe_in(
            &api_key_input, window,
            |this, _state, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.mark_dirty(cx);
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &base_url_input, window,
            |this, _state, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.mark_dirty(cx);
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &model_input, window,
            |this, _state, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.mark_dirty(cx);
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &proxy_url_input, window,
            |this, _state, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.mark_dirty(cx);
                }
            },
        ));

        // Subscribe to settings model changes (external updates)
        subscriptions.push(cx.subscribe(&settings_model, |this, model, _event, cx| {
            let settings = model.read(cx).settings.clone();
            this.load_from_settings(&settings);
            cx.notify();
        }));

        let mut panel = Self {
            settings_model,
            api_key_input,
            base_url_input,
            model_input,
            proxy_url_input,
            draft_proxy_enabled: settings.proxy.enabled,
            draft_theme: settings.theme,
            dirty: false,
            _subscriptions: subscriptions,
        };
        panel.draft_proxy_enabled = settings.proxy.enabled;
        panel.draft_theme = settings.theme;
        panel
    }

    fn load_from_settings(&mut self, settings: &AppSettings) {
        // Note: We cannot call set_value here since we don't have window access.
        // The input states will be refreshed on next render if needed.
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
            llm: LlmSettings {
                api_key,
                base_url,
                model,
            },
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
}

impl Render for SettingsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window, cx);
        let text_color = theme.text;
        let text_muted = theme.text_muted;
        let dirty = self.dirty;
        let proxy_enabled = self.draft_proxy_enabled;
        let current_theme = self.draft_theme;

        div()
            .id("settings-panel")
            .flex()
            .flex_col()
            .flex_1()
            .p_3()
            .gap_4()
            .overflow_y_scroll()
            // -- LLM Configuration Section --
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::BOLD)
                    .text_color(text_color)
                    .child("LLM Configuration"),
            )
            // API Key
            .child(self.render_input_field("API Key", &self.api_key_input.clone(), &theme))
            // Base URL
            .child(self.render_input_field("Base URL", &self.base_url_input.clone(), &theme))
            // Model
            .child(self.render_input_field("Model", &self.model_input.clone(), &theme))
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
                        Button::new("proxy-toggle")
                            .label(if proxy_enabled { "ON" } else { "OFF" })
                            .with_variant(if proxy_enabled {
                                ButtonVariant::Primary
                            } else {
                                ButtonVariant::Secondary
                            })
                            .small()
                            .on_click(cx.listener(srow_debug::traced_listener!("settings:proxy_toggle", |this, _, _, cx| {
                                this.draft_proxy_enabled = !this.draft_proxy_enabled;
                                this.mark_dirty(cx);
                            })))
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(text_muted)
                            .child("Enable HTTP/SOCKS5 proxy"),
                    ),
            )
            // Proxy URL (only shown when enabled)
            .when(proxy_enabled, |el| {
                el.child(self.render_input_field("Proxy URL", &self.proxy_url_input.clone(), &theme))
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
                        Button::new("save-btn")
                            .label(if dirty { "Save Settings" } else { "Settings Saved" })
                            .primary()
                            .disabled(!dirty)
                            .on_click(cx.listener(srow_debug::traced_listener!("settings:save", |this, _, _, cx| {
                                if this.dirty {
                                    this.save(cx);
                                }
                            })))
                    ),
            )
    }
}

impl SettingsPanel {
    fn render_input_field(
        &self,
        label: &str,
        input_state: &Entity<InputState>,
        theme: &Theme,
    ) -> impl IntoElement {
        let text_muted = theme.text_muted;

        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .text_color(text_muted)
                    .child(label.to_string()),
            )
            .child(
                Input::new(input_state)
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
            .on_click(cx.listener(srow_debug::traced_listener!("settings:theme_mode", move |this, _, _, cx| {
                this.draft_theme = mode;
                this.mark_dirty(cx);
            })))
    }
}
