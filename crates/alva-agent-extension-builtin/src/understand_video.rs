// INPUT:  alva_kernel_abi, schemars, serde, tokio, crate::media::kimi_video
// OUTPUT: UnderstandVideoTool
// POS:    Layer-2 thin Tool over the reusable `media::kimi_video` core.
//         Takes a local video path, sends it to a multimodal backend
//         (Kimi/Moonshot), and returns the model's text understanding so
//         any agent — even one whose own model can't see video — can "watch"
//         a clip. The interchange back into the agent loop is TEXT.
//! understand_video — describe/understand a local video via an external multimodal model

use alva_kernel_abi::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::media::kimi_video::{self, VideoInput, VideoUnderstandConfig};

/// Maximum video size accepted (matches the backend's 100MB limit).
const MAX_VIDEO_BYTES: u64 = 100 * 1024 * 1024;

/// Default instruction when the caller doesn't supply one.
const DEFAULT_PROMPT: &str = "Watch this video and describe what happens in detail, \
    in chronological order, including any on-screen text, UI elements, or code.";

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Path to a local video file. Absolute, or relative to the workspace.
    /// Supported: mp4, mov, webm, mkv, avi, m4v, flv, wmv, mpg/mpeg, ogv, 3gp.
    path: String,
    /// What to focus on or ask about the video. Optional; defaults to a
    /// full chronological description.
    #[serde(default)]
    prompt: Option<String>,
}

#[derive(Tool)]
#[tool(
    name = "understand_video",
    description = "Understand a local video file by sending it to an external multimodal model \
        (Kimi/Moonshot) and returning a text description. Use this to 'watch' a video — \
        summarize it, extract steps/UI flows, or answer questions about its content. \
        Returns text only. Requires KIMI_API_KEY (or MOONSHOT_API_KEY) in the environment. \
        Max 100MB.",
    input = Input,
    read_only,
)]
pub struct UnderstandVideoTool;

impl UnderstandVideoTool {
    async fn execute_impl(
        &self,
        params: Input,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let path = resolve_path(&params.path, ctx.workspace());

        // Type check by extension before touching the network.
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("video")
            .to_string();
        let Some(mime_type) = kimi_video::guess_video_mime(&filename) else {
            return Ok(ToolOutput::error(format!(
                "\"{}\" is not a recognized video file. Supported: mp4, mov, webm, mkv, avi, m4v, flv, wmv, mpg, mpeg, ogv, 3gp.",
                params.path
            )));
        };

        // Existence + size guard.
        match tokio::fs::metadata(&path).await {
            Ok(meta) if !meta.is_file() => {
                return Ok(ToolOutput::error(format!(
                    "\"{}\" is not a file.",
                    params.path
                )));
            }
            Ok(meta) if meta.len() == 0 => {
                return Ok(ToolOutput::error(format!("\"{}\" is empty.", params.path)));
            }
            Ok(meta) if meta.len() > MAX_VIDEO_BYTES => {
                return Ok(ToolOutput::error(format!(
                    "\"{}\" is {:.1} MB, which exceeds the {} MB limit.",
                    params.path,
                    meta.len() as f64 / 1024.0 / 1024.0,
                    MAX_VIDEO_BYTES / 1024 / 1024
                )));
            }
            Ok(_) => {}
            Err(e) => {
                return Ok(ToolOutput::error(format!(
                    "Cannot read \"{}\": {e}",
                    params.path
                )));
            }
        }

        // Backend config from environment.
        let config = match VideoUnderstandConfig::from_env() {
            Ok(c) => c,
            Err(e) => return Ok(ToolOutput::error(e.to_string())),
        };

        // Read bytes.
        let bytes = match tokio::fs::read(&path).await {
            Ok(b) => b,
            Err(e) => {
                return Ok(ToolOutput::error(format!(
                    "Failed to read \"{}\": {e}",
                    params.path
                )))
            }
        };

        let prompt = params
            .prompt
            .as_deref()
            .filter(|p| !p.trim().is_empty())
            .unwrap_or(DEFAULT_PROMPT)
            .to_string();

        let input = VideoInput {
            bytes,
            filename,
            mime_type: mime_type.to_string(),
        };

        // Run with cancellation support — uploads can be slow.
        let mut cancel = ctx.cancel_token().clone();
        tokio::select! {
            result = kimi_video::understand_video(input, &prompt, &config) => match result {
                Ok(text) => Ok(ToolOutput::text(text)),
                Err(e) => Ok(ToolOutput::error(format!("Video understanding failed: {e}"))),
            },
            _ = cancel.cancelled() => Ok(ToolOutput::error("Video understanding cancelled.")),
        }
    }
}

