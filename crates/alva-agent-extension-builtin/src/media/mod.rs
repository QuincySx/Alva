// INPUT:  kimi_video
// OUTPUT: video understanding core (backend-agnostic, reusable)
// POS:    Layer-1 reusable capability — turns a video into text via an
//         OpenAI-compatible multimodal backend (Kimi/Moonshot by default).
//         Decoupled from the Tool/agent layer so it can be reused by a
//         Tool today and a video sub-agent later.
//! media — reusable multimodal capabilities (currently: video → text)

#[cfg(all(feature = "web", not(target_family = "wasm")))]
pub mod kimi_video;
