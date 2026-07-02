// INPUT:  reqwest, serde_json, std::env, std::time
// OUTPUT: VideoUnderstandConfig, VideoInput, VideoUnderstandError, understand_video, guess_video_mime
// POS:    Layer-1 core: upload a video to an OpenAI-compatible multimodal
//         backend (Kimi/Moonshot) and return the model's text understanding.
//         Mirrors what kimi-code's kimi-files.ts + chat path does:
//           1) POST {base}/files (multipart, purpose=video) -> file id
//           2) POST {base}/chat/completions with a `video_url: ms://<id>` part
//         No frame extraction / transcoding happens here — that is the
//         backend's job (MoonViT-3D). This module is pure transport.
//! kimi_video — reusable "video → text" client over an OpenAI-compatible backend

use std::time::Duration;

/// How long to wait for upload + understanding before giving up.
const DEFAULT_TIMEOUT_SECS: u64 = 180;
/// Default Moonshot/Kimi OpenAI-compatible base URL.
const DEFAULT_BASE_URL: &str = "https://api.moonshot.ai/v1";
/// Default multimodal model id (override via `KIMI_MODEL` — ids change).
const DEFAULT_MODEL: &str = "kimi-k2.5";

/// Configuration for the video-understanding backend.
///
/// Backend-agnostic in shape, but defaults target Kimi/Moonshot. Any
/// OpenAI-compatible endpoint that accepts `purpose=video` file uploads
/// and `video_url` content parts works by overriding `base_url` / `model`.
#[derive(Debug, Clone)]
pub struct VideoUnderstandConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub timeout: Duration,
}

impl VideoUnderstandConfig {
    /// Build config from environment:
    /// - key:   `KIMI_API_KEY` or `MOONSHOT_API_KEY` (required)
    /// - base:  `KIMI_BASE_URL` (default `https://api.moonshot.ai/v1`)
    /// - model: `KIMI_MODEL` (default `kimi-k2.5`)
    pub fn from_env() -> Result<Self, VideoUnderstandError> {
        let api_key = std::env::var("KIMI_API_KEY")
            .ok()
            .or_else(|| std::env::var("MOONSHOT_API_KEY").ok())
            .filter(|s| !s.is_empty())
            .ok_or(VideoUnderstandError::MissingApiKey)?;
        let base_url = std::env::var("KIMI_BASE_URL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        let model = std::env::var("KIMI_MODEL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_MODEL.to_string());
        Ok(Self {
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
            model,
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
        })
    }
}

/// An in-memory video to understand.
#[derive(Debug, Clone)]
pub struct VideoInput {
    pub bytes: Vec<u8>,
    pub filename: String,
    pub mime_type: String,
}

/// Errors surfaced by the video-understanding flow.
#[derive(Debug)]
pub enum VideoUnderstandError {
    /// No `KIMI_API_KEY` / `MOONSHOT_API_KEY` in the environment.
    MissingApiKey,
    /// Building the HTTP client / request failed.
    Client(String),
    /// Network-level failure (connect/timeout/etc.).
    Http(String),
    /// Backend returned a non-2xx status.
    Api { stage: &'static str, status: u16, body: String },
    /// Response body could not be parsed into the expected shape.
    Parse(String),
}

impl std::fmt::Display for VideoUnderstandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingApiKey => write!(
                f,
                "missing API key: set KIMI_API_KEY (or MOONSHOT_API_KEY)"
            ),
            Self::Client(m) => write!(f, "HTTP client error: {m}"),
            Self::Http(m) => write!(f, "HTTP request failed: {m}"),
            Self::Api { stage, status, body } => {
                write!(f, "backend error during {stage}: HTTP {status}: {body}")
            }
            Self::Parse(m) => write!(f, "failed to parse backend response: {m}"),
        }
    }
}

impl std::error::Error for VideoUnderstandError {}

/// Guess a video MIME type from a filename extension. Returns `None` for
/// non-video / unknown extensions. Matches the set kimi-code accepts.
pub fn guess_video_mime(filename: &str) -> Option<&'static str> {
    let ext = filename.rsplit('.').next()?.to_ascii_lowercase();
    let mime = match ext.as_str() {
        "mp4" => "video/mp4",
        "m4v" => "video/x-m4v",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "avi" => "video/x-msvideo",
        "flv" => "video/x-flv",
        "wmv" => "video/x-ms-wmv",
        "mpg" | "mpeg" => "video/mpeg",
        "ogv" => "video/ogg",
        "3gp" => "video/3gpp",
        "3g2" => "video/3gpp2",
        _ => return None,
    };
    Some(mime)
}

/// Upload `video` to the backend and return the model's text understanding,
/// guided by `prompt`.
///
/// Two round-trips: (1) multipart file upload with `purpose=video` to get a
/// file id; (2) a chat completion carrying a `video_url: ms://<id>` part.
pub async fn understand_video(
    video: VideoInput,
    prompt: &str,
    config: &VideoUnderstandConfig,
) -> Result<String, VideoUnderstandError> {
    let client = reqwest::Client::builder()
        .timeout(config.timeout)
        .build()
        .map_err(|e| VideoUnderstandError::Client(e.to_string()))?;

    let file_id = upload_video(&client, &video, config).await?;
    chat_understand(&client, &file_id, prompt, config).await
}

