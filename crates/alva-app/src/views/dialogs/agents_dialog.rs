// INPUT:  gpui, gpui_component (Button, ButtonVariants, Input, InputState, InputEvent, Dialog, WindowExt), crate::theme::Theme
// OUTPUT: pub struct AgentsDialogContent
// POS:    GPUI view for agents management with list/edit modes inside a Dialog.
use gpui::{prelude::*, App, Context, Entity, FontWeight, Render, Subscription, Window, div, px};

use gpui_component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::Sizable;

use crate::theme::Theme;

/// Data for a single agent displayed in the dialog.
#[derive(Clone)]
struct AgentViewData {
    name: String,
    description: String,
    model: String,
    system_prompt: String,
    skills: Vec<String>,
}

/// GPUI view that renders agents list/edit content.
/// Meant to be displayed inside a gpui-component Dialog via `open_dialog`.
pub struct AgentsDialogContent {
    agents: Vec<AgentViewData>,
    #[allow(dead_code)]
    search_query: String,
    editing_index: Option<usize>,
    // Edit form state
    edit_name_input: Entity<InputState>,
    edit_desc_input: Entity<InputState>,
    edit_model_input: Entity<InputState>,
    edit_prompt_input: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl AgentsDialogContent {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let edit_name_input = cx.new(|cx| InputState::new(window, cx).placeholder("Agent name..."));
        let edit_desc_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Description..."));
        let edit_model_input = cx.new(|cx| InputState::new(window, cx).placeholder("gpt-4o"));
        let edit_prompt_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("System prompt..."));

        let mut subscriptions = Vec::new();

