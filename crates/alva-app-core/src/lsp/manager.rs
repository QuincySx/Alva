//! LSP server lifecycle management.
//!
//! [`LspServerManager`] keeps track of running LSP server instances, allowing
//! the application to register, query, update, and remove servers by language
//! identifier.

use std::collections::HashMap;

/// Status of an LSP server instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LspServerStatus {
    /// The server process is starting up.
    Starting,
    /// The server is running and has completed initialisation.
    Running,
    /// The server has stopped (gracefully or crashed).
    Stopped,
    /// An error occurred (e.g. failed to spawn).
    Error,
}

/// Metadata for a single LSP server instance.
#[derive(Debug, Clone)]
pub struct LspServerInfo {
    /// Language identifier (e.g. `"rust"`, `"typescript"`).
    pub language_id: String,
    /// Human-readable server name (e.g. `"rust-analyzer"`).
    pub server_name: String,
    /// Command used to start the server.
    pub command: String,
    /// Current status.
    pub status: LspServerStatus,
    /// Optional PID of the child process.
    pub pid: Option<u32>,
}

/// Manages multiple LSP server instances keyed by language identifier.
#[derive(Debug, Default)]
pub struct LspServerManager {
    servers: HashMap<String, LspServerInfo>,
}

impl LspServerManager {
    /// Create an empty manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new server. Replaces any existing entry for the same
    /// language.
    pub fn register(&mut self, info: LspServerInfo) {
        self.servers.insert(info.language_id.clone(), info);
    }

    /// Get information about a server by language id.
    pub fn get(&self, language_id: &str) -> Option<&LspServerInfo> {
        self.servers.get(language_id)
    }

    /// List all registered servers.
    pub fn list(&self) -> Vec<&LspServerInfo> {
        self.servers.values().collect()
    }

    /// Update the status of an existing server. Returns `true` if the server
    /// was found and updated.
    pub fn update_status(&mut self, language_id: &str, status: LspServerStatus) -> bool {
        if let Some(info) = self.servers.get_mut(language_id) {
            info.status = status;
            true
        } else {
            false
        }
    }

    /// Remove a server by language id. Returns the removed info if present.
    pub fn remove(&mut self, language_id: &str) -> Option<LspServerInfo> {
        self.servers.remove(language_id)
    }

    /// Number of registered servers.
    pub fn len(&self) -> usize {
        self.servers.len()
    }

    /// Whether any servers are registered.
    pub fn is_empty(&self) -> bool {
        self.servers.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_info(lang: &str) -> LspServerInfo {
        LspServerInfo {
            language_id: lang.to_string(),
            server_name: format!("{}-server", lang),
            command: format!("{}-lsp", lang),
            status: LspServerStatus::Starting,
            pid: None,
        }
    }

    #[test]
    fn register_and_get() {
        let mut mgr = LspServerManager::new();
        mgr.register(sample_info("rust"));
        assert!(mgr.get("rust").is_some());
        assert_eq!(mgr.get("rust").unwrap().server_name, "rust-server");
    }

    #[test]
    fn list_returns_all() {
        let mut mgr = LspServerManager::new();
        mgr.register(sample_info("rust"));
        mgr.register(sample_info("python"));
        assert_eq!(mgr.list().len(), 2);
    }

    #[test]
    fn update_status() {
        let mut mgr = LspServerManager::new();
        mgr.register(sample_info("rust"));
        assert!(mgr.update_status("rust", LspServerStatus::Running));
        assert_eq!(mgr.get("rust").unwrap().status, LspServerStatus::Running);
        assert!(!mgr.update_status("go", LspServerStatus::Error));
    }

    #[test]
    fn remove_returns_info() {
        let mut mgr = LspServerManager::new();
        mgr.register(sample_info("rust"));
        let removed = mgr.remove("rust");
        assert!(removed.is_some());
        assert!(mgr.is_empty());
    }
}
