//! Contextual tip system for displaying helpful hints during loading.
//!
//! Tips are chosen using a "longest since shown" algorithm with per-context
//! filtering and a cooldown period to avoid repeating tips too frequently.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// The context in which a tip may be shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TipContext {
    /// General usage tips (always applicable).
    General,
    /// Tips related to query / prompt composition.
    Query,
    /// Tips shown when the agent is using tools.
    ToolUse,
    /// Tips for first-time users.
    Onboarding,
}

/// A single tip entry.
#[derive(Debug, Clone)]
pub struct Tip {
    /// Unique identifier for the tip.
    pub id: &'static str,
    /// The text displayed to the user.
    pub text: &'static str,
    /// Applicable context(s).
    pub contexts: &'static [TipContext],
}

/// Tracks when a tip was last shown.
#[derive(Debug, Clone)]
struct TipState {
    last_shown: Option<Instant>,
}

/// Registry of tips with cooldown tracking and selection logic.
pub struct TipRegistry {
    tips: Vec<Tip>,
    state: HashMap<&'static str, TipState>,
    cooldown: Duration,
}

impl TipRegistry {
    /// Create a new registry pre-loaded with built-in tips.
    pub fn new() -> Self {
        Self::with_cooldown(Duration::from_secs(120))
    }

    /// Create a registry with a custom cooldown period.
    pub fn with_cooldown(cooldown: Duration) -> Self {
        let tips = built_in_tips();
        let state = tips
            .iter()
            .map(|t| (t.id, TipState { last_shown: None }))
            .collect();
        Self {
            tips,
            state,
            cooldown,
        }
    }

    /// Return all registered tips.
    pub fn tips(&self) -> &[Tip] {
        &self.tips
    }

    /// Select the best tip for the given context.
    ///
    /// Uses the "longest since shown" algorithm:
    /// 1. Filter tips matching the context.
    /// 2. Exclude tips still within the cooldown window.
    /// 3. Among the remaining, pick the one that was shown the longest ago
    ///    (or never shown).
    pub fn next_tip(&mut self, context: TipContext) -> Option<&Tip> {
        let now = Instant::now();

        let mut best_idx: Option<usize> = None;
        let mut best_age: Option<Duration> = None; // None means "never shown" (oldest)

        for (i, tip) in self.tips.iter().enumerate() {
            if !tip.contexts.contains(&context) && !tip.contexts.contains(&TipContext::General) {
                continue;
            }

            let st = self.state.get(tip.id)?;

            // Check cooldown
            if let Some(last) = st.last_shown {
                if now.duration_since(last) < self.cooldown {
                    continue;
                }
            }

            let age = st.last_shown.map(|l| now.duration_since(l));

            let is_better = match (age, best_age) {
                // Never shown beats any shown time.
                (None, Some(_)) => true,
                (None, None) => best_idx.is_none(),
                (Some(_), None) => false,
                (Some(a), Some(b)) => a > b,
            };

            if is_better {
                best_idx = Some(i);
                best_age = age;
            }
        }

        if let Some(idx) = best_idx {
            let id = self.tips[idx].id;
            if let Some(st) = self.state.get_mut(id) {
                st.last_shown = Some(now);
            }
            Some(&self.tips[idx])
        } else {
            None
        }
    }

    /// Reset all cooldown / last-shown state.
    pub fn reset(&mut self) {
        for st in self.state.values_mut() {
            st.last_shown = None;
        }
    }
}

impl Default for TipRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Built-in tips shipped with the CLI.
fn built_in_tips() -> Vec<Tip> {
    vec![
        Tip {
            id: "multiline",
            text: "Use Shift+Enter for multi-line input.",
            contexts: &[TipContext::General],
        },
        Tip {
            id: "slash_help",
            text: "Type /help to see all available commands.",
            contexts: &[TipContext::Onboarding],
        },
        Tip {
            id: "shell_bang",
            text: "Prefix a message with ! to run a shell command directly.",
            contexts: &[TipContext::General],
        },
        Tip {
            id: "session_resume",
            text: "Use /resume to pick up where you left off.",
            contexts: &[TipContext::General],
        },
        Tip {
            id: "be_specific",
            text: "More specific prompts produce better results.",
            contexts: &[TipContext::Query],
        },
        Tip {
            id: "provide_context",
            text: "Include file paths or error messages in your prompt for more targeted help.",
            contexts: &[TipContext::Query],
        },
        Tip {
            id: "tool_approval",
            text: "Tools that modify files will ask for approval before running.",
            contexts: &[TipContext::ToolUse],
        },
        Tip {
            id: "print_mode",
            text: "Use -p / --print for non-interactive, single-prompt mode.",
            contexts: &[TipContext::General],
        },
        Tip {
            id: "config_cmd",
            text: "Run /config to view your current provider and model settings.",
            contexts: &[TipContext::Onboarding],
        },
        Tip {
            id: "sessions_list",
            text: "Use /sessions to list all past sessions in this workspace.",
            contexts: &[TipContext::General],
        },
        Tip {
            id: "iterate",
            text: "You can ask follow-up questions to refine the answer.",
            contexts: &[TipContext::Query],
        },
        Tip {
            id: "clear_screen",
            text: "Type /clear to clear the terminal screen.",
            contexts: &[TipContext::General],
        },
        Tip {
            id: "env_debug",
            text: "Set RUST_LOG=debug for verbose logging.",
            contexts: &[TipContext::General],
        },
        Tip {
            id: "tool_chain",
            text: "The agent can chain multiple tool calls to complete complex tasks.",
            contexts: &[TipContext::ToolUse],
        },
        Tip {
            id: "new_session",
            text: "Start a /new session to reset context and begin fresh.",
            contexts: &[TipContext::General],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_tip_returns_some_for_general() {
        let mut reg = TipRegistry::with_cooldown(Duration::ZERO);
        let tip = reg.next_tip(TipContext::General);
        assert!(tip.is_some());
    }

    #[test]
    fn next_tip_rotates() {
        let mut reg = TipRegistry::with_cooldown(Duration::ZERO);
        let first = reg.next_tip(TipContext::General).map(|t| t.id);
        let second = reg.next_tip(TipContext::General).map(|t| t.id);
        // After showing the first, it should pick a different one.
        assert_ne!(first, second);
    }

    #[test]
    fn cooldown_prevents_repeat() {
        let mut reg = TipRegistry::with_cooldown(Duration::from_secs(9999));
        // Show all general tips once.
        let general_count = reg
            .tips()
            .iter()
            .filter(|t| t.contexts.contains(&TipContext::General))
            .count();
        for _ in 0..general_count {
            reg.next_tip(TipContext::General);
        }
        // Now all are on cooldown.
        assert!(reg.next_tip(TipContext::General).is_none());
    }

    #[test]
    fn reset_clears_state() {
        let mut reg = TipRegistry::new();
        reg.next_tip(TipContext::General);
        reg.reset();
        // After reset, the first tip should be available again.
        assert!(reg.next_tip(TipContext::General).is_some());
    }
}
