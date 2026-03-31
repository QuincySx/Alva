//! Interactive setup wizard — guides new users through provider/model configuration.
//!
//! Called when no valid config is found (no alva.json and no ALVA_API_KEY env var).
//! Saves the result to `alva.json` in the current workspace.

use crossterm::style::Stylize;
use std::io::{self, BufRead, Write};
use std::path::Path;

use alva_provider::ProviderConfig;

/// Known provider presets.
struct ProviderPreset {
    name: &'static str,
    base_url: &'static str,
    default_model: &'static str,
    needs_api_key: bool,
}

const PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        name: "OpenAI",
        base_url: "https://api.openai.com/v1",
        default_model: "gpt-4o",
        needs_api_key: true,
    },
    ProviderPreset {
        name: "DeepSeek",
        base_url: "https://api.deepseek.com/v1",
        default_model: "deepseek-chat",
        needs_api_key: true,
    },
    ProviderPreset {
        name: "Ollama (local)",
        base_url: "http://localhost:11434/v1",
        default_model: "llama3",
        needs_api_key: false,
    },
    ProviderPreset {
        name: "Azure OpenAI",
        base_url: "",
        default_model: "gpt-4o",
        needs_api_key: true,
    },
    ProviderPreset {
        name: "Custom (OpenAI-compatible)",
        base_url: "",
        default_model: "",
        needs_api_key: true,
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
    let key_preview = if api_key.len() > 8 {
        format!("{}...{}", &api_key[..4], &api_key[api_key.len()-4..])
    } else {
        "****".to_string()
    };
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
