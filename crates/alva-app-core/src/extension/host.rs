//! ExtensionHost — runtime container for extension event handlers and commands.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use alva_types::{AgentMessage, CancellationToken, Message};
use super::events::{ExtensionEvent, EventResult};

type HandlerFn = Box<dyn Fn(&ExtensionEvent) -> EventResult + Send + Sync>;

/// Command registered by an extension (metadata only in V1).
pub struct RegisteredCommand {
    pub name: String,
    pub description: String,
    pub source_extension: String,
}

/// Runtime container for extension event handlers and commands.
pub struct ExtensionHost {
    handlers: HashMap<&'static str, Vec<(String, HandlerFn)>>,  // (extension_name, handler)
    commands: Vec<RegisteredCommand>,
    pending_messages: Option<Arc<alva_agent_core::pending_queue::PendingMessageQueue>>,
    cancel_token: Option<Arc<std::sync::Mutex<CancellationToken>>>,
}

impl ExtensionHost {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            commands: Vec::new(),
            pending_messages: None,
            cancel_token: None,
        }
    }

    pub fn register_handler(&mut self, event_type: &'static str, source: String, handler: HandlerFn) {
        self.handlers.entry(event_type).or_default().push((source, handler));
    }

    pub fn register_command(&mut self, cmd: RegisteredCommand) {
        self.commands.push(cmd);
    }

    /// Dispatch event to all registered handlers. Sequential, first Block/Handled wins.
    pub fn emit(&self, event: &ExtensionEvent) -> EventResult {
        let event_type = event.event_type();
        if let Some(handlers) = self.handlers.get(event_type) {
            for (ext_name, handler) in handlers {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    handler(event)
                }));
                match result {
                    Ok(EventResult::Continue) => continue,
                    Ok(r @ EventResult::Block { .. }) => {
                        tracing::info!(extension = ext_name.as_str(), event = event_type, "extension blocked event");
                        return r;
                    }
                    Ok(r @ EventResult::Handled) => {
                        tracing::debug!(extension = ext_name.as_str(), event = event_type, "extension handled event");
                        return r;
                    }
                    Err(panic) => {
                        let msg = if let Some(s) = panic.downcast_ref::<String>() {
                            s.clone()
                        } else if let Some(s) = panic.downcast_ref::<&str>() {
                            s.to_string()
                        } else {
                            "unknown panic".to_string()
                        };
                        tracing::error!(extension = ext_name.as_str(), event = event_type, error = msg.as_str(), "extension handler panicked");
                        continue; // isolated: don't propagate
                    }
                }
            }
        }
        EventResult::Continue
    }

    pub fn bind_agent(
        &mut self,
        pending: Arc<alva_agent_core::pending_queue::PendingMessageQueue>,
        cancel: Arc<std::sync::Mutex<CancellationToken>>,
    ) {
        self.pending_messages = Some(pending);
        self.cancel_token = Some(cancel);
    }

    pub fn commands(&self) -> &[RegisteredCommand] {
        &self.commands
    }
}

impl Default for ExtensionHost {
    fn default() -> Self {
        Self::new()
    }
}

/// API handle given to extensions during activate().
pub struct HostAPI {
    host: Arc<RwLock<ExtensionHost>>,
    extension_name: String,
}

impl HostAPI {
    pub fn new(host: Arc<RwLock<ExtensionHost>>, extension_name: String) -> Self {
        Self { host, extension_name }
    }

    /// Subscribe to an event type.
    pub fn on(&self, event_type: &'static str, handler: impl Fn(&ExtensionEvent) -> EventResult + Send + Sync + 'static) {
        let mut host = self.host.write().unwrap();
        host.register_handler(event_type, self.extension_name.clone(), Box::new(handler));
    }

    /// Register a /command (metadata only in V1, routing is P3).
    pub fn register_command(&self, name: &str, description: &str) {
        let mut host = self.host.write().unwrap();
        host.register_command(RegisteredCommand {
            name: name.to_string(),
            description: description.to_string(),
            source_extension: self.extension_name.clone(),
        });
    }

    /// Inject a steering message into the agent.
    pub fn steer(&self, text: &str) {
        let host = self.host.read().unwrap();
        if let Some(ref pending) = host.pending_messages {
            pending.steer(AgentMessage::Steering(Message::user(text)));
        }
    }

    /// Queue a follow-up message.
    pub fn follow_up(&self, text: &str) {
        let host = self.host.read().unwrap();
        if let Some(ref pending) = host.pending_messages {
            pending.follow_up(AgentMessage::FollowUp(Message::user(text)));
        }
    }

    /// Cancel the current agent loop.
    pub fn shutdown(&self) {
        let host = self.host.read().unwrap();
        if let Some(ref cancel) = host.cancel_token {
            let token = cancel.lock().unwrap();
            token.cancel();
        }
    }

    /// Get the extension name this API belongs to.
    pub fn extension_name(&self) -> &str {
        &self.extension_name
    }
}
