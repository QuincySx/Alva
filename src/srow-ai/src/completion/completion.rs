use std::collections::HashMap;

use srow_core::error::ChatError;

use crate::util::AbortController;

pub struct Completion {
    api_url: String,
    client: reqwest::Client,
    headers: HashMap<String, String>,
    completion: String,
    is_loading: bool,
    error: Option<ChatError>,
    abort: Option<AbortController>,
}

impl Completion {
    pub fn new(api_url: impl Into<String>) -> Self {
        Self {
            api_url: api_url.into(),
            client: reqwest::Client::new(),
            headers: HashMap::new(),
            completion: String::new(),
            is_loading: false,
            error: None,
            abort: None,
        }
    }

    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers = headers;
        self
    }

    pub async fn complete(&mut self, prompt: &str) -> Result<String, ChatError> {
        self.is_loading = true;
        self.error = None;
        self.completion.clear();

        let (controller, mut abort_handle) = AbortController::new();
        self.abort = Some(controller);

        let body = serde_json::json!({ "prompt": prompt });
        let mut req = self.client.post(&self.api_url).json(&body);
        for (k, v) in &self.headers {
            req = req.header(k, v);
        }

        let response = req
            .send()
            .await
            .map_err(|e| ChatError::Transport(e.to_string()))?;

        use futures::StreamExt;
        let stream = response.bytes_stream();
        let mut stream = std::pin::pin!(stream);

        loop {
            tokio::select! {
                chunk = stream.next() => {
                    match chunk {
                        Some(Ok(bytes)) => {
                            if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                                self.completion.push_str(&text);
                            }
                        }
                        Some(Err(e)) => {
                            self.is_loading = false;
                            let err = ChatError::Stream(e.to_string());
                            self.error = Some(err.clone());
                            return Err(err);
                        }
                        None => break,
                    }
                }
                _ = abort_handle.cancelled() => {
                    self.is_loading = false;
                    return Ok(self.completion.clone());
                }
            }
        }

        self.is_loading = false;
        self.abort = None;
        Ok(self.completion.clone())
    }

    pub fn stop(&mut self) {
        if let Some(controller) = self.abort.take() {
            controller.abort();
        }
    }

    pub fn completion(&self) -> &str {
        &self.completion
    }

    pub fn is_loading(&self) -> bool {
        self.is_loading
    }

    pub fn error(&self) -> Option<&ChatError> {
        self.error.as_ref()
    }
}
