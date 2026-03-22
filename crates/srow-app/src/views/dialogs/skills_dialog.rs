// INPUT:  gpui, gpui_component (Button, ButtonVariants, Input, InputState, InputEvent, Dialog, WindowExt),
//         crate::theme::Theme
// OUTPUT: pub struct SkillsDialogContent
// POS:    GPUI view for skills management — list/edit modes inside a Dialog.
use gpui::{prelude::*, App, Context, Entity, FontWeight, Render, Subscription, Window, div, px};

use gpui_component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{Disableable, Sizable};

use crate::theme::Theme;

/// Data for a single skill displayed in the dialog.
#[derive(Clone)]
struct SkillViewData {
    name: String,
    description: String,
    version: Option<String>,
    source: String,
    used_by: Vec<String>,
    update_available: bool,
}

/// GPUI view that renders skills list/edit content.
/// Meant to be displayed inside a gpui-component Dialog via `open_dialog`.
pub struct SkillsDialogContent {
    skills: Vec<SkillViewData>,
    #[allow(dead_code)]
    search_query: String,
    editing_index: Option<usize>,
    // Edit form state
    edit_name_input: Entity<InputState>,
    edit_desc_input: Entity<InputState>,
    edit_source_input: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl SkillsDialogContent {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let edit_name_input = cx.new(|cx| InputState::new(window, cx).placeholder("Skill name..."));
        let edit_desc_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Description..."));
        let edit_source_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("github:owner/repo#name or local:/path")
        });

        let mut subscriptions = Vec::new();

        for input in [&edit_name_input, &edit_desc_input, &edit_source_input] {
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
        let skills = vec![
            SkillViewData {
                name: "web-search".into(),
                description: "Search the web using various providers".into(),
                version: Some("1.2.0".into()),
                source: "github:srow-ai/skills#web-search".into(),
                used_by: vec!["Main Agent".into()],
                update_available: false,
            },
            SkillViewData {
                name: "code-gen".into(),
                description: "Generate code from natural language descriptions".into(),
                version: Some("0.9.1".into()),
                source: "github:srow-ai/skills#code-gen".into(),
                used_by: vec!["Main Agent".into()],
                update_available: true,
            },
            SkillViewData {
                name: "code-analysis".into(),
                description: "Analyze code for bugs, style issues, and complexity".into(),
                version: Some("1.0.0".into()),
                source: "local:/skills/code-analysis".into(),
                used_by: vec!["CodeReview".into()],
                update_available: false,
            },
        ];

        Self {
            skills,
            search_query: String::new(),
            editing_index: None,
            edit_name_input,
            edit_desc_input,
            edit_source_input,
            _subscriptions: subscriptions,
        }
    }

    fn start_edit(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(skill) = self.skills.get(index) {
            self.edit_name_input
                .update(cx, |s, cx| s.set_value(&skill.name, window, cx));
            self.edit_desc_input
                .update(cx, |s, cx| s.set_value(&skill.description, window, cx));
            self.edit_source_input
                .update(cx, |s, cx| s.set_value(&skill.source, window, cx));
            self.editing_index = Some(index);
            cx.notify();
        }
    }

    fn start_create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.skills.push(SkillViewData {
            name: String::new(),
            description: String::new(),
            version: None,
            source: String::new(),
            used_by: vec![],
            update_available: false,
        });
        let idx = self.skills.len() - 1;
        self.start_edit(idx, window, cx);
    }

    fn save_edit(&mut self, cx: &mut Context<Self>) {
        if let Some(idx) = self.editing_index {
            if let Some(skill) = self.skills.get_mut(idx) {
                skill.name = self.edit_name_input.read(cx).value().trim().to_string();
                skill.description = self.edit_desc_input.read(cx).value().trim().to_string();
                skill.source = self.edit_source_input.read(cx).value().trim().to_string();
            }
            self.editing_index = None;
            cx.notify();
        }
    }

    fn delete_current(&mut self, cx: &mut Context<Self>) {
        if let Some(idx) = self.editing_index {
            if idx < self.skills.len() {
                self.skills.remove(idx);
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
        let success = theme.success;
        let warning = theme.warning;

        let mut container = div().flex().flex_col().gap_2().w_full();

        // Action buttons row
        container = container.child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .child(
                    Button::new("create-skill-btn")
                        .label("+ Create Skill")
                        .primary()
                        .small()
                        .on_click(cx.listener(
                            srow_debug::traced_listener!(
                                "skills_dialog:create",
                                |this, _: &gpui::ClickEvent, window, cx| {
                                    this.start_create(window, cx);
                                }
                            ),
                        )),
                )
                .child(
                    Button::new("import-skill-btn")
                        .label("+ Import from GitHub")
                        .outline()
                        .small()
                        .disabled(true),
                ),
        );

        if self.skills.is_empty() {
            return container.child(
                div()
                    .py_4()
                    .text_sm()
                    .text_color(text_muted)
                    .child("No skills installed. Create or import a skill."),
            );
        }

        // Skill cards
        for (i, skill) in self.skills.iter().enumerate() {
            let skill_name = skill.name.clone();
            let skill_desc = skill.description.clone();
            let skill_source = skill.source.clone();
            let skill_version = skill.version.clone();
            let used_by = skill.used_by.clone();
            let has_update = skill.update_available;

            container = container.child(
                div()
                    .id(gpui::ElementId::Name(format!("skill-card-{}", i).into()))
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
                    // Name + Version row
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap_2()
                                    .items_center()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(text_color)
                                            .child(skill_name),
                                    )
                                    .when_some(skill_version, |el, ver| {
                                        el.child(
                                            div()
                                                .px(px(6.))
                                                .py(px(1.))
                                                .rounded(px(4.))
                                                .bg(accent)
                                                .text_color(gpui::rgb(0xFFFFFF))
                                                .text_xs()
                                                .child(format!("v{}", ver)),
                                        )
                                    }),
                            )
                            .when(has_update, |el| {
                                el.child(
                                    div()
                                        .px(px(6.))
                                        .py(px(1.))
                                        .rounded(px(4.))
                                        .bg(warning)
                                        .text_color(gpui::rgb(0x000000))
                                        .text_xs()
                                        .child("Update available"),
                                )
                            }),
                    )
                    // Description
                    .child(
                        div()
                            .text_xs()
                            .text_color(text_muted)
                            .child(skill_desc),
                    )
                    // Source
                    .child(
                        div()
                            .text_xs()
                            .text_color(text_muted)
                            .child(format!("Source: {}", skill_source)),
                    )
                    // Used by
                    .when(!used_by.is_empty(), move |el| {
                        el.child(
                            div()
                                .flex()
                                .flex_row()
                                .flex_wrap()
                                .gap_1()
                                .mt_1()
                                .children(used_by.into_iter().map(|agent| {
                                    div()
                                        .px(px(6.))
                                        .py(px(2.))
                                        .rounded(px(4.))
                                        .bg(success)
                                        .text_color(gpui::rgb(0x000000))
                                        .text_xs()
                                        .child(agent)
                                })),
                        )
                    })
                    // Edit button
                    .child(
                        div()
                            .mt_1()
                            .child(
                                Button::new(gpui::ElementId::Name(
                                    format!("edit-skill-{}", i).into(),
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
                Button::new("back-to-skills-list-btn")
                    .label("\u{2190} Back")
                    .ghost()
                    .small()
                    .on_click(cx.listener(
                        srow_debug::traced_listener!(
                            "skills_dialog:back",
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
            // Source
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(text_muted)
                            .child("Source"),
                    )
                    .child(Input::new(&self.edit_source_input)),
            )
            // Action buttons
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .mt_2()
                    .child(
                        Button::new("save-skill-btn")
                            .label("Save")
                            .primary()
                            .small()
                            .on_click(cx.listener(
                                srow_debug::traced_listener!(
                                    "skills_dialog:save",
                                    |this, _: &gpui::ClickEvent, _, cx| {
                                        this.save_edit(cx);
                                    }
                                ),
                            )),
                    )
                    .child(
                        Button::new("delete-skill-btn")
                            .label("Delete")
                            .with_variant(ButtonVariant::Danger)
                            .small()
                            .on_click(cx.listener(
                                srow_debug::traced_listener!(
                                    "skills_dialog:delete",
                                    |this, _: &gpui::ClickEvent, _, cx| {
                                        this.delete_current(cx);
                                    }
                                ),
                            )),
                    ),
            )
    }
}

impl Render for SkillsDialogContent {
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

/// Opens the Skills management dialog using gpui-component's Dialog API.
pub fn open_skills_dialog(window: &mut Window, cx: &mut App) {
    use gpui_component::WindowExt as _;

    let content = cx.new(|cx| SkillsDialogContent::new(window, cx));

    window.open_dialog(cx, move |dialog, _window, _cx| {
        dialog
            .title("Skills")
            .width(px(560.))
            .child(content.clone())
    });
}
