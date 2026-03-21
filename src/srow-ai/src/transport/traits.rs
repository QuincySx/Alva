use std::pin::Pin;
use async_trait::async_trait;
use futures::Stream;
use srow_core::ui_message::UIMessage;
use srow_core::ui_message_stream::UIMessageChunk;
use srow_core::error::StreamError;
use crate::util::AbortHandle;

/// Transport error
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("Engine error: {0}")]
    Engine(String),
    #[error("Connection refused")]
    ConnectionRefused,
    #[error("Other: {0}")]
    Other(String),
}

/// Request to send messages via transport
pub struct ChatRequest {
    pub chat_id: String,
    pub messages: Vec<UIMessage>,
    pub abort_handle: AbortHandle,
}

/// Abstract transport layer — how messages get sent to AI and chunks come back.
#[async_trait]
pub trait ChatTransport: Send + Sync {
    async fn send_messages(
        &self,
        request: ChatRequest,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<UIMessageChunk, StreamError>> + Send>>,
        TransportError,
    >;

    /// Attempt to reconnect/resume an interrupted stream.
    async fn reconnect(
        &self,
        chat_id: &str,
    ) -> Result<
        Option<Pin<Box<dyn Stream<Item = Result<UIMessageChunk, StreamError>> + Send>>>,
        TransportError,
    >;
}
