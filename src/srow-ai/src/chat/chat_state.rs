use srow_core::ui_message::UIMessage;
use srow_core::ui_message_stream::ChatStatus;
use srow_core::error::ChatError;

/// Framework-agnostic chat state interface.
/// Does NOT require Send + Sync — GPUI's Context is not Send.
/// The framework binding layer handles thread safety.
pub trait ChatState {
    /// Returns an owned copy of messages (avoids borrow conflicts with interior mutability)
    fn messages(&self) -> Vec<UIMessage>;
    fn set_messages(&mut self, messages: Vec<UIMessage>);
    fn push_message(&mut self, message: UIMessage);
    fn pop_message(&mut self) -> Option<UIMessage>;
    fn replace_message(&mut self, index: usize, message: UIMessage);

    fn status(&self) -> ChatStatus;
    fn set_status(&mut self, status: ChatStatus);

    fn error(&self) -> Option<ChatError>;
    fn set_error(&mut self, error: Option<ChatError>);

    /// Notify the UI framework that messages changed (e.g. cx.notify() in GPUI)
    fn notify_messages_changed(&mut self);
    fn notify_status_changed(&mut self);
    fn notify_error_changed(&mut self);
}
