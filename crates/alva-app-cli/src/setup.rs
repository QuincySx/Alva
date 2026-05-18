//! Interactive setup wizard — guides new users through provider/model configuration.
//!
//! Called when no valid config is found (no alva.json and no ALVA_API_KEY env var).
//! Saves the result to `alva.json` in the current workspace.

use crossterm::style::Stylize;
use std::io::{self, BufRead, Write};
use std::path::Path;

use alva_llm_provider::ProviderConfig;

/// Known provider presets. `kind` maps to the provider impl in
/// `alva_llm_provider` — see `alva-app-cli/src/agent_setup.rs` for the
/// dispatch.
struct ProviderPreset {
    name: &'static str,
    base_url: &'static str,
    default_model: &'static str,
    needs_api_key: bool,
    /// Provider impl kind. `"openai-chat"` is the broadest
    /// OpenAI-compatible path (OpenAI / DeepSeek / Ollama / custom).
    kind: &'static str,
}

const PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        name: "OpenAI",
        base_url: "https://api.openai.com/v1",
        default_model: "gpt-4o",
        needs_api_key: true,
        kind: "openai-chat",
    },
    ProviderPreset {
        name: "Anthropic",
        base_url: "https://api.anthropic.com",
        default_model: "claude-sonnet-4-6",
        needs_api_key: true,
        kind: "anthropic",
    },
    ProviderPreset {
        name: "Google Gemini",
        base_url: "https://generativelanguage.googleapis.com",
        default_model: "gemini-1.5-pro",
        needs_api_key: true,
        kind: "gemini",
    },
    ProviderPreset {
        name: "OpenAI (Responses API)",
        base_url: "https://api.openai.com",
        default_model: "gpt-4o",
        needs_api_key: true,
        kind: "openai-responses",
    },
    ProviderPreset {
        name: "DeepSeek",
        base_url: "https://api.deepseek.com/v1",
        default_model: "deepseek-chat",
        needs_api_key: true,
        kind: "openai-chat",
    },
    ProviderPreset {
        name: "Ollama (local)",
        base_url: "http://localhost:11434/v1",
        default_model: "llama3",
        needs_api_key: false,
        kind: "openai-chat",
    },
    ProviderPreset {
        name: "Azure OpenAI",
        base_url: "",
        default_model: "gpt-4o",
        needs_api_key: true,
        kind: "openai-chat",
    },
    ProviderPreset {
        name: "Custom (OpenAI-compatible)",
        base_url: "",
        default_model: "",
        needs_api_key: true,
        kind: "openai-chat",
    },
];

fn read_line_trimmed() -> String {
    let mut input = String::new();
    let _ = io::stdin().lock().read_line(&mut input);
    input.trim().to_string()
}

/// Run the interactive setup wizard. Returns a ProviderConfig on success.
pub fn run_setup_wizard(_workspace: &Path) -> Option<ProviderConfig> {
    eprintln!();
    eprintln!(
        "{} {} {}",
        "╭".dark_grey(),
        "Alva Agent".bold().cyan(),
        "— First-time Setup".dark_grey(),
    );
    eprintln!(
        "{}  {}",
        "│".dark_grey(),
        "No configuration found. Let's set up your AI provider.".white()
    );
    eprintln!(
        "{}",
        "╰───────────────────────────────────────────────────".dark_grey()
    );
    eprintln!();

    // Step 1: Pick provider
    eprintln!("{}", "Select a provider:".bold());
    for (i, preset) in PRESETS.iter().enumerate() {
        eprintln!("  {} {}", format!("[{}]", i + 1).cyan(), preset.name);
    }
    eprintln!();
    eprint!("{} ", "Choice [1]:".bold());
    io::stderr().flush().ok();

    let choice_str = read_line_trimmed();
    let choice: usize = if choice_str.is_empty() {
        1
    } else {
        match choice_str.parse() {
            Ok(n) if n >= 1 && n <= PRESETS.len() => n,
            _ => {
                eprintln!("{}", "Invalid choice.".red());
                return None;
            }
        }
    };
    let preset = &PRESETS[choice - 1];
    eprintln!();

    // Step 2: Base URL (for Azure/Custom, or confirm for others)
    let base_url = if preset.base_url.is_empty() {
        eprintln!(
            "{}",
            format!("Enter the API base URL for {}:", preset.name).bold()
        );
        eprint!(
            "  {} ",
            "URL:".bold()
        );
        io::stderr().flush().ok();
        let url = read_line_trimmed();
        if url.is_empty() {
            eprintln!("{}", "URL is required.".red());
            return None;
        }
        eprintln!();
        url
    } else {
        preset.base_url.to_string()
    };

    // Step 3: API Key
    let api_key = if preset.needs_api_key {
        eprintln!("{}", format!("Enter your {} API key:", preset.name).bold());
        eprint!("  {} ", "Key:".bold());
        io::stderr().flush().ok();
        let key = read_line_trimmed();
        if key.is_empty() {
            eprintln!("{}", "API key is required.".red());
            return None;
        }
        eprintln!();
        key
    } else {
        // Local providers like Ollama don't need a key
        "ollama".to_string()
    };

    // Step 4: Model
    let default_model_hint = if preset.default_model.is_empty() {
        String::new()
    } else {
        format!(" [{}]", preset.default_model)
    };

    eprintln!("{}", "Enter model name:".bold());
    eprint!(
        "  {}{}",
        "Model".bold(),
        format!("{}: ", default_model_hint).dark_grey()
    );
    io::stderr().flush().ok();

    let model_input = read_line_trimmed();
    let model = if model_input.is_empty() {
        if preset.default_model.is_empty() {
            eprintln!("{}", "Model name is required.".red());
            return None;
        }
        preset.default_model.to_string()
    } else {
        model_input
    };
    eprintln!();

    // Step 5: Confirm
    let key_preview = mask_api_key(&api_key);
    eprintln!("{}", "Configuration:".bold());
    eprintln!("  Provider:  {}", preset.name.cyan());
    eprintln!("  Base URL:  {}", base_url.as_str().white());
    eprintln!("  API Key:   {}", key_preview.dark_grey());
    eprintln!("  Model:     {}", model.as_str().yellow());
    eprintln!();
    eprint!("{} ", "Save to alva.json? [Y/n]:".bold());
    io::stderr().flush().ok();

    let confirm = read_line_trimmed();
    if confirm.eq_ignore_ascii_case("n") {
        eprintln!("{}", "Setup cancelled.".dark_grey());
        return None;
    }

    let config = ProviderConfig {
        api_key,
        model,
        base_url,
        max_tokens: 8192,
        custom_headers: std::collections::HashMap::new(),
        kind: Some(preset.kind.to_string()),
    };

    // Save to global config (~/.config/alva/config.json)
    match config.save_global() {
        Ok(path) => {
            eprintln!(
                "  {} {}",
                "Saved".green().bold(),
                path.display().to_string().dark_grey()
            );
        }
        Err(e) => {
            eprintln!(
                "  {} {}",
                "Warning:".yellow().bold(),
                format!("Could not save global config: {}", e)
            );
            eprintln!("  You can set env vars instead: ALVA_API_KEY, ALVA_MODEL, ALVA_BASE_URL");
        }
    }

    eprintln!();
    Some(config)
}

