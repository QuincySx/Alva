// INPUT:  serde_json, std::path, alva_kernel_abi, crate::{authorized_roots, cache, classifier, modes, permission, rules, sandbox, sensitive_paths}
// OUTPUT: SecurityDecision, SecurityGuard
// POS:    Unified security gate composing sensitive-path filtering, authorized-root checking,
//         HITL permission management, permission rules, caching, modes, and bash classification.
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use alva_kernel_abi::{bus_cap, ToolExecutionContext};

use crate::authorized_roots::AuthorizedRoots;
use crate::cache::{CachedDecision, PermissionCache};
use crate::classifier::{BashClassifier, CommandClassification};
use crate::mode_control::{SecurityModeControl, SecurityModeHandle};
use crate::modes::PermissionMode;
use crate::path_utils::normalize_path;
use crate::permission::PermissionManager;
use crate::rules::{PermissionRules, RuleDecision};
use crate::sandbox::{SandboxConfig, SandboxMode};
use crate::sensitive_paths::SensitivePathFilter;
use crate::url_info::{inspect_url, UrlInfo, UrlRisk, UrlRules};

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

/// Default mapping of tools whose arguments contain a URL that needs SSRF
/// inspection, to the JSON arg key holding that URL.
/// Added to during construction; user can extend via `add_url_aware_tool`.
const DEFAULT_URL_AWARE_TOOLS: &[(&str, &str)] = &[("read_url", "url")];

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

/// Bus Capability: unified security gate shared by middleware + HITL UI.
/// Published on the bus wrapped in `tokio::sync::Mutex<SecurityGuard>`.
///
/// **Provider**: `SecurityPlugin::register`
/// (`alva-agent-extension-builtin/src/wrappers/security.rs`). The
/// default-replacement contract applies — register your own plugin
/// named `"security"` to swap the guard.
/// **Consumers**: `BaseAgent::resolve_permission`
/// (`alva-app-core/src/base_agent/agent.rs`) so the CLI / UI can
/// answer pending HITL approval prompts without going through a
/// hardcoded accessor; internally `SecurityMiddleware` retains its own
/// `Arc<Mutex<_>>` clone.
/// **Why bus**: the middleware enforcing security sits in
/// `alva-agent-security`; the HITL resolver lives in the outer app.
/// Exposing the guard on the bus avoids bolting a `Security`-shaped
/// accessor onto the stable `BaseAgent` API.
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
#[bus_cap]
pub struct SecurityGuard {
    sensitive_paths: SensitivePathFilter,
    permission_manager: PermissionManager,
    authorized_roots: AuthorizedRoots,
    sandbox_config: SandboxConfig,
    /// Permission rules for pattern-based decisions.
    permission_rules: PermissionRules,
    /// Cache for repeated permission decisions.
    permission_cache: PermissionCache,
    /// Current permission mode — atomic-backed `Arc` so that any holder of
    /// the same handle (e.g. a `SecurityModeControl` published on the bus)
    /// observes mode changes without going through the guard's tokio Mutex.
    mode: Arc<SecurityModeHandle>,
    /// Tools requiring HITL review — configurable at runtime.
    dangerous_tools: HashSet<String>,
    /// JSON keys to extract paths from — configurable at runtime.
    path_keys: HashSet<String>,
    /// Pending approval receivers keyed by request ID.
    pending_receivers: std::collections::HashMap<
        String,
        tokio::sync::oneshot::Receiver<crate::permission::PermissionDecision>,
    >,
    /// URL fetch policy (SSRF defense / T6 3C path).
    /// Single knob: `ask_threshold` — default Medium means private/loopback/
    /// link-local/DNS-fail trigger HITL approval; public URLs auto-pass.
    /// See `crate::url_info` for the IP classification map.
    url_rules: UrlRules,
    /// Tools whose arguments contain a URL needing SSRF inspection.
    /// Maps tool_name → arg_name (JSON key with the URL string).
    /// Consulted by SecurityMiddleware in `before_tool_call`.
    url_aware_tools: std::collections::HashMap<String, String>,
}

impl SecurityGuard {
    pub fn new(workspace: PathBuf, sandbox_mode: SandboxMode) -> Self {
        Self::with_mode_handle(
            workspace,
            sandbox_mode,
            Arc::new(SecurityModeHandle::default()),
        )
    }

