// INPUT:  (re-exports submodule: agent_panel)
// OUTPUT: pub use agent_panel::* (AgentPanel)
// POS:    Barrel module for the agent panel, re-exporting the AgentPanel view.
pub mod agent_panel;

pub use agent_panel::*;
