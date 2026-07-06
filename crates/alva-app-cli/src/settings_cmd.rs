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

use alva_app_core::components::{ComponentMeta, COMPONENTS};
use alva_app_core::config::{self, AlvaConfig, ProviderEntry};

const KNOWN_KINDS: &[&str] = &["anthropic", "openai-chat", "openai-responses", "gemini"];

struct ComponentRow {
    id: &'static str,
    enabled: bool,
    source: &'static str,
    meta: &'static ComponentMeta,
}

fn component_meta(id: &str) -> Option<&'static ComponentMeta> {
    COMPONENTS.iter().find(|meta| meta.id == id)
}

fn set_component_override(cfg: &mut AlvaConfig, id: &str, enabled: bool) -> Result<(), String> {
    if component_meta(id).is_none() {
        return Err(format!("unknown component: {id}"));
    }
    cfg.components.insert(id.to_string(), enabled);
    Ok(())
}

fn reset_component_override(cfg: &mut AlvaConfig, id: &str) -> Result<(), String> {
    if component_meta(id).is_none() {
        return Err(format!("unknown component: {id}"));
    }
    cfg.components.remove(id);
    Ok(())
}

fn component_rows(cfg: &AlvaConfig) -> Vec<ComponentRow> {
    COMPONENTS
        .iter()
        .map(|meta| match cfg.components.get(meta.id) {
            Some(enabled) => ComponentRow {
                id: meta.id,
                enabled: *enabled,
                source: "override",
                meta,
            },
            None => ComponentRow {
                id: meta.id,
                enabled: meta.default_on,
                source: "default",
                meta,
            },
        })
        .collect()
}

pub async fn run(args: &[String]) -> i32 {
    match args.first().map(|s| s.as_str()) {
        None | Some("list") => run_list().await,
        Some("get") => run_get(&args[1..]).await,
        Some("set") => run_set(&args[1..]).await,
        Some("active") => run_active(&args[1..]).await,
        Some("remove") | Some("rm") => run_remove(&args[1..]).await,
        Some("component") | Some("components") => run_component(&args[1..]).await,
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
    eprintln!("  alva settings component list          List component toggles");
    eprintln!("  alva settings component enable <id>   Force-enable a component");
    eprintln!("  alva settings component disable <id>  Force-disable a component");
    eprintln!("  alva settings component reset <id>    Remove one override");
    eprintln!("  alva settings component reset --all   Remove all overrides");
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
    eprintln!(
        "Providers in {}:",
        config::config_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<no home>".into())
    );
    for kind in kinds {
        let entry = &cfg.providers[kind];
        let star = if Some(kind.as_str()) == active {
            "*"
        } else {
            " "
        };
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
    }))
    .unwrap_or_default();
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
    let entry = cfg
        .providers
        .entry(kind.clone())
        .or_insert_with(ProviderEntry::default);
    if let Some(k) = api_key {
        entry.api_key = k;
    }
    if let Some(m) = model {
        entry.model = Some(m);
    }
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

fn parse_component_bool(value: &str) -> Option<bool> {
    match value {
        "on" | "true" | "1" | "yes" | "enable" | "enabled" => Some(true),
        "off" | "false" | "0" | "no" | "disable" | "disabled" => Some(false),
        _ => None,
    }
}

async fn run_component(args: &[String]) -> i32 {
    match args.first().map(|s| s.as_str()) {
        None | Some("list") => {
            let cfg = config::load().unwrap_or_default();
            eprintln!("Components:");
            eprintln!("  state  source    kind        category  id");
            for row in component_rows(&cfg) {
                let state = if row.enabled { "on" } else { "off" };
                let kind = match row.meta.kind {
                    alva_app_core::components::ComponentKind::Plugin => "plugin",
                    alva_app_core::components::ComponentKind::Middleware => "middleware",
                };
                eprintln!(
                    "  {state:<5}  {source:<8}  {kind:<10}  {category:<8}  {id}",
                    source = row.source,
                    category = row.meta.category,
                    id = row.id,
                );
            }
            0
        }
        Some("enable") | Some("on") => run_component_set(&args[1..], true).await,
        Some("disable") | Some("off") => run_component_set(&args[1..], false).await,
        Some("set") => {
            let Some(id) = args.get(1) else {
                eprintln!("usage: alva settings component set <id> <on|off>");
                return 2;
            };
            let Some(value) = args.get(2).and_then(|v| parse_component_bool(v)) else {
                eprintln!("usage: alva settings component set <id> <on|off>");
                return 2;
            };
            run_component_set(&[id.clone()], value).await
        }
        Some("reset") => run_component_reset(&args[1..]).await,
        Some("help") | Some("-h") | Some("--help") => {
            eprintln!("usage: alva settings component <list|enable|disable|set|reset>");
            0
        }
        Some(other) => {
            eprintln!("unknown component subcommand: {other}");
            2
        }
    }
}