    /// Construct with an externally-owned mode handle so a `SecurityModeControl`
    /// published on the bus shares the exact same `Arc<AtomicU8>` the guard
    /// reads through.
    pub fn with_mode_handle(
        workspace: PathBuf,
        sandbox_mode: SandboxMode,
        mode: Arc<SecurityModeHandle>,
    ) -> Self {
        Self {
            sensitive_paths: SensitivePathFilter::default_rules(),
            permission_manager: PermissionManager::new(),
            authorized_roots: AuthorizedRoots::new(workspace.clone()),
            sandbox_config: SandboxConfig::for_workspace(&workspace, sandbox_mode),
            permission_rules: PermissionRules::default(),
            permission_cache: PermissionCache::new(),
            mode,
            dangerous_tools: DEFAULT_DANGEROUS_TOOLS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            path_keys: DEFAULT_PATH_KEYS.iter().map(|s| s.to_string()).collect(),
            pending_receivers: std::collections::HashMap::new(),
            url_rules: UrlRules::default(),
            url_aware_tools: DEFAULT_URL_AWARE_TOOLS
                .iter()
                .map(|(t, a)| (t.to_string(), a.to_string()))
                .collect(),
        }
    }

    /// Shared reference to the mode handle. Publish this on the bus as
    /// `dyn SecurityModeControl` to let outer crates flip the mode without
    /// holding the tokio Mutex over the guard.
    pub fn mode_handle(&self) -> Arc<SecurityModeHandle> {
        self.mode.clone()
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

    /// Set URL rules (currently just the HITL ask-threshold).
    pub fn set_url_rules(&mut self, rules: UrlRules) {
        self.url_rules = rules;
    }

    /// Get a reference to the current URL rules.
    pub fn url_rules(&self) -> &UrlRules {
        &self.url_rules
    }

    /// Inspect a URL: parse + DNS-resolve + classify, no fetch.
    /// Pure delegate to `crate::url_info::inspect_url` — see that
    /// function for the failure semantics (parse-fail / DNS-fail both
    /// produce `UrlRisk::High` with `ip_class = None`).
    pub async fn inspect_url(&self, url: &str) -> UrlInfo {
        inspect_url(url).await
    }

    /// Compare a risk level against `url_rules.ask_threshold`.
    /// Returns true if the caller (e.g. `read_url`) must request HITL
    /// approval before proceeding.
    pub fn should_ask_for_url(&self, risk: UrlRisk) -> bool {
        self.url_rules.should_ask(risk)
    }

    /// Add or replace a URL-aware tool mapping (e.g. `("read_url", "url")`).
    pub fn add_url_aware_tool(&mut self, tool: impl Into<String>, arg: impl Into<String>) {
        self.url_aware_tools.insert(tool.into(), arg.into());
    }

    /// Replace the entire URL-aware tools map.
    pub fn set_url_aware_tools(&mut self, m: std::collections::HashMap<String, String>) {
        self.url_aware_tools = m;
    }

    /// SSRF check for tool calls whose args carry a URL.
    /// Called by SecurityMiddleware AFTER the main `check_tool_call`
    /// returned Allow — chains a second decision based on URL risk.
    ///
    /// Returns:
    /// - `Some(NeedHumanApproval { request_id })` — risk ≥ threshold; a
    ///   pending approval has been registered and the middleware should
    ///   run the same HITL flow it uses for `dangerous_tools`.
    /// - `Some(Allow)` — URL inspected and below threshold, explicit OK.
    /// - `None` — tool is not URL-aware (no mapping exists) or its args
    ///   don't carry the expected key, so the middleware should proceed.
    pub async fn check_url_in_tool_call(
        &mut self,
        tool_name: &str,
        args: &Value,
    ) -> Option<SecurityDecision> {
        let arg_name = self.url_aware_tools.get(tool_name)?;
        let url = args.get(arg_name).and_then(|v| v.as_str())?;
        let info = inspect_url(url).await;
        if !self.url_rules.should_ask(info.risk) {
            return Some(SecurityDecision::Allow);
        }
        // Risk above threshold — register pending approval. The
        // middleware will see `NeedHumanApproval` and reuse its existing
        // notifier-and-receiver dance.
        let request_id = format!("url-{}", uuid::Uuid::new_v4());
        let rx = self
            .permission_manager
            .request_approval(request_id.clone(), tool_name, args);
        self.pending_receivers.insert(request_id.clone(), rx);
        Some(SecurityDecision::NeedHumanApproval { request_id })
    }

    /// Set the permission mode.
    pub fn set_permission_mode(&self, mode: PermissionMode) {
        self.mode.set_mode(mode);
    }

    /// Get the current permission mode.
    pub fn permission_mode(&self) -> PermissionMode {
        self.mode.get_mode()
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
        let current_mode = self.mode.get_mode();

        // 1. Permission mode enforcement
        if !current_mode.allows_writes() && self.is_dangerous(tool_name) {
            tracing::info!(tool = tool_name, mode = %current_mode, "denied by plan mode");
            return SecurityDecision::Deny {
                reason: format!(
                    "tool '{}' blocked: write operations not allowed in {} mode",
                    tool_name, current_mode
                ),
            };
        }

        // Bypass mode: allow everything (assumes sandbox is active)
        if current_mode == PermissionMode::Bypass {
            tracing::debug!(tool = tool_name, "allowed by bypass mode");
            return SecurityDecision::Allow;
        }

        // 2. Extract paths from tool arguments, split by source.
        //
        // explicit_paths come from structured args (`file_path`, `path`, etc.)
        // and are authoritative — tool clearly intends to touch THAT path.
        //
        // command_paths come from tokenizing a shell command string. They're
        // a best-effort heuristic — a shell command might reference `/` or
        // `/tmp` as part of normal operation without meaning "access this
        // file". Root-checking these makes shell unusable (any `ls /` gets
        // denied). The sensitive-path filter still applies so `/etc/passwd`
        // or secret stores stay blocked; shell safety beyond that belongs
        // to `BashClassifier`, not the root boundary.
        let explicit_paths = self.extract_explicit_paths(args);
        let command_paths = Self::extract_paths_from_args_commands(args);

        // 3. Sensitive-path check applies to BOTH (explicit + shell-extracted).
        for path in explicit_paths.iter().chain(command_paths.iter()) {
            if let Some(reason) = self.sensitive_paths.check(path) {
                tracing::info!(tool = tool_name, path = %path.display(), "denied: sensitive path");
                return SecurityDecision::Deny {
                    reason: format!("tool '{}' blocked: {}", tool_name, reason),
                };
            }
        }

        // 4. Root check applies ONLY to explicit paths — see note above.
        for path in &explicit_paths {
            if let Err(reason) = self.authorized_roots.check(path) {
                tracing::info!(tool = tool_name, path = %path.display(), "denied: outside roots");
                return SecurityDecision::Deny {
                    reason: format!("tool '{}' blocked: {}", tool_name, reason),
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
                        reason: format!("tool '{}' is cached as denied", tool_name),
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
                        reason: format!("tool '{}' blocked by permission rule", tool_name),
                    };
                }
                RuleDecision::Ask => {
                    // Fall through to HITL check below
                }
            }
        }

        // 7. Auto mode: use bash classifier for auto-approval
        if current_mode.auto_approves() && self.is_dangerous(tool_name) {
            if let Some(command) = Self::extract_command(args) {
                match BashClassifier::classify(&command) {
                    CommandClassification::ReadOnly => {
                        tracing::debug!(tool = tool_name, "auto-approved: read-only command");
                        return SecurityDecision::Allow;
                    }
                    CommandClassification::Destructive => {
                        tracing::info!(
                            tool = tool_name,
                            "denied: destructive command in auto mode"
                        );
                        return SecurityDecision::Deny {
                            reason: format!(
                                "tool '{}' blocked: destructive command '{}' not allowed in auto mode",
                                tool_name, command
                            ),
                        };
                    }
                    CommandClassification::Unknown => {
                        // Auto mode auto-approves unknown commands too (trusts sandbox)
                        tracing::debug!(
                            tool = tool_name,
                            "auto-approved: unknown command in auto mode"
                        );
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
                    let rx = self.permission_manager.request_approval(
                        request_id.clone(),
                        tool_name,
                        args,
                    );
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
        if let Some(path) = args
            .get("path")
            .or(args.get("file_path"))
            .and_then(|v| v.as_str())
        {
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
    /// Paths named explicitly in tool args (`file_path`, `path`, `cwd`…).
    /// Root check applies to these — a write/read tool with an explicit
    /// `file_path: "/etc/hosts"` is unambiguously trying to touch that file.
    fn extract_explicit_paths(&self, args: &Value) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Value::Object(map) = args {
            for (key, value) in map {
                let key_lower = key.to_lowercase();
                if self.path_keys.contains(&key_lower) {
                    if let Value::String(s) = value {
                        if !s.is_empty() {
                            paths.push(self.resolve_path_argument(s));
                        }
                    }
                }
            }
        }
        paths
    }

    /// Paths heuristically pulled out of shell command strings. Only the
    /// sensitive-path filter uses these — see `check_tool_call` step 2.
    fn extract_paths_from_args_commands(args: &Value) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Value::Object(map) = args {
            for (key, value) in map {
                if key.to_lowercase() == "command" {
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
        cancel: alva_kernel_abi::CancellationToken,
    }

    impl alva_kernel_abi::ToolExecutionContext for TestToolContext {
        fn cancel_token(&self) -> &alva_kernel_abi::CancellationToken {
            &self.cancel
        }
        fn session_id(&self) -> &str {
            "test-session"
        }
        fn workspace(&self) -> Option<&std::path::Path> {
            Some(&self.workspace)
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    fn test_ctx() -> TestToolContext {
        TestToolContext {
            workspace: PathBuf::from("/projects/myapp"),
            cancel: alva_kernel_abi::CancellationToken::new(),
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
        assert!(matches!(
            decision,
            SecurityDecision::NeedHumanApproval { .. }
        ));
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
        let paths = guard.extract_explicit_paths(&args);
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn shell_commands_not_root_checked() {
        // Regression: `ls /` used to extract "/" as a path and get denied
        // by the root check, making shell useless outside the workspace.
        // The command-path heuristic must not feed the root gate.
        let mut guard = SecurityGuard::new(
            PathBuf::from("/projects/myapp"),
            SandboxMode::RestrictiveOpen,
        );
        let args = json!({ "command": "ls /" });
        let decision = guard.check_tool_call("execute_shell", &args, &test_ctx());
        assert!(
            !matches!(&decision, SecurityDecision::Deny { reason } if reason.contains("outside")),
            "shell command `ls /` must not be denied by the root check: {decision:?}"
        );
    }

    #[test]
    fn explicit_path_outside_workspace_still_denied() {
        // Sanity: the split didn't accidentally loosen the explicit-path gate.
        let mut guard = SecurityGuard::new(
            PathBuf::from("/projects/myapp"),
            SandboxMode::RestrictiveOpen,
        );
        let args = json!({ "file_path": "/etc/hosts" });
        let decision = guard.check_tool_call("read_file", &args, &test_ctx());
        assert!(
            matches!(&decision, SecurityDecision::Deny { .. }),
            "explicit file_path outside workspace must still be denied"
        );
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
        guard.resolve_permission(
            &request_id,
            "execute_shell",
            crate::permission::PermissionDecision::AllowOnce,
        );
        let decision = rx.unwrap().await.unwrap();
        assert_eq!(decision, crate::permission::PermissionDecision::AllowOnce);
    }

    // ─── URL inspection (Loop C of T6 3C path) ────────────────────────

    fn fresh_guard() -> SecurityGuard {
        SecurityGuard::new(
            PathBuf::from("/projects/myapp"),
            SandboxMode::RestrictiveOpen,
        )
    }

    #[test]
    fn url_rules_default_is_some_medium_threshold() {
        // Default construction must give Some(Medium) — confirms the
        // wiring in `with_mode_handle` actually plumbs UrlRules::default()
        // and that nothing replaced it with `Default::default()` elsewhere
        // (which would give a different field in some future refactor).
        let g = fresh_guard();
        assert_eq!(g.url_rules().ask_threshold, Some(UrlRisk::Medium));
    }

    #[test]
    fn set_url_rules_overrides_threshold() {
        let mut g = fresh_guard();
        g.set_url_rules(UrlRules {
            ask_threshold: Some(UrlRisk::High),
        });
        assert_eq!(g.url_rules().ask_threshold, Some(UrlRisk::High));

        g.set_url_rules(UrlRules {
            ask_threshold: None,
        });
        assert_eq!(g.url_rules().ask_threshold, None);
    }

    #[test]
    fn should_ask_for_url_delegates_to_threshold() {
        let g = fresh_guard(); // default Medium
        assert!(!g.should_ask_for_url(UrlRisk::Low));
        assert!(g.should_ask_for_url(UrlRisk::Medium));
        assert!(g.should_ask_for_url(UrlRisk::High));
    }

    #[tokio::test]
    async fn inspect_url_through_guard_returns_high_for_imds() {
        // Smoke-check the delegate: SecurityGuard.inspect_url must hit
        // the same classifier `url_info::inspect_url` does, so IMDS
        // still resolves to High. If this fails, the delegate forgot to
        // await or is calling a stale function.
        let g = fresh_guard();
        let info = g
            .inspect_url("http://169.254.169.254/latest/meta-data/")
            .await;
        assert_eq!(info.risk, UrlRisk::High);
        assert_eq!(info.ip_class, Some(crate::url_info::IpClass::LinkLocal));
    }

    #[tokio::test]
    async fn inspect_url_through_guard_is_low_for_public_literal() {
        let g = fresh_guard();
        let info = g.inspect_url("https://8.8.8.8/").await;
        assert_eq!(info.risk, UrlRisk::Low);
        // Default threshold (Medium) → Low must NOT trigger HITL
        assert!(!g.should_ask_for_url(info.risk));
    }
}
