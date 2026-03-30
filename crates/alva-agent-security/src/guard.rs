// INPUT:  serde_json, std::path, alva_types, crate::{authorized_roots, permission, sandbox, sensitive_paths}
// OUTPUT: SecurityDecision, SecurityGuard
// POS:    Unified security gate composing sensitive-path filtering, authorized-root checking, and HITL permission management.
use std::collections::HashSet;
use serde_json::Value;
use std::path::{Path, PathBuf};

use alva_types::ToolContext;

use crate::authorized_roots::AuthorizedRoots;
use crate::permission::PermissionManager;
use crate::sandbox::{SandboxConfig, SandboxMode};
use crate::sensitive_paths::SensitivePathFilter;

/// The outcome of a security check before tool execution.
#[derive(Debug, Clone)]
pub enum SecurityDecision {
    /// Tool call is allowed to proceed.
    Allow,
    /// Tool call is blocked. `reason` explains why.
    Deny { reason: String },
    /// Tool call needs human approval. `request_id` is used to track the
    /// pending decision via `PermissionManager::resolve()`.
    NeedHumanApproval { request_id: String },
}

/// Default tools that require HITL review.
const DEFAULT_DANGEROUS_TOOLS: &[&str] = &[
    "execute_shell",
    "create_file",
    "file_edit",
    "browser_action",
    "browser_navigate",
];

/// Default JSON keys that contain file paths in tool arguments.
const DEFAULT_PATH_KEYS: &[&str] = &[
    "path",
    "file_path",
    "filepath",
    "directory",
    "dir",
    "target",
    "destination",
    "source",
    "src",
    "dest",
    "filename",
    "file",
    "folder",
    "working_directory",
    "cwd",
];

fn expand_home(path: &str) -> PathBuf {
    #[cfg(feature = "native")]
    {
        dirs::home_dir().unwrap_or_default().join(path)
    }
    #[cfg(not(feature = "native"))]
    {
        PathBuf::from("/home/agent").join(path)
    }
}

/// Unified security gate — the single entry point checked before every tool
/// execution.
///
/// Composes all security subsystems:
///   1. Sensitive path filtering
///   2. Authorized root checking
///   3. HITL permission management
///   4. Sandbox configuration (for command wrapping)
pub struct SecurityGuard {
    sensitive_paths: SensitivePathFilter,
    permission_manager: PermissionManager,
    authorized_roots: AuthorizedRoots,
    sandbox_config: SandboxConfig,
    /// Tools requiring HITL review — configurable at runtime.
    dangerous_tools: HashSet<String>,
    /// JSON keys to extract paths from — configurable at runtime.
    path_keys: HashSet<String>,
    /// Pending approval receivers keyed by request ID.
    pending_receivers: std::collections::HashMap<String, tokio::sync::oneshot::Receiver<crate::permission::PermissionDecision>>,
}

impl SecurityGuard {
    pub fn new(workspace: PathBuf, sandbox_mode: SandboxMode) -> Self {
        Self {
            sensitive_paths: SensitivePathFilter::default_rules(),
            permission_manager: PermissionManager::new(),
            authorized_roots: AuthorizedRoots::new(workspace.clone()),
            sandbox_config: SandboxConfig::for_workspace(&workspace, sandbox_mode),
            dangerous_tools: DEFAULT_DANGEROUS_TOOLS.iter().map(|s| s.to_string()).collect(),
            path_keys: DEFAULT_PATH_KEYS.iter().map(|s| s.to_string()).collect(),
            pending_receivers: std::collections::HashMap::new(),
        }
    }

    /// Add a tool name to the dangerous tools set (requires HITL review).
    pub fn add_dangerous_tool(&mut self, name: impl Into<String>) {
        self.dangerous_tools.insert(name.into());
    }

    /// Remove a tool from the dangerous tools set.
    pub fn remove_dangerous_tool(&mut self, name: &str) {
        self.dangerous_tools.remove(name);
    }

    /// Add a JSON key to the path extraction keys set.
    pub fn add_path_key(&mut self, key: impl Into<String>) {
        self.path_keys.insert(key.into());
    }

    /// Replace the sensitive path filter with a custom one.
    pub fn set_sensitive_paths(&mut self, filter: SensitivePathFilter) {
        self.sensitive_paths = filter;
    }

