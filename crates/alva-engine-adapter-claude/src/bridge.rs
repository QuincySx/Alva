use std::path::PathBuf;

use alva_engine_runtime::RuntimeError;

const BRIDGE_SCRIPT: &str = include_str!("../bridge/index.mjs");
const BRIDGE_DIR_NAME: &str = "alva-engine-claude-bridge";

/// Ensure the bridge script exists in a user-level cache directory.
///
/// The script content is embedded at compile time via `include_str!`.
/// Only rewrites if content differs, avoiding unnecessary I/O.
///
/// **Contains sync I/O** — callers should use `spawn_blocking` in async context.
pub(crate) fn ensure_bridge_script() -> Result<PathBuf, RuntimeError> {
    let base = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ensure_bridge_script_creates_file() {
        let path = ensure_bridge_script().unwrap();
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, BRIDGE_SCRIPT);
    }

    #[test]
    fn test_ensure_bridge_script_idempotent() {
        let path1 = ensure_bridge_script().unwrap();
        let path2 = ensure_bridge_script().unwrap();
        assert_eq!(path1, path2);
    }
}
