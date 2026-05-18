// INPUT:  alva_app_core::config (AlvaConfig, ProviderEntry), serde_json
// OUTPUT: run(args): dispatcher for `alva settings ...` subcommand
// POS:    Read/write `~/.alva/config.json` — the same multi-provider config
//         alva-app-tauri reads. Lets users manage providers from CLI without
//         opening the Tauri Settings modal, and surfaces what's stored so
//         both apps stay in sync.

//! `alva settings` — manage the shared multi-provider config at `~/.alva/config.json`.
//!
//! Subcommands:
//!
//! - `alva settings`                — list all providers, mark active
//! - `alva settings list`           — same as above
//! - `alva settings get <kind>`     — show one provider's config
//! - `alva settings set <kind> [--api-key K] [--model M] [--base-url U]`
//!                                  — upsert a provider entry
//! - `alva settings active <kind>`  — set the active provider
//! - `alva settings remove <kind>`  — drop a provider entry
//! - `alva settings path`           — print the config file path
//!
//! The file format matches what alva-app-tauri reads, so changes here are
//! visible to the desktop app on its next IPC call (the Tauri side never
//! caches — re-reads the file every `lookup_provider`).

use alva_app_core::config::{self, ProviderEntry};

const KNOWN_KINDS: &[&str] = &["anthropic", "openai-chat", "openai-responses", "gemini"];

pub async fn run(args: &[String]) -> i32 {
    match args.first().map(|s| s.as_str()) {
        None | Some("list") => run_list().await,
        Some("get") => run_get(&args[1..]).await,
        Some("set") => run_set(&args[1..]).await,
        Some("active") => run_active(&args[1..]).await,
        Some("remove") | Some("rm") => run_remove(&args[1..]).await,
        Some("path") => run_path().await,
        Some("help") | Some("-h") | Some("--help") => {
            print_help();
            0
        }
        Some(other) => {
            eprintln!("unknown subcommand: {other}\n");
            print_help();
            2
        }
    }
}

fn print_help() {
    eprintln!("alva settings — manage ~/.alva/config.json (shared with the Tauri app)\n");
    eprintln!("Usage:");
    eprintln!("  alva settings                         List all providers");
    eprintln!("  alva settings get <kind>              Show one provider");
    eprintln!("  alva settings set <kind> [flags]      Upsert a provider");
    eprintln!("    flags: --api-key K  --model M  --base-url U");
    eprintln!("  alva settings active <kind>           Set the active provider");
    eprintln!("  alva settings remove <kind>           Drop a provider");
    eprintln!("  alva settings path                    Print config file path\n");
    eprintln!("Known provider kinds: {}", KNOWN_KINDS.join(", "));
}

fn mask(api_key: &str) -> String {
    if api_key.is_empty() {
        return String::from("(empty)");
    }
    let n = api_key.chars().count();
    if n <= 8 {
        return "•".repeat(n);
    }
    let head: String = api_key.chars().take(4).collect();
    let tail: String = api_key.chars().skip(n.saturating_sub(4)).collect();
    format!("{}…{}", head, tail)
}

async fn run_list() -> i32 {
    let cfg = config::load().unwrap_or_default();
    if cfg.providers.is_empty() {
        eprintln!("No providers configured.");
        eprintln!("Add one: alva settings set anthropic --api-key sk-... --model claude-opus-4-7");
        return 0;
    }
    let active = cfg.active.as_deref();
    let mut kinds: Vec<&String> = cfg.providers.keys().collect();
    kinds.sort();
    eprintln!("Providers in {}:",
        config::config_path().map(|p| p.display().to_string()).unwrap_or_else(|| "<no home>".into()));
    for kind in kinds {
        let entry = &cfg.providers[kind];
        let star = if Some(kind.as_str()) == active { "*" } else { " " };
        eprintln!(
            "  {star} {kind:<20}  key={key}  model={model}  base_url={base}",
            kind = kind,
            key = mask(&entry.api_key),
            model = entry.model.as_deref().unwrap_or("(unset)"),
            base = entry.base_url.as_deref().unwrap_or("(default)"),
        );
    }
    if active.is_none() {
        eprintln!("\nNo active provider set. Use: alva settings active <kind>");
    }
    0
}

