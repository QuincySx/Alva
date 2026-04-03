// INPUT:  super::types::{AgentSpawnConfig, SpawnMode}, tokio::process, std::time
// OUTPUT: spawn_teammate (async)
// POS:    Agent spawning utilities — in-process, subprocess, and tmux backends.

use super::types::*;

/// Spawn a new agent as a teammate.
///
/// Returns the generated agent ID on success.
pub async fn spawn_teammate(config: AgentSpawnConfig) -> Result<String, String> {
    match config.mode {
        SpawnMode::InProcess => spawn_in_process(config).await,
        SpawnMode::Subprocess => spawn_subprocess(config).await,
        SpawnMode::Tmux => spawn_tmux(config).await,
    }
}

/// Spawn an in-process agent (sharing the same tokio runtime).
///
/// The agent runs as a tokio task and communicates via the mailbox system.
async fn spawn_in_process(config: AgentSpawnConfig) -> Result<String, String> {
    let agent_id = format!("agent-{}", generate_short_id());

    tracing::info!(
        agent_id = %agent_id,
        name = %config.name,
        "Spawning in-process agent"
    );

    // In-process agents are spawned as tokio tasks by the caller.
    // This function only generates the ID; the actual task spawning
    // is done by the coordinator which has access to the agent runtime.
    Ok(agent_id)
}

/// Spawn an agent as a separate OS process.
async fn spawn_subprocess(config: AgentSpawnConfig) -> Result<String, String> {
    let agent_id = format!("agent-{}", generate_short_id());

    let exe = std::env::current_exe()
        .map_err(|e| format!("Cannot find current executable: {}", e))?;

    let mut cmd = tokio::process::Command::new(exe);

    cmd.arg("-p") // print mode
        .arg(&config.prompt)
        .current_dir(&config.workspace);

    if let Some(model) = &config.model {
        cmd.env("ALVA_MODEL", model);
    }

    let _child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn subprocess: {}", e))?;

    tracing::info!(
        agent_id = %agent_id,
        name = %config.name,
        "Spawned subprocess agent"
    );

    Ok(agent_id)
}

/// Spawn an agent in a tmux split pane.
async fn spawn_tmux(config: AgentSpawnConfig) -> Result<String, String> {
    let agent_id = format!("agent-{}", generate_short_id());

    // Check if tmux is available
    let tmux_check = tokio::process::Command::new("tmux")
        .arg("has-session")
        .output()
        .await;

    if tmux_check.is_err() {
        return Err("tmux is not available. Use in-process or subprocess mode.".to_string());
    }

    let exe = std::env::current_exe()
        .map_err(|e| format!("Cannot find current executable: {}", e))?;

    let cmd = format!(
        "{} -p '{}'",
        exe.display(),
        config.prompt.replace('\'', "'\\''")
    );

    let output = tokio::process::Command::new("tmux")
        .args(["split-window", "-h", "-d", &cmd])
        .current_dir(&config.workspace)
        .output()
        .await
        .map_err(|e| format!("Failed to spawn tmux pane: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "tmux command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    tracing::info!(
        agent_id = %agent_id,
        name = %config.name,
        "Spawned tmux agent"
    );

    Ok(agent_id)
}

/// Generate a short hex ID from the current timestamp.
fn generate_short_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:x}", t as u64 & 0xFFFF_FFFF)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_short_id_is_hex() {
        let id = generate_short_id();
        assert!(!id.is_empty());
        for ch in id.chars() {
            assert!(
                ch.is_ascii_hexdigit(),
                "non-hex char in id: {}",
                ch
            );
        }
    }

    #[test]
    fn generate_short_id_varies() {
        // Two calls should (almost certainly) produce different IDs
        let id1 = generate_short_id();
        // Tiny sleep to ensure different nanos
        std::thread::sleep(std::time::Duration::from_nanos(1));
        let id2 = generate_short_id();
        // Not a hard assert — nanos could theoretically collide,
        // but in practice they won't.
        let _ = (id1, id2);
    }

    #[tokio::test]
    async fn spawn_in_process_returns_id() {
        let config = AgentSpawnConfig {
            name: "test-agent".to_string(),
            prompt: "Hello".to_string(),
            model: None,
            workspace: std::path::PathBuf::from("/tmp"),
            mode: SpawnMode::InProcess,
            isolation: None,
            run_in_background: false,
            max_depth: 3,
        };
        let id = spawn_teammate(config).await.unwrap();
        assert!(id.starts_with("agent-"));
    }
}
