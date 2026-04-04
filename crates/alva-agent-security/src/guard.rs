// INPUT:  serde_json, std::path, alva_types, crate::{authorized_roots, cache, classifier, modes, permission, rules, sandbox, sensitive_paths}
// OUTPUT: SecurityDecision, SecurityGuard
// POS:    Unified security gate composing sensitive-path filtering, authorized-root checking,
//         HITL permission management, permission rules, caching, modes, and bash classification.
use std::collections::HashSet;
use serde_json::Value;
use std::path::{Path, PathBuf};

use alva_types::ToolExecutionContext;

use crate::authorized_roots::AuthorizedRoots;
use crate::cache::{CachedDecision, PermissionCache};
use crate::classifier::{BashClassifier, CommandClassification};
use crate::modes::PermissionMode;
use crate::path_utils::normalize_path;
use crate::permission::PermissionManager;
use crate::rules::{PermissionRules, RuleDecision};
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
    "notebook_path",
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
///   3. Permission rules (pattern-based allow/deny/ask)
///   4. Permission cache (memoize repeated decisions)
///   5. Permission mode (interactive/auto/plan/bypass)
///   6. Bash command classification (read-only/destructive/unknown)
///   7. HITL permission management
///   8. Sandbox configuration (for command wrapping)
pub struct SecurityGuard {
    sensitive_paths: SensitivePathFilter,
    permission_manager: PermissionManager,
    authorized_roots: AuthorizedRoots,
    sandbox_config: SandboxConfig,
    /// Permission rules for pattern-based decisions.
    permission_rules: PermissionRules,
    /// Cache for repeated permission decisions.
    permission_cache: PermissionCache,
    /// Current permission mode.
    permission_mode: PermissionMode,
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
            permission_rules: PermissionRules::default(),
            permission_cache: PermissionCache::new(),
            permission_mode: PermissionMode::default(),
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

    /// Set permission rules for pattern-based decisions.
    pub fn set_permission_rules(&mut self, rules: PermissionRules) {
        self.permission_rules = rules;
    }

    /// Get a reference to the current permission rules.
    pub fn permission_rules(&self) -> &PermissionRules {
        &self.permission_rules
    }

    /// Set the permission mode.
    pub fn set_permission_mode(&mut self, mode: PermissionMode) {
        self.permission_mode = mode;
    }

    /// Get the current permission mode.
    pub fn permission_mode(&self) -> PermissionMode {
        self.permission_mode
    }

    /// Get a reference to the permission cache.
    pub fn permission_cache(&self) -> &PermissionCache {
        &self.permission_cache
    }

    /// Classify a bash command for safety.
    pub fn classify_command(&self, command: &str) -> CommandClassification {
        BashClassifier::classify(command)
    }