async fn run_get(args: &[String]) -> i32 {
    let Some(kind) = args.first() else {
        eprintln!("usage: alva settings get <kind>");
        return 2;
    };
    let cfg = config::load().unwrap_or_default();
    let Some(entry) = cfg.providers.get(kind) else {
        eprintln!("provider not configured: {kind}");
        return 1;
    };
    let json = serde_json::to_string_pretty(&serde_json::json!({
        "kind": kind,
        "api_key": mask(&entry.api_key),
        "model": entry.model,
        "base_url": entry.base_url,
        "active": cfg.active.as_deref() == Some(kind.as_str()),
    })).unwrap_or_default();
    println!("{json}");
    0
}

async fn run_set(args: &[String]) -> i32 {
    let Some(kind) = args.first() else {
        eprintln!("usage: alva settings set <kind> [--api-key K] [--model M] [--base-url U]");
        return 2;
    };
    let mut api_key: Option<String> = None;
    let mut model: Option<String> = None;
    let mut base_url: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        let key = &args[i];
        let val = args.get(i + 1).cloned();
        match key.as_str() {
            "--api-key" => api_key = val,
            "--model" => model = val,
            "--base-url" => base_url = val,
            other => {
                eprintln!("unknown flag: {other}");
                return 2;
            }
        }
        i += 2;
    }

    let mut cfg = config::load().unwrap_or_default();
    let entry = cfg.providers.entry(kind.clone()).or_insert_with(ProviderEntry::default);
    if let Some(k) = api_key { entry.api_key = k; }
    if let Some(m) = model { entry.model = Some(m); }
    if let Some(b) = base_url {
        entry.base_url = if b.is_empty() { None } else { Some(b) };
    }
    // First-set: also become active if no active set yet.
    if cfg.active.is_none() {
        cfg.active = Some(kind.clone());
    }
    match config::save(&cfg) {
        Ok(p) => {
            eprintln!("Saved {kind} to {}", p.display());
            0
        }
        Err(e) => {
            eprintln!("save failed: {e}");
            1
        }
    }
}

async fn run_active(args: &[String]) -> i32 {
    let Some(kind) = args.first() else {
        let cfg = config::load().unwrap_or_default();
        match cfg.active {
            Some(k) => println!("{k}"),
            None => {
                eprintln!("no active provider set");
                return 1;
            }
        }
        return 0;
    };
    let mut cfg = config::load().unwrap_or_default();
    if !cfg.providers.contains_key(kind) {
        eprintln!("provider not configured: {kind}");
        eprintln!("set it first: alva settings set {kind} --api-key sk-...");
        return 1;
    }
    cfg.active = Some(kind.clone());
    match config::save(&cfg) {
        Ok(p) => {
            eprintln!("Active provider set to {kind} in {}", p.display());
            0
        }
        Err(e) => {
            eprintln!("save failed: {e}");
            1
        }
    }
}

async fn run_remove(args: &[String]) -> i32 {
    let Some(kind) = args.first() else {
        eprintln!("usage: alva settings remove <kind>");
        return 2;
    };
    let mut cfg = config::load().unwrap_or_default();
    if cfg.providers.remove(kind).is_none() {
        eprintln!("not configured: {kind}");
        return 1;
    }
    if cfg.active.as_deref() == Some(kind.as_str()) {
        // Pick a remaining provider as active, or clear it.
        cfg.active = cfg.providers.keys().next().cloned();
    }
    match config::save(&cfg) {
        Ok(p) => {
            eprintln!("Removed {kind} from {}", p.display());
            0
        }
        Err(e) => {
            eprintln!("save failed: {e}");
            1
        }
    }
}

async fn run_path() -> i32 {
    match config::config_path() {
        Some(p) => {
            println!("{}", p.display());
            0
        }
        None => {
            eprintln!("could not resolve home directory");
            1
        }
    }
}