    /// Main security check — called before every tool execution.
    ///
    /// Checks in order:
    ///   1. Extract paths from tool args and check against sensitive path filter
    ///   2. Check extracted paths against authorized roots
    ///   3. Check HITL permission cache for dangerous tools
    ///   4. Return Allow / Deny / NeedHumanApproval
    pub fn check_tool_call(
        &mut self,
        tool_name: &str,
        args: &Value,
        _ctx: &dyn ToolContext,
    ) -> SecurityDecision {
        // 1. Extract all paths from tool arguments
        let paths = self.extract_paths(args);

        // 2. Check sensitive paths
        for path in &paths {
            if let Some(reason) = self.sensitive_paths.check(path) {
                return SecurityDecision::Deny {
                    reason: format!(
                        "tool '{}' blocked: {}",
                        tool_name, reason
                    ),
                };
            }
        }

        // 3. Check authorized roots
        for path in &paths {
            if let Err(reason) = self.authorized_roots.check(path) {
                return SecurityDecision::Deny {
                    reason: format!(
                        "tool '{}' blocked: {}",
                        tool_name, reason
                    ),
                };
            }
        }

        // 4. HITL check for dangerous tools
        if self.is_dangerous(tool_name) {
            match self.permission_manager.check(tool_name) {
                Some(true) => return SecurityDecision::Allow,
                Some(false) => {
                    return SecurityDecision::Deny {
                        reason: format!(
                            "tool '{}' is permanently denied for this session",
                            tool_name
                        ),
                    }
                }
                None => {
                    let request_id = uuid::Uuid::new_v4().to_string();
                    // Register pending approval — caller must await the receiver
                    let rx = self
                        .permission_manager
                        .request_approval(request_id.clone());
                    self.pending_receivers.insert(request_id.clone(), rx);
                    return SecurityDecision::NeedHumanApproval { request_id };
                }
            }
        }

        SecurityDecision::Allow
    }

    /// Resolve a pending HITL approval.
    pub fn resolve_permission(
        &mut self,
        request_id: &str,
        tool_name: &str,
        decision: crate::permission::PermissionDecision,
    ) -> bool {
        self.permission_manager
            .resolve(request_id, tool_name, decision)
    }

    /// Cancel a pending approval (e.g., on timeout).
    pub fn cancel_permission(&mut self, request_id: &str) {
        self.permission_manager.cancel(request_id);
    }

    /// Take the pending approval receiver for the given request.
    /// Returns `None` if no pending request exists (already taken or never created).
    pub fn take_pending_receiver(
        &mut self,
        request_id: &str,
    ) -> Option<tokio::sync::oneshot::Receiver<crate::permission::PermissionDecision>> {
        self.pending_receivers.remove(request_id)
    }

    /// Add an extra authorized root directory.
    pub fn add_authorized_root(&mut self, root: PathBuf) {
        self.authorized_roots.add_root(root);
    }

    /// Get the sandbox config (for wrapping shell commands).
    pub fn sandbox_config(&self) -> &SandboxConfig {
        &self.sandbox_config
    }

    /// Reset session-level permission caches.
    pub fn reset_permissions(&mut self) {
        self.permission_manager.reset();
    }

    // ---- internal helpers ----

    /// Check if a tool is considered dangerous.
    fn is_dangerous(&self, tool_name: &str) -> bool {
        self.dangerous_tools.contains(tool_name)
    }

    /// Extract file paths from JSON tool arguments by looking at well-known
    /// keys. Also handles the `command` key for shell tools (extracts paths
    /// from command strings).
    fn extract_paths(&self, args: &Value) -> Vec<PathBuf> {
        let mut paths = Vec::new();

        if let Value::Object(map) = args {
            for (key, value) in map {
                let key_lower = key.to_lowercase();

                // Direct path keys
                if self.path_keys.contains(&key_lower) {
                    if let Value::String(s) = value {
                        let p = Path::new(s);
                        if p.is_absolute() || s.starts_with("~/") || s.starts_with("./") || s.starts_with("../") {
                            let expanded = if let Some(rest) = s.strip_prefix("~/") {
                                expand_home(rest)
                            } else {
                                PathBuf::from(s)
                            };
                            paths.push(expanded);
                        }
                    }
                }

                // Shell command — try to extract paths from the command string
                if key_lower == "command" {
                    if let Value::String(cmd) = value {
                        paths.extend(Self::extract_paths_from_command(cmd));
                    }
                }
            }
        }

        paths
    }

    /// Best-effort extraction of file paths from a shell command string.
    fn extract_paths_from_command(command: &str) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        for token in command.split_whitespace() {
            // Remove common shell quoting
            let cleaned = token.trim_matches(|c| c == '\'' || c == '"');
            if cleaned.starts_with('/')
                || cleaned.starts_with("~/")
                || cleaned.starts_with("./")
                || cleaned.starts_with("../")
            {
                let expanded = if let Some(rest) = cleaned.strip_prefix("~/") {
                    expand_home(rest)
                } else {
                    PathBuf::from(cleaned)
                };
                paths.push(expanded);
            }
        }
        paths
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    struct TestToolContext {
        workspace: PathBuf,
    }

    impl alva_types::ToolContext for TestToolContext {
        fn session_id(&self) -> &str { "test-session" }
        fn get_config(&self, _key: &str) -> Option<String> { None }
        fn as_any(&self) -> &dyn std::any::Any { self }
        fn local(&self) -> Option<&dyn alva_types::LocalToolContext> { Some(self) }
    }

