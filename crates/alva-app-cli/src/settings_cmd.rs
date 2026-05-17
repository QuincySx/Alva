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
    let model = entry.model.clone().unwrap_or_else(|| match kind {
        "anthropic" => "claude-opus-4-7".to_string(),
        "openai-chat" | "openai-responses" => "gpt-4o".to_string(),
        "gemini" => "gemini-2.5-pro".to_string(),
        _ => "gpt-4o".to_string(),
    });
    let base_url = entry.base_url.clone().unwrap_or_else(|| match kind {
        "anthropic" => "https://api.anthropic.com".to_string(),
        "gemini" => "https://generativelanguage.googleapis.com".to_string(),
        // openai-chat / openai-responses / unknown
        _ => "https://api.openai.com/v1".to_string(),
    });
    Some(alva_llm_provider::ProviderConfig {
        api_key: entry.api_key.clone(),
        model,
        base_url,
        max_tokens: 32_000,
        custom_headers: std::collections::HashMap::new(),
        kind: Some(kind.to_string()),
    })
}