/// Public helper: try to load a `ProviderConfig` from the shared
/// `~/.alva/config.json` (active provider). Returns `None` if no shared
/// config exists or no active provider is set. Used by main.rs as a layer
/// between env vars and the legacy CLI-only config paths.
pub fn try_load_provider_from_shared() -> Option<alva_llm_provider::ProviderConfig> {
    let cfg = config::load()?;
    let (kind, entry) = cfg.active_provider()?;
    if entry.api_key.is_empty() && std::env::var("ALVA_API_KEY").ok().filter(|s| !s.is_empty()).is_none() {
        return None;
    }
    let model = entry
        .model
        .clone()
        .unwrap_or_else(|| default_model_for_kind(kind).to_string());
    let base_url = entry
        .base_url
        .clone()
        .unwrap_or_else(|| default_base_url_for_kind(kind).to_string());
    Some(alva_llm_provider::ProviderConfig {
        api_key: entry.api_key.clone(),
        model,
        base_url,
        max_tokens: 32_000,
        custom_headers: std::collections::HashMap::new(),
        kind: Some(kind.to_string()),
    })
}

/// Default model name to use when a provider entry has no `model` set.
/// Unknown kinds fall back to a sensible OpenAI default rather than
/// erroring — the user sees a working config and can override later.
fn default_model_for_kind(kind: &str) -> &'static str {
    match kind {
        "anthropic" => "claude-opus-4-7",
        "openai-chat" | "openai-responses" => "gpt-4o",
        "gemini" => "gemini-2.5-pro",
        _ => "gpt-4o",
    }
}

/// Default base URL to use when a provider entry has no `base_url` set.
/// openai-chat / openai-responses / unknown all fall through to the
/// official OpenAI endpoint.
fn default_base_url_for_kind(kind: &str) -> &'static str {
    match kind {
        "anthropic" => "https://api.anthropic.com",
        "gemini" => "https://generativelanguage.googleapis.com",
        _ => "https://api.openai.com/v1",
    }
}

#[cfg(test)]
mod tests {
    //! Tests for `mask` — the API-key display helper used by `alva
    //! settings list/get`. Already correctly uses `chars()`-based
    //! truncation (chars-based slicing was the team's established
    //! pattern; setup.rs's byte-slice variant fixed in L63 was the
    //! outlier). These tests pin down the existing contract:
    //! - empty   → "(empty)"
    //! - n ≤ 8   → "•" repeated n times (don't leak short keys)
    //! - n > 8   → "<first-4 chars>…<last-4 chars>" (note: ASCII ellipsis
    //!   `…` is one char, not "...")
    //!
    //! Note: setup.rs has a parallel `mask_api_key` with a slightly
    //! different output format (`...` not `…`, "****" not "•"*n). Both
    //! coexist; consolidation requires picking one canonical format,
    //! which is a user-visible decision deferred to a later loop.
    use super::*;

    #[test]
    fn mask_empty_string_returns_empty_placeholder() {
        assert_eq!(mask(""), "(empty)");
    }

    #[test]
    fn mask_short_keys_render_as_bullet_repeat() {
        // n ≤ 8 → "•" repeated exactly n times so the displayed length
        // tells the reader the key length without revealing characters.
        assert_eq!(mask("a"), "•");
        assert_eq!(mask("abcd"), "••••");
        assert_eq!(mask("abcdefgh"), "••••••••", "exactly 8 chars → 8 bullets");
    }

    #[test]
    fn mask_nine_chars_starts_showing_preview_with_ellipsis() {
        // 9-char boundary: first 4 + … + last 4. Note overlap intentional:
        // when n=9, head="abcd" and tail starts at skip(5) → "fghi" so
        // char 'e' (index 4) is the only one hidden.
        assert_eq!(mask("abcdefghi"), "abcd…fghi");
    }

    #[test]
    fn mask_typical_ascii_key_shows_first_and_last_four() {
        let key = "sk-ant-1234567890abcdefghijklmnopqr";
        assert_eq!(key.chars().count(), 35);
        assert_eq!(mask(key), "sk-a…opqr");
    }

