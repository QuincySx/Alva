// INPUT:  Stateful Hub form entities
// OUTPUT: Reusable automation form composed from gpui-component fields
// POS:    Domain form layer shared by automation dialogs and future editors

use gpui::{App, Entity, IntoElement, ParentElement as _, RenderOnce, Window};
use gpui_component::{
    form::{field, v_form},
    input::{Input, InputState},
    searchable_list::SearchableVec,
    select::{Select, SelectState},
};

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
}
