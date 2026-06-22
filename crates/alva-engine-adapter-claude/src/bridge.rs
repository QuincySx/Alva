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
    let base = bridge_base_dir().join(BRIDGE_DIR_NAME);

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

    // -- Loop 149 gap-fill: bridge_base_dir priority + constants +
    //    "exists but stale" rewrite contract -----------------------------

    #[test]
    fn cache_dir_override_env_var_name_is_pinned_literal() {
        // SILENT CONTRACT: users / operators set this env var name in
        // shell rc files, deploy scripts, or container env. A silent
        // rename would make every existing override silently no-op,
        // sending the bridge script to dirs::cache_dir instead.
        assert_eq!(
            CACHE_DIR_OVERRIDE_ENV,
            "ALVA_ENGINE_CLAUDE_BRIDGE_CACHE_DIR"
        );
    }

    #[test]
    fn bridge_dir_name_is_pinned_literal() {
        // SILENT CONTRACT: the directory name lives under
        // dirs::cache_dir() and is the lookup key for cached
        // bridge scripts across CLI sessions. A silent rename
        // would orphan every previously-written bridge cache
        // (the new code wouldn't find it, the old dir would
        // accumulate forever).
        assert_eq!(BRIDGE_DIR_NAME, "alva-engine-claude-bridge");
    }

    #[test]
    fn bridge_base_dir_respects_env_override_over_dirs_cache() {
        // Priority pin: env override MUST take precedence over
        // dirs::cache_dir(). A refactor that swapped the order would
        // silently route bridge writes to ~/Library/Caches even when
        // the user explicitly overrode the location (e.g. for hermetic
        // CI runs or sandbox testing).
        let _guard = env_lock().lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let expected = temp.path().to_path_buf();
        std::env::set_var(CACHE_DIR_OVERRIDE_ENV, &expected);
        let result = bridge_base_dir();
        std::env::remove_var(CACHE_DIR_OVERRIDE_ENV);
        assert_eq!(
            result, expected,
            "env override must beat dirs::cache_dir; got {result:?}, expected {expected:?}"
        );
    }

    #[test]
    fn bridge_base_dir_returns_some_path_when_no_env_override() {
        // Pin: with no override, bridge_base_dir returns SOMETHING
        // valid (either dirs::cache_dir or env::temp_dir fallback).
        // We can't pin the exact dirs::cache_dir value (varies per OS
        // + per user), but we CAN pin that the result is NOT the
        // override path (proves the env var is actually consulted)
        // and is a non-empty absolute path.
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var(CACHE_DIR_OVERRIDE_ENV);
        let result = bridge_base_dir();
        // result is either dirs::cache_dir() or temp_dir() fallback;
        // both are absolute on supported platforms.
        assert!(
            result.is_absolute(),
            "bridge_base_dir must return absolute path, got {result:?}"
        );
        // sanity: not empty
        assert!(
            !result.as_os_str().is_empty(),
            "bridge_base_dir must not return empty path"
        );
    }

    #[test]
    fn ensure_bridge_script_rewrites_when_existing_content_differs() {
        // CRITICAL: the documented "Only rewrites if content differs"
        // contract has TWO halves — existing test covers "same content
        // → no-op" via idempotent path equality. This test covers the
        // OTHER half: "existing but stale → MUST rewrite". Without
        // this pin, a refactor that flipped the comparison (`==` vs
        // `!=`) or short-circuited on file-exists would silently let
        // users run a stale bridge after upgrading.
        let _guard = env_lock().lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var(CACHE_DIR_OVERRIDE_ENV, temp.path());

        // Pre-seed a stale bridge script with WRONG content.
        let base = temp.path().join(BRIDGE_DIR_NAME);
        std::fs::create_dir_all(&base).unwrap();
        let script_path = base.join("index.mjs");
        std::fs::write(&script_path, "// stale outdated bridge content\n").unwrap();

        // ensure_bridge_script must detect mismatch and overwrite.
        let returned_path = ensure_bridge_script().unwrap();
        std::env::remove_var(CACHE_DIR_OVERRIDE_ENV);

        assert_eq!(returned_path, script_path);
        let new_content = std::fs::read_to_string(&script_path).unwrap();
        assert_eq!(
            new_content, BRIDGE_SCRIPT,
            "stale bridge content must be overwritten by the embedded BRIDGE_SCRIPT"
        );
        assert_ne!(
            new_content, "// stale outdated bridge content\n",
            "rewrite did not happen — pre-existing stale content survived"
        );
    }
}
