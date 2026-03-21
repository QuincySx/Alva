use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures::Stream;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;

use srow_core::domain::agent::AgentConfig;
use srow_core::error::StreamError;
use srow_core::ports::provider::language_model::LanguageModel;
use srow_core::ports::storage::SessionStorage;
use srow_core::ports::tool::ToolRegistry;
use srow_core::ui_message_stream::{FinishReason, UIMessageChunk};

use super::traits::{ChatRequest, ChatTransport, TransportError};

/// Approval response sent from UI back to engine
#[derive(Debug, Clone)]
pub struct ApprovalResponse {
    pub tool_call_id: String,
    pub approved: bool,
}

pub struct DirectChatTransport {
    #[allow(dead_code)]
    llm: Arc<dyn LanguageModel>,
    #[allow(dead_code)]
    tools: Arc<ToolRegistry>,
    #[allow(dead_code)]
    storage: Arc<dyn SessionStorage>,
    #[allow(dead_code)]
    config: Arc<AgentConfig>,
}

impl DirectChatTransport {
    pub fn new(
        llm: Arc<dyn LanguageModel>,
        tools: Arc<ToolRegistry>,
        storage: Arc<dyn SessionStorage>,
        config: Arc<AgentConfig>,
    ) -> Self {
        Self {
            llm,
            tools,
            storage,
            config,
        }
    }
}

#[async_trait]
impl ChatTransport for DirectChatTransport {
    async fn send_messages(
        &self,
        _request: ChatRequest,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<UIMessageChunk, StreamError>> + Send>>,
        TransportError,
    > {
        let (chunk_tx, chunk_rx) = mpsc::unbounded_channel::<Result<UIMessageChunk, StreamError>>();

        // For now, just emit a simple Start -> Finish sequence as placeholder.
        // The real integration with AgentEngine happens in Task 11 when we
        // migrate the engine to emit UIMessageChunk directly.
        //
        // PLACEHOLDER: We emit Start + Finish to make the transport functional
        // but not yet wired to the real engine.
        let tx = chunk_tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(Ok(UIMessageChunk::Start {
                message_id: Some(uuid::Uuid::new_v4().to_string()),
                message_metadata: None,
            }));
            let _ = tx.send(Ok(UIMessageChunk::Finish {
                finish_reason: FinishReason::Stop,
                usage: None,
            }));
        });

        Ok(Box::pin(UnboundedReceiverStream::new(chunk_rx)))
    }

    async fn reconnect(
        &self,
        _chat_id: &str,
    ) -> Result<
        Option<Pin<Box<dyn Stream<Item = Result<UIMessageChunk, StreamError>> + Send>>>,
        TransportError,
    > {
        Ok(None)
    }
}
