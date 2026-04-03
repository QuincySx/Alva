// INPUT:  std::collections::VecDeque, std::sync::Mutex, alva_types::AgentMessage
// OUTPUT: AgentLoopHook, PendingMessageQueue
// POS:    Pending message queue for agent steering and follow-up.

use std::collections::VecDeque;
use std::sync::Mutex;

use alva_types::AgentMessage;

/// Extension point for the agent run loop.
///
/// The run loop calls these methods at specific checkpoints to check for
/// injected messages. Implement this trait to control message injection
/// from outside the loop.
///
/// Default implementations return no messages (no-op).
pub trait AgentLoopHook: Send + Sync {
    /// Called after tool execution completes. Return a message to inject
    /// as a mid-turn steering instruction before the next LLM call.
    fn take_steering(&self) -> Option<AgentMessage> {
        None
    }

    /// Called when the inner loop finishes naturally (no more tool calls).
    /// Return messages to continue the conversation.
    fn take_follow_ups(&self) -> Vec<AgentMessage> {
        vec![]
    }

    /// Whether there are any pending injections.
    fn has_pending(&self) -> bool {
        false
    }
}

/// Runtime message injection for agent steering and follow-up.
///
/// Thread-safe: can be called from UI thread while agent loop runs.
pub struct PendingMessageQueue {
    steering: Mutex<Option<AgentMessage>>,
    follow_up: Mutex<VecDeque<AgentMessage>>,
}

impl PendingMessageQueue {
    pub fn new() -> Self {
        Self {
            steering: Mutex::new(None),
            follow_up: Mutex::new(VecDeque::new()),
        }
    }

    /// Queue a steering message (replaces previous if any).
    pub fn steer(&self, msg: AgentMessage) {
        *self.steering.lock().unwrap() = Some(msg);
    }

    /// Queue a follow-up message (accumulates).
    pub fn follow_up(&self, msg: AgentMessage) {
        self.follow_up.lock().unwrap().push_back(msg);
    }
}

impl AgentLoopHook for PendingMessageQueue {
    fn take_steering(&self) -> Option<AgentMessage> {
        self.steering.lock().unwrap().take()
    }

    fn take_follow_ups(&self) -> Vec<AgentMessage> {
        self.follow_up.lock().unwrap().drain(..).collect()
    }

    fn has_pending(&self) -> bool {
        self.steering.lock().unwrap().is_some() || !self.follow_up.lock().unwrap().is_empty()
    }
}

impl Default for PendingMessageQueue {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use alva_types::base::message::Message;

    fn user_msg(text: &str) -> AgentMessage {
        AgentMessage::Standard(Message::user(text))
    }

    fn steering_msg(text: &str) -> AgentMessage {
        AgentMessage::Steering(Message::user(text))
    }

    #[test]
    fn steer_replaces_previous() {
        let injector = PendingMessageQueue::new();
        injector.steer(steering_msg("first"));
        injector.steer(steering_msg("second"));

        let taken = injector.take_steering().unwrap();
        if let AgentMessage::Steering(m) = taken {
            assert!(m.text_content().contains("second"));
        } else {
            panic!("expected Steering message");
        }
    }

    #[test]
    fn follow_up_accumulates() {
        let injector = PendingMessageQueue::new();
        injector.follow_up(user_msg("a"));
        injector.follow_up(user_msg("b"));
        injector.follow_up(user_msg("c"));

        let taken = injector.take_follow_ups();
        assert_eq!(taken.len(), 3);

        if let AgentMessage::Standard(ref m) = taken[0] {
            assert!(m.text_content().contains("a"));
        }
        if let AgentMessage::Standard(ref m) = taken[2] {
            assert!(m.text_content().contains("c"));
        }
    }

    #[test]
    fn take_steering_returns_and_clears() {
        let injector = PendingMessageQueue::new();
        injector.steer(steering_msg("once"));

        assert!(injector.take_steering().is_some());
        assert!(injector.take_steering().is_none());
    }

    #[test]
    fn take_follow_ups_returns_and_clears() {
        let injector = PendingMessageQueue::new();
        injector.follow_up(user_msg("x"));
        injector.follow_up(user_msg("y"));

        let first = injector.take_follow_ups();
        assert_eq!(first.len(), 2);

        let second = injector.take_follow_ups();
        assert!(second.is_empty());
    }

    #[test]
    fn has_pending_reflects_state() {
        let injector = PendingMessageQueue::new();
        assert!(!injector.has_pending());

        injector.steer(steering_msg("s"));
        assert!(injector.has_pending());

        injector.take_steering();
        assert!(!injector.has_pending());

        injector.follow_up(user_msg("f"));
        assert!(injector.has_pending());

        injector.take_follow_ups();
        assert!(!injector.has_pending());
    }

    #[test]
    fn default_impl() {
        let injector = PendingMessageQueue::default();
        assert!(!injector.has_pending());
    }
}