    impl alva_types::LocalToolContext for TestToolContext {
        fn workspace(&self) -> &std::path::Path { &self.workspace }
        fn allow_dangerous(&self) -> bool { false }
    }

    fn test_ctx() -> TestToolContext {
        TestToolContext {
            workspace: PathBuf::from("/projects/myapp"),
        }
    }

    #[test]
    fn allows_safe_tool_in_workspace() {
        let mut guard = SecurityGuard::new(
            PathBuf::from("/projects/myapp"),
            SandboxMode::RestrictiveOpen,
        );
        let args = json!({ "path": "/projects/myapp/src/main.rs" });
        let decision = guard.check_tool_call("read_file", &args, &test_ctx());
        assert!(matches!(decision, SecurityDecision::Allow));
    }

    #[test]
    fn denies_sensitive_path() {
        let mut guard = SecurityGuard::new(
            PathBuf::from("/projects/myapp"),
            SandboxMode::RestrictiveOpen,
        );
        let args = json!({ "path": "/projects/myapp/.env" });
        let decision = guard.check_tool_call("read_file", &args, &test_ctx());
        assert!(matches!(decision, SecurityDecision::Deny { .. }));
    }

    #[test]
    fn denies_outside_workspace() {
        let mut guard = SecurityGuard::new(
            PathBuf::from("/projects/myapp"),
            SandboxMode::RestrictiveOpen,
        );
        let args = json!({ "path": "/etc/passwd" });
        let decision = guard.check_tool_call("read_file", &args, &test_ctx());
        assert!(matches!(decision, SecurityDecision::Deny { .. }));
    }

    #[test]
    fn dangerous_tool_needs_approval() {
        let mut guard = SecurityGuard::new(
            PathBuf::from("/projects/myapp"),
            SandboxMode::RestrictiveOpen,
        );
        let args = json!({ "command": "ls /projects/myapp" });
        let decision = guard.check_tool_call("execute_shell", &args, &test_ctx());
        assert!(matches!(decision, SecurityDecision::NeedHumanApproval { .. }));
    }

    #[test]
    fn dangerous_tool_after_always_allow() {
        let mut guard = SecurityGuard::new(
            PathBuf::from("/projects/myapp"),
            SandboxMode::RestrictiveOpen,
        );
        // First call — needs approval
        let args = json!({ "command": "ls /projects/myapp" });
        let decision = guard.check_tool_call("execute_shell", &args, &test_ctx());
        if let SecurityDecision::NeedHumanApproval { request_id } = decision {
            guard.resolve_permission(
                &request_id,
                "execute_shell",
                crate::permission::PermissionDecision::AllowAlways,
            );
        }
        // Second call — should be auto-allowed
        let decision2 = guard.check_tool_call("execute_shell", &args, &test_ctx());
        assert!(matches!(decision2, SecurityDecision::Allow));
    }

    #[test]
    fn extracts_paths_from_args() {
        let args = json!({
            "path": "/projects/myapp/file.txt",
            "destination": "~/Documents/output.txt"
        });
        let guard = SecurityGuard::new(PathBuf::from("/tmp"), SandboxMode::RestrictiveOpen);
        let paths = guard.extract_paths(&args);
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn extracts_paths_from_command() {
        let paths = SecurityGuard::extract_paths_from_command("cat /etc/passwd ~/.bashrc");
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn non_dangerous_tool_auto_allowed() {
        let mut guard = SecurityGuard::new(
            PathBuf::from("/projects/myapp"),
            SandboxMode::RestrictiveOpen,
        );
        let args = json!({ "path": "/projects/myapp/src/lib.rs" });
        let decision = guard.check_tool_call("read_file", &args, &test_ctx());
        assert!(matches!(decision, SecurityDecision::Allow));
    }

    #[tokio::test]
    async fn approval_receiver_can_be_taken_after_check() {
        let mut guard = SecurityGuard::new(
            PathBuf::from("/projects/myapp"),
            SandboxMode::RestrictiveOpen,
        );
        let args = json!({ "command": "ls /projects/myapp" });
        let decision = guard.check_tool_call("execute_shell", &args, &test_ctx());

        let request_id = match decision {
            SecurityDecision::NeedHumanApproval { request_id } => request_id,
            other => panic!("expected NeedHumanApproval, got {:?}", other),
        };

        // Should be able to take the receiver
        let rx = guard.take_pending_receiver(&request_id);
        assert!(rx.is_some(), "receiver should exist after check_tool_call");

        // Resolve and verify receiver gets the decision
        guard.resolve_permission(&request_id, "execute_shell", crate::permission::PermissionDecision::AllowOnce);
        let decision = rx.unwrap().await.unwrap();
        assert_eq!(decision, crate::permission::PermissionDecision::AllowOnce);
    }
}
