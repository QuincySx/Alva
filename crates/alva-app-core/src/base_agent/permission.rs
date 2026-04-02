/// Controls how the agent handles tool permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionMode {
    /// All write/execute tools require human approval (default).
    Ask,
    /// All write tools auto-approved; shell commands still need approval.
    AcceptEdits,
    /// No tools execute — agent can only read and analyze.
    Plan,
}
