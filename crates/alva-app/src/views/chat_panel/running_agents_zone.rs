// INPUT:  gpui (prelude, Context, Entity, EventEmitter, FontWeight, Render, Subscription, Window, div, px),
//         crate::models::AgentModel, crate::types::AgentStatusKind, crate::theme::Theme,
//         super::agent_block::render_running_agent
// OUTPUT: pub struct RunningAgentsZone, pub enum RunningAgentsZoneEvent
// POS:    GPUI view showing currently running agents pinned above the chat input box.
use gpui::{prelude::*, Context, Entity, EventEmitter, FontWeight, Render, Subscription, Window, div, px};

use crate::models::AgentModel;
use crate::theme::Theme;
use crate::types::AgentStatusKind;
use super::agent_block::render_running_agent;

pub struct RunningAgentsZone {
    agent_model: Entity<AgentModel>,
    _subscription: Subscription,
}

pub enum RunningAgentsZoneEvent {
    AgentClicked { session_id: String },
}

impl EventEmitter<RunningAgentsZoneEvent> for RunningAgentsZone {}

impl RunningAgentsZone {
    pub fn new(
        agent_model: Entity<AgentModel>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.subscribe(&agent_model, |_this, _model, _event, cx| {
            cx.notify();
        });

        Self {
            agent_model,
            _subscription: subscription,
        }
    }
}

impl Render for RunningAgentsZone {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window, cx);
        let model = self.agent_model.read(cx);

        // Collect running/waiting agents
        let running_agents: Vec<_> = model
            .statuses
            .iter()
            .filter(|(_, status)| {
                matches!(
                    status.kind,
                    AgentStatusKind::Running | AgentStatusKind::WaitingHitl
                )
            })
            .map(|(id, status)| (id.clone(), status.clone()))
            .collect();

        if running_agents.is_empty() {
            // Empty div, zero height
            return div().into_any_element();
        }

        let mut container = div()
            .flex()
            .flex_col()
            .w_full()
            .px_4()
            .pb_2()
            // Divider with label
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .py_1()
                    .child(
                        div()
                            .flex_1()
                            .h(px(1.))
                            .bg(theme.border),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text_muted)
                            .child("Running Agents"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .h(px(1.))
                            .bg(theme.border),
                    ),
            );

        for (session_id, status) in running_agents {
            let detail = status.detail.as_deref().unwrap_or("").to_string();
            let sid = session_id.clone();

            container = container.child(
                div()
                    .id(gpui::ElementId::Name(format!("running-agent-{}", session_id).into()))
                    .py_1()
                    .on_click(cx.listener(alva_app_debug::traced_listener!(
                        "running_agents:agent_click",
                        move |_this, _: &gpui::ClickEvent, _, cx| {
                            tracing::info!(session_id = %sid, "running_agents: agent clicked");
                            cx.emit(RunningAgentsZoneEvent::AgentClicked {
                                session_id: sid.clone(),
                            });
                        }
                    )))
                    .child(render_running_agent(
                        &session_id,
                        &detail,
                        None,
                        &theme,
                    )),
            );
        }

        container.into_any_element()
    }
}
