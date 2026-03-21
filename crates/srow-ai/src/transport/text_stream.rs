use std::pin::Pin;

use async_trait::async_trait;
use futures::{Stream, StreamExt};

use srow_core::error::StreamError;
use srow_core::ui_message_stream::{FinishReason, UIMessageChunk};

use super::traits::{ChatRequest, ChatTransport, TransportError};

pub struct TextStreamChatTransport {
    api_url: String,
    client: reqwest::Client,
}

impl TextStreamChatTransport {
    pub fn new(api_url: impl Into<String>) -> Self {
        Self {
            api_url: api_url.into(),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl ChatTransport for TextStreamChatTransport {
    async fn send_messages(
        &self,
        request: ChatRequest,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<UIMessageChunk, StreamError>> + Send>>,
        TransportError,
    > {
        let body = serde_json::json!({ "messages": request.messages });
        let response = self
            .client
            .post(&self.api_url)
            .json(&body)
            .send()
            .await
            .map_err(|e| TransportError::Http(e.to_string()))?;

        let text_id = uuid::Uuid::new_v4().to_string();
        let tid = text_id.clone();

        // Wrap the text stream into UIMessageChunk sequence
        let byte_stream = response.bytes_stream();

        let chunk_stream = async_stream::stream! {
            yield Ok(UIMessageChunk::Start { message_id: None, message_metadata: None });
            yield Ok(UIMessageChunk::TextStart { id: tid.clone() });

            let mut byte_stream = std::pin::pin!(byte_stream);
            while let Some(result) = byte_stream.next().await {
                match result {
                    Ok(bytes) => {
                        if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                            if !text.is_empty() {
                                yield Ok(UIMessageChunk::TextDelta {
                                    id: tid.clone(),
                                    delta: text,
                                });
                            }
                        }
                    }
                    Err(_) => {
                        yield Err(StreamError::Interrupted);
                        return;
                    }
                }
            }

            yield Ok(UIMessageChunk::TextEnd { id: tid.clone() });
            yield Ok(UIMessageChunk::Finish {
                finish_reason: FinishReason::Stop,
                usage: None,
            });
        };

        Ok(Box::pin(chunk_stream))
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
