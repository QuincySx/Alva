use std::collections::HashMap;
use std::pin::Pin;

use async_trait::async_trait;
use futures::{Stream, StreamExt};

use srow_core::error::StreamError;
use srow_core::ui_message_stream::sse::parse_sse_stream;
use srow_core::ui_message_stream::UIMessageChunk;

use super::traits::{ChatRequest, ChatTransport, TransportError};

pub struct HttpSseChatTransport {
    api_url: String,
    headers: HashMap<String, String>,
    client: reqwest::Client,
}

impl HttpSseChatTransport {
    pub fn new(api_url: impl Into<String>) -> Self {
        Self {
            api_url: api_url.into(),
            headers: HashMap::new(),
            client: reqwest::Client::new(),
        }
    }

    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers = headers;
        self
    }

    fn build_sse_stream(
        response: reqwest::Response,
    ) -> Pin<Box<dyn Stream<Item = Result<UIMessageChunk, StreamError>> + Send>> {
        let byte_stream = response
            .bytes_stream()
            .map(|r| r.map_err(|_e| StreamError::Interrupted));
        let chunk_stream = parse_sse_stream(byte_stream);
        Box::pin(chunk_stream)
    }
}

#[async_trait]
impl ChatTransport for HttpSseChatTransport {
    async fn send_messages(
        &self,
        request: ChatRequest,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<UIMessageChunk, StreamError>> + Send>>,
        TransportError,
    > {
        let body = serde_json::json!({
            "messages": request.messages,
        });

        let mut req = self.client.post(&self.api_url).json(&body);
        for (k, v) in &self.headers {
            req = req.header(k, v);
        }

        let response = req
            .send()
            .await
            .map_err(|e| TransportError::Http(e.to_string()))?;

        if !response.status().is_success() {
            return Err(TransportError::Http(format!("HTTP {}", response.status())));
        }

        Ok(Self::build_sse_stream(response))
    }

    async fn reconnect(
        &self,
        chat_id: &str,
    ) -> Result<
        Option<Pin<Box<dyn Stream<Item = Result<UIMessageChunk, StreamError>> + Send>>>,
        TransportError,
    > {
        // Try reconnect endpoint
        let url = format!("{}?chatId={}&resume=true", self.api_url, chat_id);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| TransportError::Http(e.to_string()))?;

        if !response.status().is_success() {
            return Ok(None);
        }

        Ok(Some(Self::build_sse_stream(response)))
    }
}