/// Mask an API key for display as `<first-4>...<last-4>`. Operates on
/// chars (not bytes) so it never panics on multi-byte UTF-8 chars —
/// API keys are almost always ASCII, but a user could paste anything
/// here, and a panic during the setup flow would break first-run.
///
/// Keys with 8 or fewer chars are masked entirely as `****` to avoid
/// leaking most of a short key.
fn mask_api_key(key: &str) -> String {
    let total_chars = key.chars().count();
    if total_chars <= 8 {
        return "****".to_string();
    }
    let first_4: String = key.chars().take(4).collect();
    // Take the last 4 chars by reversing, taking 4, then reversing back.
    let last_4: String = key
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{}...{}", first_4, last_4)
}

#[cfg(test)]
mod tests {
    //! Tests for `mask_api_key` — the chars-based replacement for the
    //! old `&api_key[..4]` / `&api_key[len-4..]` byte slicing that
    //! could panic if a user pasted unicode content as their API key.
    use super::*;

    #[test]
    fn mask_api_key_typical_ascii_key_shows_first_and_last_four() {
        // Realistic Anthropic-shaped key (35 chars).
        let key = "sk-ant-1234567890abcdefghijklmnopqr";
        assert_eq!(key.chars().count(), 35);
        assert_eq!(mask_api_key(key), "sk-a...opqr");
    }

    #[test]
    fn mask_api_key_short_key_masked_entirely() {
        // ≤8 chars → "****", don't leak any prefix/suffix
        assert_eq!(mask_api_key(""), "****");
        assert_eq!(mask_api_key("abcd"), "****");
        assert_eq!(mask_api_key("abcdefgh"), "****", "exactly 8 chars → masked");
    }

    #[test]
    fn mask_api_key_boundary_9_chars_starts_showing_preview() {
        // 9 chars is the smallest size that shows preview.
        assert_eq!(mask_api_key("abcdefghi"), "abcd...fghi");
    }

    #[test]
    fn mask_api_key_unicode_chars_no_panic_and_correct_count() {
        // Regression: pre-fix `&api_key[..4]` would panic if byte 4 was
        // mid-char. With chars()-based we want "first 4 chars" semantics:
        // 9 CJK chars = 27 bytes. Old code: `&[..4]` is bytes 0..4 which
        // is INSIDE the 2nd CJK char (bytes 3..6) → PANIC.
        let key = "中文中文中文中文中"; // 9 chars, 27 bytes
        assert_eq!(key.chars().count(), 9);
        assert_eq!(key.len(), 27);
        // Must not panic. First 4 chars + "..." + last 4 chars.
        let masked = mask_api_key(key);
        assert_eq!(masked, "中文中文...文中文中");
    }

    #[test]
    fn mask_api_key_emoji_in_middle_no_panic() {
        // Mix of ASCII and emoji somewhere in the key — the byte
        // positions of the boundary chars matter for the old code,
        // but with chars() it just works.
        let key = "sk-🦀-test-key-abc";
        // count chars not bytes — should be > 8 → preview shown
        assert!(key.chars().count() > 8);
        let masked = mask_api_key(key);
        assert!(masked.starts_with("sk-🦀"), "first 4 chars include the emoji: {}", masked);
        assert!(masked.contains("..."));
    }
}
