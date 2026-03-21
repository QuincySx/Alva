// INPUT:  srow_core (UIMessage, ChatStatus, ChatError), srow_ai (ChatState), futures
// OUTPUT: pub struct GpuiChatState, pub enum NotifyKind
// POS:    ChatState implementation that bridges srow-ai's AbstractChat to GPUI via a notify channel.
use srow_core::ui_message::UIMessage;
use srow_core::ui_message_stream::ChatStatus;
use srow_core::error::ChatError;
use srow_ai::chat::ChatState;

pub struct GpuiChatState {
    messages: Vec<UIMessage>,
    status: ChatStatus,
    error: Option<ChatError>,
    notify_tx: futures::channel::mpsc::UnboundedSender<NotifyKind>,
}

#[derive(Debug)]
pub enum NotifyKind {
    Messages,
    Status,
    Error,
}

impl GpuiChatState {
    pub fn new(notify_tx: futures::channel::mpsc::UnboundedSender<NotifyKind>) -> Self {
        Self {
            messages: Vec::new(),
            status: ChatStatus::Ready,
            error: None,
            notify_tx,
        }
    }
}

impl ChatState for GpuiChatState {
    fn messages(&self) -> Vec<UIMessage> {
        self.messages.clone()
    }

    fn set_messages(&mut self, messages: Vec<UIMessage>) {
        self.messages = messages;
    }

    fn push_message(&mut self, message: UIMessage) {
        self.messages.push(message);
    }

    fn pop_message(&mut self) -> Option<UIMessage> {
        self.messages.pop()
    }

    fn replace_message(&mut self, index: usize, message: UIMessage) {
        if index < self.messages.len() {
            self.messages[index] = message;
        }
    }

    fn status(&self) -> ChatStatus {
        self.status.clone()
    }

    fn set_status(&mut self, status: ChatStatus) {
        self.status = status;
    }

    fn error(&self) -> Option<ChatError> {
        self.error.clone()
    }

    fn set_error(&mut self, error: Option<ChatError>) {
        self.error = error;
    }

    fn notify_messages_changed(&mut self) {
        let _ = self.notify_tx.unbounded_send(NotifyKind::Messages);
    }

    fn notify_status_changed(&mut self) {
        let _ = self.notify_tx.unbounded_send(NotifyKind::Status);
    }

    fn notify_error_changed(&mut self) {
        let _ = self.notify_tx.unbounded_send(NotifyKind::Error);
    }
}
