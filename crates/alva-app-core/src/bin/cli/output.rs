//! Terminal output formatting for alva-cli.

use crossterm::style::Stylize;
use std::io::{self, Write};
use std::path::Path;

pub fn print_banner(model: &str, workspace: &str) {
    eprintln!(
        "{} {} {}",
        "╭".dark_grey(),
        "Alva Agent".bold().cyan(),
        format!("v{}", env!("CARGO_PKG_VERSION")).dark_grey(),
    );
    eprintln!("{}  Model: {}", "│".dark_grey(), model.yellow());
    eprintln!("{}  Workspace: {}", "│".dark_grey(), workspace.white());
}

pub fn print_banner_end() {
    eprintln!("{}", "╰───────────────────────────────────────".dark_grey());
}

pub fn print_git_status(workspace: &Path) {
    let branch = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(workspace)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    if let Some(branch) = branch {
        eprintln!("{}  Branch: {}", "│".dark_grey(), branch.magenta());

        let status = std::process::Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(workspace)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

        if let Some(status) = status {
            if status.is_empty() {
                eprintln!("{}  Status: {}", "│".dark_grey(), "clean".green());
            } else {
                let count = status.lines().count();
                eprintln!(
                    "{}  Status: {}",
                    "│".dark_grey(),
                    format!("{} changed files", count).yellow()
                );
            }
        }
    }
}

pub fn print_tool_start(name: &str) {
    eprintln!("  {} {} ...", "●".dark_yellow(), name.dark_yellow());
}

pub fn print_tool_end(name: &str, is_error: bool, preview: &str) {
    let preview_clean = preview.replace('\n', " ");
    let preview_short = if preview_clean.len() > 100 {
        format!("{}...", &preview_clean[..100])
    } else {
        preview_clean
    };
    if is_error {
        eprintln!("  {} {} {}", "✗".red(), name.red(), preview_short.dark_grey());
    } else {
        eprintln!(
            "  {} {} {}",
            "✓".green(),
            name.green(),
            preview_short.dark_grey()
        );
    }
}

pub fn print_error(msg: &str) {
    eprintln!("{} {}", "Error:".red().bold(), msg);
}

pub fn print_assistant_text(text: &str) {
    print!("{}", text);
    io::stdout().flush().ok();
}

pub fn print_divider() {
    eprintln!(
        "{}",
        "───────────────────────────────────────".dark_grey()
    );
}

pub fn print_help() {
    eprintln!("{}", "Commands:".bold());
    eprintln!("  {}        Start a fresh session", "/new".cyan());
    eprintln!("  {}     Resume a saved session", "/resume".cyan());
    eprintln!("  {}   List all sessions", "/sessions".cyan());
    eprintln!("  {}       Toggle plan mode (read-only)", "/plan".cyan());
    eprintln!("  {} Switch model (e.g. /model gpt-4o)", "/model [id]".cyan());
    eprintln!("  {}      Clear the terminal", "/clear".cyan());
    eprintln!("  {}     Show current config", "/config".cyan());
    eprintln!("  {}     Rewind to checkpoint", "/rewind".cyan());
    eprintln!("  {}       Show this help", "/help".cyan());
    eprintln!("  {}      Reconfigure provider/model", "/setup".cyan());
    eprintln!("  {} {}    Exit", "/quit".cyan(), "/exit".cyan());
    eprintln!();
    eprintln!("  {}       Run shell command directly", "!cmd".yellow());
}

pub fn print_prompt() {
    eprint!("{} ", ">".bold().cyan());
    io::stderr().flush().ok();
}

pub fn print_session_resumed(id: &str, count: usize, summary: &str) {
    eprintln!(
        "{}",
        format!(
            "Resuming session {} ({} messages) — \"{}\"",
            id, count, summary
        )
        .dark_grey()
    );
    eprintln!(
        "{}",
        "Type /new for fresh session, /help for commands.".dark_grey()
    );
}

pub fn print_session_new(id: &str) {
    eprintln!("{}", format!("New session: {}", id).dark_grey());
}

pub fn print_usage(input_tokens: u32, output_tokens: u32) {
    let total = input_tokens + output_tokens;
    eprintln!(
        "\x1b[90m  tokens: {} in / {} out / {} total\x1b[0m",
        input_tokens, output_tokens, total
    );
}

pub fn print_approval_prompt(tool_name: &str, args: &serde_json::Value) {
    eprintln!();
    eprintln!(
        "  {}",
        "╭─ Permission Required ──────────────────────────".dark_yellow()
    );
    eprintln!(
        "  {}  Tool: {}",
        "│".dark_yellow(),
        tool_name.yellow().bold()
    );

    // Show relevant arguments
    if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
        eprintln!("  {}  Command: {}", "│".dark_yellow(), cmd.white());
    }
    if let Some(path) = args
        .get("path")
        .or_else(|| args.get("file_path"))
        .and_then(|v| v.as_str())
    {
        eprintln!("  {}  Path: {}", "│".dark_yellow(), path.white());
    }
    if let Some(content) = args.get("content").and_then(|v| v.as_str()) {
        let preview = if content.len() > 80 {
            format!("{}...", &content[..80])
        } else {
            content.to_string()
        };
        eprintln!("  {}  Content: {}", "│".dark_yellow(), preview.dark_grey());
    }

    eprintln!(
        "  {}",
        "╰────────────────────────────────────────────────".dark_yellow()
    );
    eprint!(
        "  Allow? [{}]es / [{}]o / [{}]lways / [{}]eny always: ",
        "y".green().bold(),
        "n".red().bold(),
        "a".cyan().bold(),
        "d".red().bold(),
    );
    io::stderr().flush().ok();
}
