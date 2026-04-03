use super::types::*;
use std::path::{Path, PathBuf};

/// Load settings from the 5-level cascade
/// Priority: policy > flag > local > project > user (later overrides earlier)
pub async fn load_settings(workspace: &Path, home_dir: &Path) -> Settings {
    let mut settings = Settings::default();

    // 1. User settings (lowest priority)
    let user_path = home_dir.join(".claude").join("settings.json");
    if let Some(user_settings) = load_settings_file(&user_path).await {
        merge_settings(&mut settings, &user_settings);
    }

    // 2. Project settings
    let project_path = workspace.join(".claude").join("settings.json");
    if let Some(project_settings) = load_settings_file(&project_path).await {
        merge_settings(&mut settings, &project_settings);
    }

    // 3. Local settings (git-ignored)
    let local_path = workspace.join(".claude").join("settings.local.json");
    if let Some(local_settings) = load_settings_file(&local_path).await {
        merge_settings(&mut settings, &local_settings);
    }

    // 4. Flag settings (would come from remote, skip for now)
    // 5. Policy settings (would come from remote, skip for now)

    settings
}

async fn load_settings_file(path: &Path) -> Option<Settings> {
    let content = tokio::fs::read_to_string(path).await.ok()?;
    serde_json::from_str(&content).ok()
}

/// Merge source settings into target (source overrides target for non-default values)
fn merge_settings(target: &mut Settings, source: &Settings) {
    // Merge permission rules (append, don't replace)
    target
        .permissions
        .allow
        .extend(source.permissions.allow.iter().cloned());
    target
        .permissions
        .deny
        .extend(source.permissions.deny.iter().cloned());
    target
        .permissions
        .ask
        .extend(source.permissions.ask.iter().cloned());

    // Merge env vars
    target
        .env
        .extend(source.env.iter().map(|(k, v)| (k.clone(), v.clone())));

    // Override scalar values if set
    if source.model.is_some() {
        target.model.clone_from(&source.model);
    }
    if source.theme.is_some() {
        target.theme.clone_from(&source.theme);
    }
    if source.system_prompt.is_some() {
        target.system_prompt.clone_from(&source.system_prompt);
    }
    if source.verbose {
        target.verbose = true;
    }
    if source.expand_output {
        target.expand_output = true;
    }
    if source.max_thinking_tokens.is_some() {
        target.max_thinking_tokens = source.max_thinking_tokens;
    }
    if source.api_base_url.is_some() {
        target.api_base_url.clone_from(&source.api_base_url);
    }
    if source.sandbox.is_some() {
        target.sandbox.clone_from(&source.sandbox);
    }

    // Merge trusted directories
    target
        .trusted_directories
        .extend(source.trusted_directories.iter().cloned());

    // Merge MCP servers
    target
        .mcp_servers
        .extend(source.mcp_servers.iter().map(|(k, v)| (k.clone(), v.clone())));

    // Merge hooks
    target
        .hooks
        .pre_tool_use
        .extend(source.hooks.pre_tool_use.iter().cloned());
    target
        .hooks
        .post_tool_use
        .extend(source.hooks.post_tool_use.iter().cloned());
    target
        .hooks
        .session_start
        .extend(source.hooks.session_start.iter().cloned());
    target
        .hooks
        .session_end
        .extend(source.hooks.session_end.iter().cloned());
    target
        .hooks
        .notification
        .extend(source.hooks.notification.iter().cloned());
}

/// Get all settings file paths for watching
pub fn settings_file_paths(workspace: &Path, home_dir: &Path) -> Vec<PathBuf> {
    vec![
        home_dir.join(".claude").join("settings.json"),
        workspace.join(".claude").join("settings.json"),
        workspace.join(".claude").join("settings.local.json"),
    ]
}