        // Track search query changes (we'll add search input for list mode later if needed)
        // For now just track edits
        for input in [
            &edit_name_input,
            &edit_desc_input,
            &edit_model_input,
            &edit_prompt_input,
        ] {
            let input_clone = input.clone();
            subscriptions.push(cx.subscribe_in(
                &input_clone,
                window,
                |_this, _state, event: &InputEvent, _window, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                },
            ));
        }

        // Sample data
        let agents = vec![
            AgentViewData {
                name: "Main Agent".into(),
                description: "Primary assistant for general tasks".into(),
                model: "gpt-4o".into(),
                system_prompt: "You are a helpful assistant.".into(),
                skills: vec!["web-search".into(), "code-gen".into()],
            },
            AgentViewData {
                name: "CodeReview".into(),
                description: "Specialized code review agent".into(),
                model: "gpt-4o".into(),
                system_prompt: "You are a code reviewer. Analyze code for bugs and style issues."
                    .into(),
                skills: vec!["code-analysis".into()],
            },
        ];

        Self {
            agents,
            search_query: String::new(),
            editing_index: None,
            edit_name_input,
            edit_desc_input,
            edit_model_input,
            edit_prompt_input,
            _subscriptions: subscriptions,
        }
    }

    fn start_edit(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(agent) = self.agents.get(index) {
            self.edit_name_input
                .update(cx, |s, cx| s.set_value(&agent.name, window, cx));
            self.edit_desc_input
                .update(cx, |s, cx| s.set_value(&agent.description, window, cx));
            self.edit_model_input
                .update(cx, |s, cx| s.set_value(&agent.model, window, cx));
            self.edit_prompt_input
                .update(cx, |s, cx| s.set_value(&agent.system_prompt, window, cx));
            self.editing_index = Some(index);
            cx.notify();
        }
    }

    fn start_create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Add a new blank agent and edit it
        self.agents.push(AgentViewData {
            name: String::new(),
            description: String::new(),
            model: "gpt-4o".into(),
            system_prompt: String::new(),
            skills: vec![],
        });
        let idx = self.agents.len() - 1;
        self.start_edit(idx, window, cx);
    }

    fn save_edit(&mut self, cx: &mut Context<Self>) {
        if let Some(idx) = self.editing_index {
            if let Some(agent) = self.agents.get_mut(idx) {
                agent.name = self.edit_name_input.read(cx).value().trim().to_string();
                agent.description = self.edit_desc_input.read(cx).value().trim().to_string();
                agent.model = self.edit_model_input.read(cx).value().trim().to_string();
                agent.system_prompt =
                    self.edit_prompt_input.read(cx).value().trim().to_string();
            }
            self.editing_index = None;
            cx.notify();
        }
    }

    fn delete_current(&mut self, cx: &mut Context<Self>) {
        if let Some(idx) = self.editing_index {
            if idx < self.agents.len() {
                self.agents.remove(idx);
            }
            self.editing_index = None;
            cx.notify();
        }
    }

    fn back_to_list(&mut self, cx: &mut Context<Self>) {
        self.editing_index = None;
        cx.notify();
    }

    fn render_list(&mut self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let text_color = theme.text;
        let text_muted = theme.text_muted;
        let surface = theme.surface;
        let surface_hover = theme.surface_hover;
        let border = theme.border;
        let accent = theme.accent;

        let mut container = div().flex().flex_col().gap_2().w_full();

        // Create Agent button
        container = container.child(
            Button::new("create-agent-btn")
                .label("+ Create Agent")
                .primary()
                .small()
                .on_click(cx.listener(
                    alva_app_debug::traced_listener!(
                        "agents_dialog:create",
                        |this, _: &gpui::ClickEvent, window, cx| {
                            this.start_create(window, cx);
                        }
                    ),
                )),
        );

        if self.agents.is_empty() {
            return container.child(
                div()
                    .py_4()
                    .text_sm()
                    .text_color(text_muted)
                    .child("No agents configured. Create your first agent."),
            );
        }

        // Agent cards
        for (i, agent) in self.agents.iter().enumerate() {
            let agent_name = agent.name.clone();
            let agent_desc = agent.description.clone();
            let agent_model = agent.model.clone();
            let skills = agent.skills.clone();

            container = container.child(
                div()
                    .id(gpui::ElementId::Name(format!("agent-card-{}", i).into()))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_3()
                    .rounded_md()
                    .border_1()
                    .border_color(border)
                    .bg(surface)
                    .hover(|s| s.bg(surface_hover))
                    .cursor_pointer()
                    // Name
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(text_color)
                                    .child(agent_name),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(text_muted)
                                    .child(agent_model),
                            ),
                    )
                    // Description
                    .child(
                        div()
                            .text_xs()
                            .text_color(text_muted)
                            .child(agent_desc),
                    )
                    // Skills tags
                    .when(!skills.is_empty(), move |el| {
                        el.child(
                            div()
                                .flex()
                                .flex_row()
                                .flex_wrap()
                                .gap_1()
                                .mt_1()
                                .children(skills.into_iter().map(|s| {
                                    div()
                                        .px(px(6.))
                                        .py(px(2.))
                                        .rounded(px(4.))
                                        .bg(accent)
                                        .text_color(gpui::rgb(0xFFFFFF))
                                        .text_xs()
                                        .child(s)
                                })),
                        )
                    })
                    // Edit button at bottom
                    .child(
                        div()
                            .mt_1()
                            .child(
                                Button::new(gpui::ElementId::Name(
                                    format!("edit-agent-{}", i).into(),
                                ))
                                .label("Edit")
                                .small()
                                .outline()
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.start_edit(i, window, cx);
                                    },
                                )),
                            ),
                    ),
            );
        }

        container
    }

    fn render_edit(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let text_muted = theme.text_muted;

        div()
            .flex()
            .flex_col()
            .gap_3()
            .w_full()
            // Back button
            .child(
                Button::new("back-to-list-btn")
                    .label("\u{2190} Back")
                    .ghost()
                    .small()
                    .on_click(cx.listener(
                        alva_app_debug::traced_listener!(
                            "agents_dialog:back",
                            |this, _: &gpui::ClickEvent, _, cx| {
                                this.back_to_list(cx);
                            }
                        ),
                    )),
            )
            // Name
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(text_muted)
                            .child("Name"),
                    )
                    .child(Input::new(&self.edit_name_input)),
            )
            // Description
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(text_muted)
                            .child("Description"),
                    )
                    .child(Input::new(&self.edit_desc_input)),
            )
            // Model
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(text_muted)
                            .child("Model"),
                    )
                    .child(Input::new(&self.edit_model_input)),
            )
            // System Prompt
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(text_muted)
                            .child("System Prompt"),
                    )
                    .child(Input::new(&self.edit_prompt_input)),
            )
            // Skills (read-only tags for now)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(text_muted)
                            .child("Skills"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(text_muted)
                            .child("Skills management coming in Phase 2"),
                    ),
            )
            // Action buttons
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .mt_2()
                    .child(
                        Button::new("save-agent-btn")
                            .label("Save")
                            .primary()
                            .small()
                            .on_click(cx.listener(
                                alva_app_debug::traced_listener!(
                                    "agents_dialog:save",
                                    |this, _: &gpui::ClickEvent, _, cx| {
                                        this.save_edit(cx);
                                    }
                                ),
                            )),
                    )
                    .child(
                        Button::new("delete-agent-btn")
                            .label("Delete")
                            .with_variant(ButtonVariant::Danger)
                            .small()
                            .on_click(cx.listener(
                                alva_app_debug::traced_listener!(
                                    "agents_dialog:delete",
                                    |this, _: &gpui::ClickEvent, _, cx| {
                                        this.delete_current(cx);
                                    }
                                ),
                            )),
                    ),
            )
    }
}

impl Render for AgentsDialogContent {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window, cx);

        if self.editing_index.is_some() {
            div()
                .flex()
                .flex_col()
                .w_full()
                .child(self.render_edit(&theme, cx))
        } else {
            div()
                .flex()
                .flex_col()
                .w_full()
                .child(self.render_list(&theme, cx))
        }
    }
}

/// Opens the Agents management dialog using gpui-component's Dialog API.
pub fn open_agents_dialog(window: &mut Window, cx: &mut App) {
    use gpui_component::WindowExt as _;

    let content = cx.new(|cx| AgentsDialogContent::new(window, cx));

    window.open_dialog(cx, move |dialog, _window, _cx| {
        dialog
            .title("Agents")
            .width(px(560.))
            .child(content.clone())
    });
}
