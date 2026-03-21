use std::collections::HashMap;
use std::marker::PhantomData;

use serde::de::DeserializeOwned;

use srow_core::error::ChatError;

use crate::util::AbortController;

pub struct ObjectGeneration<T: DeserializeOwned> {
    api_url: String,
    client: reqwest::Client,
    headers: HashMap<String, String>,
    object: Option<serde_json::Value>,
    is_loading: bool,
    error: Option<ChatError>,
    abort: Option<AbortController>,
    _phantom: PhantomData<T>,
}

impl<T: DeserializeOwned> ObjectGeneration<T> {
    pub fn new(api_url: impl Into<String>) -> Self {
        Self {
            api_url: api_url.into(),
            client: reqwest::Client::new(),
            headers: HashMap::new(),
            object: None,
            is_loading: false,
            error: None,
            abort: None,
            _phantom: PhantomData,
        }
    }

    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers = headers;
        self
    }

    pub async fn submit(&mut self, input: serde_json::Value) {
        self.is_loading = true;
        self.error = None;
        self.object = None;

        let (controller, mut abort_handle) = AbortController::new();
        self.abort = Some(controller);

        let mut req = self.client.post(&self.api_url).json(&input);
        for (k, v) in &self.headers {
            req = req.header(k, v);
        }

        let response = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                self.is_loading = false;
                self.error = Some(ChatError::Transport(e.to_string()));
                return;
            }
        };

        use futures::StreamExt;
        let stream = response.bytes_stream();
        let mut stream = std::pin::pin!(stream);
        let mut accumulated = String::new();

        loop {
            tokio::select! {
                chunk = stream.next() => {
                    match chunk {
                        Some(Ok(bytes)) => {
                            if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                                accumulated.push_str(&text);
                                // Try partial JSON parse
                                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&accumulated) {
                                    self.object = Some(value);
                                }
                            }
                        }
                        Some(Err(e)) => {
                            self.is_loading = false;
                            self.error = Some(ChatError::Stream(e.to_string()));
                            return;
                        }
                        None => break,
                    }
                }
                _ = abort_handle.cancelled() => {
                    self.is_loading = false;
                    return;
                }
            }
        }

        // Final parse
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&accumulated) {
            self.object = Some(value);
        }

        self.is_loading = false;
        self.abort = None;
    }

    pub fn object(&self) -> Option<&serde_json::Value> {
        self.object.as_ref()
    }

    pub fn typed_object(&self) -> Option<T> {
        self.object
            .as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    pub fn stop(&mut self) {
        if let Some(controller) = self.abort.take() {
            controller.abort();
        }
    }

    pub fn clear(&mut self) {
        self.object = None;
        self.error = None;
    }

    pub fn is_loading(&self) -> bool {
        self.is_loading
    }

    pub fn error(&self) -> Option<&ChatError> {
        self.error.as_ref()
    }
}