/// Step 1 — multipart upload, returns the backend file id.
async fn upload_video(
    client: &reqwest::Client,
    video: &VideoInput,
    config: &VideoUnderstandConfig,
) -> Result<String, VideoUnderstandError> {
    let part = reqwest::multipart::Part::bytes(video.bytes.clone())
        .file_name(video.filename.clone())
        .mime_str(&video.mime_type)
        .map_err(|e| VideoUnderstandError::Client(e.to_string()))?;
    let form = reqwest::multipart::Form::new()
        .text("purpose", "video")
        .part("file", part);

    let resp = client
        .post(format!("{}/files", config.base_url))
        .bearer_auth(&config.api_key)
        .multipart(form)
        .send()
        .await
        .map_err(|e| VideoUnderstandError::Http(e.to_string()))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| VideoUnderstandError::Http(e.to_string()))?;
    if !status.is_success() {
        return Err(VideoUnderstandError::Api {
            stage: "file upload",
            status: status.as_u16(),
            body,
        });
    }

    let json: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| VideoUnderstandError::Parse(e.to_string()))?;
    json.get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| VideoUnderstandError::Parse(format!("no `id` in upload response: {body}")))
}

/// Step 2 — chat completion with a `video_url: ms://<id>` content part.
async fn chat_understand(
    client: &reqwest::Client,
    file_id: &str,
    prompt: &str,
    config: &VideoUnderstandConfig,
) -> Result<String, VideoUnderstandError> {
    let request = serde_json::json!({
        "model": config.model,
        "messages": [{
            "role": "user",
            "content": [
                { "type": "text", "text": prompt },
                { "type": "video_url", "video_url": { "url": format!("ms://{file_id}") } }
            ]
        }]
    });

    let resp = client
        .post(format!("{}/chat/completions", config.base_url))
        .bearer_auth(&config.api_key)
        .json(&request)
        .send()
        .await
        .map_err(|e| VideoUnderstandError::Http(e.to_string()))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| VideoUnderstandError::Http(e.to_string()))?;
    if !status.is_success() {
        return Err(VideoUnderstandError::Api {
            stage: "chat completion",
            status: status.as_u16(),
            body,
        });
    }

    let json: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| VideoUnderstandError::Parse(e.to_string()))?;
    extract_message_text(&json)
        .ok_or_else(|| VideoUnderstandError::Parse(format!("no message content in response: {body}")))
}

/// Pull `choices[0].message.content` out of an OpenAI-compatible response.
/// Content may be a plain string or an array of `{type:text,text}` parts.
fn extract_message_text(json: &serde_json::Value) -> Option<String> {
    let content = json
        .get("choices")?
        .as_array()?
        .first()?
        .get("message")?
        .get("content")?;

    if let Some(s) = content.as_str() {
        return Some(s.to_string());
    }
    if let Some(parts) = content.as_array() {
        let text = parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("");
        if !text.is_empty() {
            return Some(text);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    //! Pure-logic tests (no network). The live upload+chat path is covered
    //! by the `#[ignore]`d integration test in `understand_video.rs`, which
    //! needs a real KIMI_API_KEY and a sample video.
    use super::*;

    #[test]
    fn guess_video_mime_known_extensions() {
        assert_eq!(guess_video_mime("clip.mp4"), Some("video/mp4"));
        assert_eq!(guess_video_mime("Demo.MOV"), Some("video/quicktime"));
        assert_eq!(guess_video_mime("a.b.webm"), Some("video/webm"));
        assert_eq!(guess_video_mime("x.mkv"), Some("video/x-matroska"));
    }

    #[test]
    fn guess_video_mime_rejects_non_video() {
        assert_eq!(guess_video_mime("notes.txt"), None);
        assert_eq!(guess_video_mime("image.png"), None);
        assert_eq!(guess_video_mime("noext"), None);
    }

    #[test]
    fn extract_text_from_string_content() {
        let v = serde_json::json!({
            "choices": [{ "message": { "content": "hello world" } }]
        });
        assert_eq!(extract_message_text(&v).as_deref(), Some("hello world"));
    }

    #[test]
    fn extract_text_from_array_content() {
        let v = serde_json::json!({
            "choices": [{ "message": { "content": [
                { "type": "text", "text": "part1 " },
                { "type": "text", "text": "part2" }
            ] } }]
        });
        assert_eq!(extract_message_text(&v).as_deref(), Some("part1 part2"));
    }

    #[test]
    fn extract_text_none_when_missing() {
        let v = serde_json::json!({ "choices": [] });
        assert!(extract_message_text(&v).is_none());
        let v2 = serde_json::json!({ "error": "boom" });
        assert!(extract_message_text(&v2).is_none());
    }
}
