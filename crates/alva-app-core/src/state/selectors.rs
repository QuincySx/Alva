//! Derived queries on top of `AppState`.
//!
//! `Selectors` wraps a reference to `AppState` and provides computed properties
//! that would be verbose to inline everywhere.

use super::app_state::{AppState, TaskStatus};

/// Shared cost estimation (Claude-family heuristic: $3/M input, $15/M output).
pub fn estimate_cost_usd(input_tokens: u64, output_tokens: u64) -> f64 {
    (input_tokens as f64 * 3.0 + output_tokens as f64 * 15.0) / 1_000_000.0
}

/// Shared compact number formatter (e.g., 1500 → "1.5K", 2500000 → "2.5M").
pub fn format_token_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Read-only query helpers over an `AppState` snapshot.
pub struct Selectors<'a> {
    state: &'a AppState,
}

impl<'a> Selectors<'a> {
    pub fn new(state: &'a AppState) -> Self {
        Self { state }
    }

    /// Total tokens used (input + output).
    pub fn total_tokens(&self) -> u64 {
        self.state.input_tokens + self.state.output_tokens
    }

    /// Rough cost estimate in USD (Claude-family heuristic: $3/M input, $15/M output).
    pub fn estimated_cost_usd(&self) -> f64 {
        estimate_cost_usd(self.state.input_tokens, self.state.output_tokens)
    }

    /// Number of active (running) tasks.
    pub fn active_task_count(&self) -> usize {
        self.state
            .tasks
            .values()
            .filter(|t| t.status == TaskStatus::Running)
            .count()
    }

    /// Number of pending tasks.
    pub fn pending_task_count(&self) -> usize {
        self.state
            .tasks
            .values()
            .filter(|t| t.status == TaskStatus::Pending)
            .count()
    }

    /// Total tool count (built-in + MCP).
    pub fn total_tool_count(&self) -> usize {
        self.state.tool_names.len() + self.state.mcp_tool_names.len()
    }

    /// Whether any loading activity is happening.
    pub fn is_busy(&self) -> bool {
        self.state.is_loading || self.active_task_count() > 0
    }

    /// Short status line suitable for a status bar.
    pub fn status_line(&self) -> String {
        let mut parts = Vec::new();

        if !self.state.model.is_empty() {
            parts.push(self.state.model.clone());
        }

        if self.state.plan_mode {
            parts.push("PLAN".to_string());
        }

        if self.state.vim_mode {
            parts.push("VIM".to_string());
        }

        let tokens = self.total_tokens();
        if tokens > 0 {
            parts.push(format!("{}T", format_compact(tokens)));
        }

        let active = self.active_task_count();
        if active > 0 {
            parts.push(format!("{}⚡", active));
        }

        parts.join(" | ")
    }
}

fn format_compact(n: u64) -> String {
    format_token_count(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::app_state::{TaskEntry, TaskStatus};
    use std::collections::HashMap;

    fn state_with_tokens(input: u64, output: u64) -> AppState {
        AppState {
            input_tokens: input,
            output_tokens: output,
            ..Default::default()
        }
    }

    #[test]
    fn total_tokens() {
        let state = state_with_tokens(1000, 500);
        let sel = Selectors::new(&state);
        assert_eq!(sel.total_tokens(), 1500);
    }

    #[test]
    fn estimated_cost() {
        let state = state_with_tokens(1_000_000, 100_000);
        let sel = Selectors::new(&state);
        // $3/M input + $15/M output = $3 + $1.5 = $4.5
        let cost = sel.estimated_cost_usd();
        assert!((cost - 4.5).abs() < 0.01, "cost should be ~$4.50, got {}", cost);
    }

    #[test]
    fn active_task_count() {
        let mut state = AppState::default();
        state.tasks.insert(
            "t1".into(),
            TaskEntry {
                id: "t1".into(),
                status: TaskStatus::Running,
                description: "task 1".into(),
            },
        );
        state.tasks.insert(
            "t2".into(),
            TaskEntry {
                id: "t2".into(),
                status: TaskStatus::Pending,
                description: "task 2".into(),
            },
        );
        state.tasks.insert(
            "t3".into(),
            TaskEntry {
                id: "t3".into(),
                status: TaskStatus::Running,
                description: "task 3".into(),
            },
        );

        let sel = Selectors::new(&state);
        assert_eq!(sel.active_task_count(), 2);
        assert_eq!(sel.pending_task_count(), 1);
    }

    #[test]
    fn total_tool_count() {
        let mut state = AppState::default();
        state.tool_names = vec!["Bash".into(), "Read".into()];
        state.mcp_tool_names = vec!["mcp_search".into()];
        let sel = Selectors::new(&state);
        assert_eq!(sel.total_tool_count(), 3);
    }

    #[test]
    fn is_busy_when_loading() {
        let mut state = AppState::default();
        state.is_loading = true;
        assert!(Selectors::new(&state).is_busy());
    }

    #[test]
    fn is_busy_when_tasks_running() {
        let mut state = AppState::default();
        state.tasks.insert(
            "t1".into(),
            TaskEntry {
                id: "t1".into(),
                status: TaskStatus::Running,
                description: "running".into(),
            },
        );
        assert!(Selectors::new(&state).is_busy());
    }

    #[test]
    fn not_busy_when_idle() {
        let state = AppState::default();
        assert!(!Selectors::new(&state).is_busy());
    }

    #[test]
    fn status_line_basic() {
        let mut state = AppState::default();
        state.model = "claude-sonnet".into();
        state.input_tokens = 15000;
        state.output_tokens = 3000;
        let line = Selectors::new(&state).status_line();
        assert!(line.contains("claude-sonnet"), "{}", line);
        assert!(line.contains("18.0K"), "{}", line);
    }

    #[test]
    fn status_line_plan_mode() {
        let mut state = AppState::default();
        state.model = "model".into();
        state.plan_mode = true;
        let line = Selectors::new(&state).status_line();
        assert!(line.contains("PLAN"), "{}", line);
    }

    #[test]
    fn status_line_vim_mode() {
        let mut state = AppState::default();
        state.model = "model".into();
        state.vim_mode = true;
        let line = Selectors::new(&state).status_line();
        assert!(line.contains("VIM"), "{}", line);
    }

    #[test]
    fn format_compact_works() {
        assert_eq!(format_compact(500), "500");
        assert_eq!(format_compact(1500), "1.5K");
        assert_eq!(format_compact(2_500_000), "2.5M");
    }
}
