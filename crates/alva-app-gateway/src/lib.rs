//! `alva-app-gateway` — HTTP routing gateway for Alva LLM providers.
//!
//! This crate turns a YAML routing file into a running HTTP proxy that
//! forwards chat-completion requests to upstream LLM providers, selecting
//! the right provider based on a URL-path alias.
//!
//! # Layers (planned)
//! * [`config`] — parse gateway YAML config → [`AliasRouter`]
//! * `server` (future task) — axum HTTP server
//! * `raw_tool` (future task) — [`RawTool`] dispatch
//!
//! [`AliasRouter`]: alva_llm_provider::AliasRouter

pub mod config;