    /// Main security check — called before every tool execution.
    ///
    /// Checks in order:
    ///   1. Permission mode enforcement (plan mode blocks writes)
    ///   2. Extract paths from tool args and check against sensitive path filter
    ///   3. Check extracted paths against authorized roots
    ///   4. Permission cache lookup
    ///   5. Permission rules check (pattern-based allow/deny/ask)
    ///   6. Bash command classification (in auto mode)
    ///   7. HITL permission manager for dangerous tools
    ///   8. Return Allow / Deny / NeedHumanApproval
    pub fn check_tool_call(
        &mut self,
        tool_name: &str,
        args: &Value,
        _ctx: &dyn ToolExecutionContext,
    ) -> SecurityDecision {
        // 1. Permission mode enforcement
        if !self.permission_mode.allows_writes() && self.is_dangerous(tool_name) {
            tracing::info!(tool = tool_name, mode = %self.permission_mode, "denied by plan mode");
            return SecurityDecision::Deny {
                reason: format!(
                    "tool '{}' blocked: write operations not allowed in {} mode",
                    tool_name, self.permission_mode
                ),
            };
        }

        // Bypass mode: allow everything (assumes sandbox is active)
        if self.permission_mode == PermissionMode::Bypass {
            tracing::debug!(tool = tool_name, "allowed by bypass mode");
            return SecurityDecision::Allow;
        }

        // 2. Extract all paths from tool arguments
        let paths = self.extract_paths(args);

        // 3. Check sensitive paths
        for path in &paths {
            if let Some(reason) = self.sensitive_paths.check(path) {
                tracing::info!(tool = tool_name, path = %path.display(), "denied: sensitive path");
                return SecurityDecision::Deny {
                    reason: format!(
                        "tool '{}' blocked: {}",
                        tool_name, reason
                    ),
                };
            }
        }

        // 4. Check authorized roots
        for path in &paths {
            if let Err(reason) = self.authorized_roots.check(path) {
                tracing::info!(tool = tool_name, path = %path.display(), "denied: outside roots");
                return SecurityDecision::Deny {
                    reason: format!(
                        "tool '{}' blocked: {}",
                        tool_name, reason
                    ),
                };
            }
        }

        // 5. Permission cache lookup
        if let Some(cached) = self.permission_cache.get(tool_name, args) {
            match cached {
                CachedDecision::AllowAlways => {
                    tracing::debug!(tool = tool_name, "allowed by cache");
                    return SecurityDecision::Allow;
                }
                CachedDecision::DenyAlways => {
                    tracing::debug!(tool = tool_name, "denied by cache");
                    return SecurityDecision::Deny {
                        reason: format!(
                            "tool '{}' is cached as denied",
                            tool_name
                        ),
                    };
                }
            }
        }

        // 6. Permission rules check (pattern-based)
        if !self.permission_rules.is_empty() {
            let input_summary = Self::summarize_input(tool_name, args);
            match self.permission_rules.check(tool_name, &input_summary) {
                RuleDecision::Allow => {
                    tracing::debug!(tool = tool_name, "allowed by rule");
                    return SecurityDecision::Allow;
                }
                RuleDecision::Deny => {
                    tracing::info!(tool = tool_name, "denied by rule");
                    return SecurityDecision::Deny {
                        reason: format!(
                            "tool '{}' blocked by permission rule",
                            tool_name
                        ),
                    };
                }
                RuleDecision::Ask => {
                    // Fall through to HITL check below
                }
            }
        }

        // 7. Auto mode: use bash classifier for auto-approval
        if self.permission_mode.auto_approves() && self.is_dangerous(tool_name) {
            if let Some(command) = Self::extract_command(args) {
                match BashClassifier::classify(&command) {
                    CommandClassification::ReadOnly => {
                        tracing::debug!(tool = tool_name, "auto-approved: read-only command");
                        return SecurityDecision::Allow;
                    }
                    CommandClassification::Destructive => {
                        tracing::info!(tool = tool_name, "denied: destructive command in auto mode");
                        return SecurityDecision::Deny {
                            reason: format!(
                                "tool '{}' blocked: destructive command '{}' not allowed in auto mode",
                                tool_name, command
                            ),
                        };
                    }
                    CommandClassification::Unknown => {
                        // Auto mode auto-approves unknown commands too (trusts sandbox)
                        tracing::debug!(tool = tool_name, "auto-approved: unknown command in auto mode");
                        return SecurityDecision::Allow;
                    }
                }
            }
        }

        // 8. HITL check for dangerous tools
        if self.is_dangerous(tool_name) {
            match self.permission_manager.check(tool_name, args) {
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
                        .request_approval(request_id.clone(), tool_name, args);
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

    /// Reset session-level permission caches (both HITL manager and permission cache).
    pub fn reset_permissions(&mut self) {
        self.permission_manager.reset();
        self.permission_cache.clear();
    }

    // ---- internal helpers ----

    /// Check if a tool is considered dangerous.
    fn is_dangerous(&self, tool_name: &str) -> bool {
        self.dangerous_tools.contains(tool_name)
    }

    /// Build a summary string from tool input for rule matching.
    fn summarize_input(tool_name: &str, args: &Value) -> String {
        // For Bash-like tools, use the command string
        if let Some(cmd) = Self::extract_command(args) {
            return cmd;
        }
        // For file tools, use the path
        if let Some(path) = args.get("path").or(args.get("file_path")).and_then(|v| v.as_str()) {
            return path.to_string();
        }
        // Fallback: serialize the args
        let _ = tool_name; // used in format context by callers
        serde_json::to_string(args).unwrap_or_default()
    }

    /// Extract command string from bash-like tool arguments.
    fn extract_command(args: &Value) -> Option<String> {
        args.get("command")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
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
                        if !s.is_empty() {
                            paths.push(self.resolve_path_argument(s));
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

    fn resolve_path_argument(&self, raw: &str) -> PathBuf {
        if let Some(rest) = raw.strip_prefix("~/") {
            expand_home(rest)
        } else if Path::new(raw).is_absolute() {
            PathBuf::from(raw)
        } else {
            normalize_path(&self.authorized_roots.workspace().join(raw))
        }
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
        cancel: alva_types::CancellationToken,
    }

    impl alva_types::ToolExecutionContext for TestToolContext {
        fn cancel_token(&self) -> &alva_types::CancellationToken { &self.cancel }
        fn session_id(&self) -> &str { "test-session" }
        fn workspace(&self) -> Option<&std::path::Path> { Some(&self.workspace) }
        fn as_any(&self) -> &dyn std::any::Any { self }
    }

    fn test_ctx() -> TestToolContext {
        TestToolContext {
            workspace: PathBuf::from("/projects/myapp"),
            cancel: alva_types::CancellationToken::new(),
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
    fn relative_sensitive_path_is_denied() {
        let mut guard = SecurityGuard::new(
            PathBuf::from("/projects/myapp"),
            SandboxMode::RestrictiveOpen,
        );
        let args = json!({ "path": ".env" });
        let decision = guard.check_tool_call("read_file", &args, &test_ctx());
        assert!(matches!(decision, SecurityDecision::Deny { .. }));
    }

    #[test]
    fn always_allow_does_not_cover_different_arguments() {
        let mut guard = SecurityGuard::new(
            PathBuf::from("/projects/myapp"),
            SandboxMode::RestrictiveOpen,
        );
        let approved_args = json!({ "command": "ls /projects/myapp" });
        let approved = guard.check_tool_call("execute_shell", &approved_args, &test_ctx());
        if let SecurityDecision::NeedHumanApproval { request_id } = approved {
            guard.resolve_permission(
                &request_id,
                "execute_shell",
                crate::permission::PermissionDecision::AllowAlways,
            );
        } else {
            panic!("expected approval request");
        }

        let different_args = json!({ "command": "ls /projects/myapp/src" });
        let decision = guard.check_tool_call("execute_shell", &different_args, &test_ctx());
        assert!(
            matches!(decision, SecurityDecision::NeedHumanApproval { .. }),
            "different arguments should require a fresh approval"
        );
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
