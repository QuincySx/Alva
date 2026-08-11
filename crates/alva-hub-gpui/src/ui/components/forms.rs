// INPUT:  Stateful Hub form entities
// OUTPUT: Reusable automation form composed from gpui-component fields
// POS:    Domain form layer shared by automation dialogs and future editors

use std::collections::HashSet;

use gpui::{App, Entity, IntoElement, ParentElement as _, RenderOnce, Styled, WeakEntity, Window};
use gpui_component::{
    checkbox::Checkbox,
    form::{field, v_form},
    h_flex,
    input::{Input, InputState},
    searchable_list::SearchableVec,
    select::{Select, SelectState},
};

use alva_hub_gpui::AgentRuntime;

use crate::ui::app::HubApp;

#[derive(IntoElement)]
pub struct HubAutomationForm {
    name: Entity<InputState>,
    prompt: Entity<InputState>,
    schedule: Entity<SelectState<SearchableVec<&'static str>>>,
}

impl HubAutomationForm {
    pub fn new(
        name: Entity<InputState>,
        prompt: Entity<InputState>,
        schedule: Entity<SelectState<SearchableVec<&'static str>>>,
    ) -> Self {
        Self {
            name,
            prompt,
            schedule,
        }
    }
}

impl RenderOnce for HubAutomationForm {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        v_form()
            .child(
                field()
                    .label("Name")
                    .required(true)
                    .child(Input::new(&self.name)),
            )
            .child(
                field()
                    .label("Prompt")
                    .required(true)
                    .description("Runs inside the selected project context.")
                    .child(Input::new(&self.prompt)),
            )
            .child(
                field()
                    .label("Schedule")
                    .required(true)
                    .child(Select::new(&self.schedule)),
            )
    }
}

#[derive(Clone, IntoElement)]
pub struct HubProviderForm {
    hub: WeakEntity<HubApp>,
    name: Entity<InputState>,
    provider_id: Entity<InputState>,
    base_url: Entity<InputState>,
    api_key: Entity<InputState>,
    model_id: Entity<InputState>,
    model_name: Entity<InputState>,
    context_window: Entity<InputState>,
    max_output: Entity<InputState>,
    protocol: Entity<SelectState<SearchableVec<&'static str>>>,
    runtimes: HashSet<AgentRuntime>,
    images: bool,
    tools: bool,
}

impl HubProviderForm {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        hub: WeakEntity<HubApp>,
        name: Entity<InputState>,
        provider_id: Entity<InputState>,
        base_url: Entity<InputState>,
        api_key: Entity<InputState>,
        model_id: Entity<InputState>,
        model_name: Entity<InputState>,
        context_window: Entity<InputState>,
        max_output: Entity<InputState>,
        protocol: Entity<SelectState<SearchableVec<&'static str>>>,
        runtimes: HashSet<AgentRuntime>,
        images: bool,
        tools: bool,
    ) -> Self {
        Self {
            hub,
            name,
            provider_id,
            base_url,
            api_key,
            model_id,
            model_name,
            context_window,
            max_output,
            protocol,
            runtimes,
            images,
            tools,
        }
    }
}

impl RenderOnce for HubProviderForm {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let mut runtime_controls = h_flex().flex_wrap().gap_3();
        for runtime in AgentRuntime::ALL {
            let hub = self.hub.clone();
            runtime_controls = runtime_controls.child(
                Checkbox::new(format!("provider-runtime-{}", runtime.id()))
                    .label(runtime.label())
                    .checked(self.runtimes.contains(&runtime))
                    .on_click(move |enabled, _, app| {
                        if let Some(hub) = hub.upgrade() {
                            hub.update(app, |hub, cx| {
                                hub.set_provider_runtime(runtime, *enabled, cx)
                            });
                        }
                    }),
            );
        }
        let images_hub = self.hub.clone();
        let tools_hub = self.hub.clone();
        v_form()
            .child(
                field()
                    .label("Provider name")
                    .required(true)
                    .child(Input::new(&self.name)),
            )
            .child(
                field()
                    .label("Provider ID")
                    .required(true)
                    .description("Stable lowercase ID used by session routing.")
                    .child(Input::new(&self.provider_id)),
            )
            .child(
                field()
                    .label("API protocol")
                    .required(true)
                    .child(Select::new(&self.protocol)),
            )
            .child(
                field()
                    .label("Base URL")
                    .required(true)
                    .child(Input::new(&self.base_url)),
            )
            .child(
                field()
                    .label("API key")
                    .required(true)
                    .description("Stored in an owner-only local file; never in Hub SQLite.")
                    .child(Input::new(&self.api_key)),
            )
            .child(
                field()
                    .label("Compatible agents")
                    .required(true)
                    .child(runtime_controls),
            )
            .child(
                field()
                    .label("Model ID")
                    .required(true)
                    .child(Input::new(&self.model_id)),
            )
            .child(
                field()
                    .label("Model name")
                    .child(Input::new(&self.model_name)),
            )
            .child(
                field()
                    .label("Context window")
                    .required(true)
                    .child(Input::new(&self.context_window)),
            )
            .child(
                field()
                    .label("Maximum output")
                    .required(true)
                    .child(Input::new(&self.max_output)),
            )
            .child(
                field().label("Capabilities").child(
                    h_flex()
                        .gap_3()
                        .child(
                            Checkbox::new("provider-images")
                                .label("Images")
                                .checked(self.images)
                                .on_click(move |enabled, _, app| {
                                    if let Some(hub) = images_hub.upgrade() {
                                        hub.update(app, |hub, cx| {
                                            hub.set_provider_capability("images", *enabled, cx)
                                        });
                                    }
                                }),
                        )
                        .child(
                            Checkbox::new("provider-tools")
                                .label("Tools")
                                .checked(self.tools)
                                .on_click(move |enabled, _, app| {
                                    if let Some(hub) = tools_hub.upgrade() {
                                        hub.update(app, |hub, cx| {
                                            hub.set_provider_capability("tools", *enabled, cx)
                                        });
                                    }
                                }),
                        ),
                ),
            )
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn automation_form_component_is_declared() {
        assert_eq!(
            std::any::type_name::<super::HubAutomationForm>()
                .split("::")
                .last(),
            Some("HubAutomationForm")
        );
    }

    #[test]
    fn provider_form_component_is_declared() {
        assert!(std::any::type_name::<super::HubProviderForm>().ends_with("HubProviderForm"));
    }
}