/// Resolve a possibly-relative path against the workspace (or CWD).
fn resolve_path(path: &str, workspace: Option<&Path>) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        return p.to_path_buf();
    }
    match workspace {
        Some(ws) => ws.join(p),
        None => p.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_kernel_abi::CancellationToken;
    use serde_json::json;
    use std::any::Any;

    struct TestContext {
        cancel: CancellationToken,
        workspace: Option<PathBuf>,
    }

    impl ToolExecutionContext for TestContext {
        fn cancel_token(&self) -> &CancellationToken {
            &self.cancel
        }
        fn session_id(&self) -> &str {
            "test-session"
        }
        fn workspace(&self) -> Option<&Path> {
            self.workspace.as_deref()
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    fn ctx() -> TestContext {
        TestContext {
            cancel: CancellationToken::new(),
            workspace: None,
        }
    }

    // ─── path resolution ──────────────────────────────────────────────

    #[test]
    fn resolve_path_absolute_is_unchanged() {
        let abs = if cfg!(windows) {
            r"C:\v\a.mp4"
        } else {
            "/v/a.mp4"
        };
        assert_eq!(
            resolve_path(abs, Some(Path::new("/ws"))),
            PathBuf::from(abs)
        );
    }

    #[test]
    fn resolve_path_relative_joins_workspace() {
        assert_eq!(
            resolve_path("clips/a.mp4", Some(Path::new("/ws"))),
            PathBuf::from("/ws/clips/a.mp4")
        );
    }

    #[test]
    fn resolve_path_relative_without_workspace_is_relative() {
        assert_eq!(resolve_path("a.mp4", None), PathBuf::from("a.mp4"));
    }

    // ─── early validation (no network) ────────────────────────────────

    #[tokio::test]
    async fn rejects_non_video_extension() {
        let out = UnderstandVideoTool
            .execute(json!({ "path": "notes.txt" }), &ctx())
            .await
            .expect("should resolve");
        assert!(out.is_error);
        assert!(out.model_text().contains("not a recognized video"));
    }

    #[tokio::test]
    async fn rejects_missing_file_before_network() {
        // .mp4 passes the extension check, then metadata() fails → readable error,
        // and crucially we never reach the backend (no API key needed here).
        let out = UnderstandVideoTool
            .execute(json!({ "path": "/no/such/clip.mp4" }), &ctx())
            .await
            .expect("should resolve");
        assert!(out.is_error);
        assert!(out.model_text().contains("Cannot read"));
    }

    // ─── live integration (opt-in) ────────────────────────────────────

    /// End-to-end against the real backend. Ignored by default.
    /// Run with: `KIMI_API_KEY=sk-... KIMI_TEST_VIDEO=/path/clip.mp4 \
    ///   cargo test -p alva-agent-extension-builtin --features web \
    ///   understand_video_live -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "hits the real Kimi API; needs KIMI_API_KEY + KIMI_TEST_VIDEO"]
    async fn understand_video_live() {
        let video =
            std::env::var("KIMI_TEST_VIDEO").expect("set KIMI_TEST_VIDEO to a local video path");
        let out = UnderstandVideoTool
            .execute(
                json!({ "path": video, "prompt": "Summarize this video." }),
                &ctx(),
            )
            .await
            .expect("should resolve");
        eprintln!("is_error={}\n{}", out.is_error, out.model_text());
        assert!(!out.is_error, "live call failed: {}", out.model_text());
        assert!(!out.model_text().trim().is_empty());
    }
}