async fn run_component_set(args: &[String], enabled: bool) -> i32 {
    let Some(id) = args.first() else {
        eprintln!(
            "usage: alva settings component {} <id>",
            if enabled { "enable" } else { "disable" }
        );
        return 2;
    };
    let mut cfg = config::load().unwrap_or_default();
    if let Err(e) = set_component_override(&mut cfg, id, enabled) {
        eprintln!("{e}");
        return 1;
    }
    match config::save(&cfg) {
        Ok(p) => {
            eprintln!(
                "Component {id} forced {} in {}",
                if enabled { "on" } else { "off" },
                p.display()
            );
            0
        }
        Err(e) => {
            eprintln!("save failed: {e}");
            1
        }
    }
}

async fn run_component_reset(args: &[String]) -> i32 {
    let Some(id) = args.first() else {
        eprintln!("usage: alva settings component reset <id|--all>");
        return 2;
    };
    let mut cfg = config::load().unwrap_or_default();
    if id == "--all" {
        cfg.components.clear();
    } else if let Err(e) = reset_component_override(&mut cfg, id) {
        eprintln!("{e}");
        return 1;
    }
    match config::save(&cfg) {
        Ok(p) => {
            eprintln!("Component override reset in {}", p.display());
            0
        }
        Err(e) => {
            eprintln!("save failed: {e}");
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
    if entry.api_key.is_empty()
        && std::env::var("ALVA_API_KEY")
            .ok()
            .filter(|s| !s.is_empty())
            .is_none()
    {
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
    // Canonical table lives in alva-llm-provider. The old local copy sent
    // openai-responses to the /v1 base — the provider appends /v1/responses
    // itself, so requests hit /v1/v1/responses and 404'd.
    alva_llm_provider::default_base_url(Some(kind))
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
        assert_eq!(
            default_model_for_kind("ANTHROPIC"),
            "gpt-4o",
            "match is case-sensitive"
        );
    }

    #[test]
    fn default_base_url_for_kind_known_providers() {
        assert_eq!(
            default_base_url_for_kind("anthropic"),
            "https://api.anthropic.com"
        );
        assert_eq!(
            default_base_url_for_kind("gemini"),
            "https://generativelanguage.googleapis.com"
        );
        // The /v1 asymmetry is load-bearing: the responses provider appends
        // /v1/responses ITSELF, so its base has no /v1 — the old local table
        // sent it to the /v1 base and requests 404'd on /v1/v1/responses.
        assert_eq!(
            default_base_url_for_kind("openai-chat"),
            "https://api.openai.com/v1"
        );
        assert_eq!(
            default_base_url_for_kind("openai-responses"),
            "https://api.openai.com"
        );
    }

    #[test]
    fn default_base_url_for_kind_unknown_falls_back_to_openai() {
        assert_eq!(default_base_url_for_kind(""), "https://api.openai.com/v1");
        assert_eq!(
            default_base_url_for_kind("xyz"),
            "https://api.openai.com/v1"
        );
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

    #[test]
    fn component_override_enable_disable_and_reset_mutates_config() {
        let mut cfg = config::AlvaConfig::default();

        set_component_override(&mut cfg, "browser", true).expect("browser is a known component");
        assert_eq!(cfg.components.get("browser"), Some(&true));

        set_component_override(&mut cfg, "browser", false).expect("browser is a known component");
        assert_eq!(cfg.components.get("browser"), Some(&false));

        reset_component_override(&mut cfg, "browser").expect("browser is a known component");
        assert!(!cfg.components.contains_key("browser"));
    }

    #[test]
    fn component_override_rejects_unknown_component() {
        let mut cfg = config::AlvaConfig::default();
        let err = set_component_override(&mut cfg, "not-a-component", true)
            .expect_err("unknown component should be rejected");
        assert!(err.contains("unknown component"), "{err}");
    }

    #[test]
    fn component_rows_report_effective_state_and_source() {
        let mut cfg = config::AlvaConfig::default();
        cfg.components.insert("browser".into(), true);
        cfg.components.insert("shell".into(), false);

        let rows = component_rows(&cfg);
        let browser = rows
            .iter()
            .find(|row| row.id == "browser")
            .expect("browser row");
        assert!(browser.enabled);
        assert_eq!(browser.source, "override");

        let shell = rows
            .iter()
            .find(|row| row.id == "shell")
            .expect("shell row");
        assert!(!shell.enabled);
        assert_eq!(shell.source, "override");

        let core = rows.iter().find(|row| row.id == "core").expect("core row");
        assert!(core.enabled);
        assert_eq!(core.source, "default");
    }
}
