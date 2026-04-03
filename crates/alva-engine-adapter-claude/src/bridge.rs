// INPUT:  std::path::PathBuf, dirs, alva_engine_runtime::RuntimeError
// OUTPUT: pub(crate) fn ensure_bridge_script
// POS:    Embeds and deploys the Node.js bridge script to a user-level cache directory.

use std::path::PathBuf;

use alva_engine_runtime::RuntimeError;

const BRIDGE_SCRIPT: &str = include_str!("../bridge/index.mjs");
const BRIDGE_DIR_NAME: &str = "alva-engine-claude-bridge";
const CACHE_DIR_OVERRIDE_ENV: &str = "ALVA_ENGINE_CLAUDE_BRIDGE_CACHE_DIR";

/// Ensure the bridge script exists in a user-level cache directory.
///
/// The script content is embedded at compile time via `include_str!`.
/// Only rewrites if content differs, avoiding unnecessary I/O.
///
/// **Contains sync I/O** — callers should use `spawn_blocking` in async context.
pub(crate) fn ensure_bridge_script() -> Result<PathBuf, RuntimeError> {
    let base = bridge_base_dir()
        .join(BRIDGE_DIR_NAME);

    std::fs::create_dir_all(&base).map_err(|e| {
        RuntimeError::ProcessError(format!("Failed to create bridge directory: {e}"))
    })?;

    let script_path = base.join("index.mjs");
    let needs_write = match std::fs::read_to_string(&script_path) {
        Ok(existing) => existing != BRIDGE_SCRIPT,
        Err(_) => true,
    };

    if needs_write {
        std::fs::write(&script_path, BRIDGE_SCRIPT).map_err(|e| {
            RuntimeError::ProcessError(format!("Failed to write bridge script: {e}"))
        })?;
    }

    Ok(script_path)
}

fn bridge_base_dir() -> PathBuf {
    std::env::var_os(CACHE_DIR_OVERRIDE_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::cache_dir().unwrap_or_else(std::env::temp_dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn test_ensure_bridge_script_creates_file() {
        let _guard = env_lock().lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var(CACHE_DIR_OVERRIDE_ENV, temp.path());
        let path = ensure_bridge_script().unwrap();
        std::env::remove_var(CACHE_DIR_OVERRIDE_ENV);

        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, BRIDGE_SCRIPT);
    }

    #[test]
    fn test_ensure_bridge_script_idempotent() {
        let _guard = env_lock().lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var(CACHE_DIR_OVERRIDE_ENV, temp.path());
        let path1 = ensure_bridge_script().unwrap();
        let path2 = ensure_bridge_script().unwrap();
        std::env::remove_var(CACHE_DIR_OVERRIDE_ENV);

        assert_eq!(path1, path2);
    }
}