    #[test]
    fn mask_cjk_chars_no_panic_chars_based() {
        // 9 CJK chars = 27 bytes. Old byte-slice would PANIC at byte 4
        // (mid-2nd-char). chars-based is fine. First 4 + … + last 4.
        let key = "中文中文中文中文中"; // 9 chars
        assert_eq!(key.chars().count(), 9);
        assert_eq!(key.len(), 27);
        assert_eq!(mask(key), "中文中文…文中文中");
    }

    #[test]
    fn mask_emoji_mixed_in_key_no_panic() {
        // chars().count() treats each emoji as 1 char regardless of byte width.
        let key = "sk-🦀-test-key-abc"; // 17 chars
        assert!(key.chars().count() > 8);
        let masked = mask(key);
        // first 4 chars: "sk-🦀"
        assert!(masked.starts_with("sk-🦀"), "got: {}", masked);
        assert!(masked.contains('…'));
    }

    // -- default_model_for_kind / default_base_url_for_kind --------------

    #[test]
    fn default_model_for_kind_known_providers() {
        assert_eq!(default_model_for_kind("anthropic"), "claude-opus-4-7");
        assert_eq!(default_model_for_kind("openai-chat"), "gpt-4o");
        assert_eq!(default_model_for_kind("openai-responses"), "gpt-4o");
        assert_eq!(default_model_for_kind("gemini"), "gemini-2.5-pro");
    }

    #[test]
    fn default_model_for_kind_unknown_falls_back_to_gpt4o() {
        assert_eq!(default_model_for_kind(""), "gpt-4o");
        assert_eq!(default_model_for_kind("future-provider"), "gpt-4o");
        assert_eq!(default_model_for_kind("ANTHROPIC"), "gpt-4o", "match is case-sensitive");
    }

    #[test]
    fn default_base_url_for_kind_known_providers() {
        assert_eq!(default_base_url_for_kind("anthropic"), "https://api.anthropic.com");
        assert_eq!(
            default_base_url_for_kind("gemini"),
            "https://generativelanguage.googleapis.com"
        );
        // Both openai-chat and openai-responses share the openai endpoint.
        assert_eq!(default_base_url_for_kind("openai-chat"), "https://api.openai.com/v1");
        assert_eq!(default_base_url_for_kind("openai-responses"), "https://api.openai.com/v1");
    }

    #[test]
    fn default_base_url_for_kind_unknown_falls_back_to_openai() {
        assert_eq!(default_base_url_for_kind(""), "https://api.openai.com/v1");
        assert_eq!(default_base_url_for_kind("xyz"), "https://api.openai.com/v1");
    }

    #[test]
    fn known_kinds_all_have_explicit_defaults() {
        // Invariant guard: every entry in KNOWN_KINDS must have a
        // non-fallback default for both model and base_url. Without
        // this test, a future commit could add a kind to KNOWN_KINDS
        // (so `alva settings set <kind>` validates it) but forget to
        // teach the defaults — users get the openai fallback silently.
        //
        // We can't introspect "did the match arm hit the fallback?",
        // but we can encode the expectation: every known kind should
        // produce defaults that are NOT both the bare openai fallbacks.
        // Iterate and verify each kind has at least one of model/base_url
        // distinct from the generic fallback pair (gpt-4o, openai.com).
        let openai_fallback_model = default_model_for_kind("__definitely_unknown__");
        let openai_fallback_url = default_base_url_for_kind("__definitely_unknown__");
        for kind in KNOWN_KINDS {
            let model = default_model_for_kind(kind);
            let url = default_base_url_for_kind(kind);
            // openai-chat/responses are themselves intended to hit the
            // openai fallback, so the assertion holds only for the
            // non-openai kinds.
            if !kind.starts_with("openai") {
                assert!(
                    model != openai_fallback_model || url != openai_fallback_url,
                    "kind `{}` falls through to the generic openai default; \
                     was a new provider added to KNOWN_KINDS without teaching \
                     default_model_for_kind / default_base_url_for_kind?",
                    kind,
                );
            }
        }
    }
}
