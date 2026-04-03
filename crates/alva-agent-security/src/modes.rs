// INPUT:  serde
// OUTPUT: PermissionMode
// POS:    Defines permission modes that determine how tool use permissions are handled (interactive, auto, plan, bypass).

use serde::{Deserialize, Serialize};

/// Permission mode determines how tool use permissions are handled.
///
/// Models the different operational modes seen in Claude Code:
/// - `Interactive` — ask user for each unrecognized tool use
/// - `Auto` — use classifier to auto-approve safe operations
/// - `Plan` — read-only mode, deny all write/execute operations
/// - `Bypass` — allow everything (requires sandbox)
/// - `Default` — same as Interactive
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    /// Interactive: Ask user for each unrecognized tool use.
    Interactive,
    /// Auto: Use classifier to auto-approve safe operations.
    Auto,
    /// Plan: Read-only mode, deny all write/execute operations.
    Plan,
    /// Bypass: Allow everything (requires sandbox).
    Bypass,
    /// Default: Same as Interactive.
    Default,
}

impl std::default::Default for PermissionMode {
    fn default() -> Self {
        Self::Default
    }
}

impl PermissionMode {
    /// Whether this mode allows write operations.
    pub fn allows_writes(&self) -> bool {
        !matches!(self, Self::Plan)
    }

    /// Whether this mode requires user confirmation for unrecognized operations.
    pub fn requires_confirmation(&self) -> bool {
        matches!(self, Self::Interactive | Self::Default)
    }

    /// Whether this mode should auto-approve safe operations.
    pub fn auto_approves(&self) -> bool {
        matches!(self, Self::Auto | Self::Bypass)
    }

    /// Whether this mode requires a sandbox for safe operation.
    pub fn requires_sandbox(&self) -> bool {
        matches!(self, Self::Bypass)
    }
}

impl std::fmt::Display for PermissionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Interactive => write!(f, "interactive"),
            Self::Auto => write!(f, "auto"),
            Self::Plan => write!(f, "plan"),
            Self::Bypass => write!(f, "bypass"),
            Self::Default => write!(f, "default"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_default_variant() {
        assert_eq!(PermissionMode::default(), PermissionMode::Default);
    }

    #[test]
    fn plan_disallows_writes() {
        assert!(!PermissionMode::Plan.allows_writes());
    }

    #[test]
    fn non_plan_allows_writes() {
        assert!(PermissionMode::Interactive.allows_writes());
        assert!(PermissionMode::Auto.allows_writes());
        assert!(PermissionMode::Bypass.allows_writes());
        assert!(PermissionMode::Default.allows_writes());
    }

    #[test]
    fn interactive_requires_confirmation() {
        assert!(PermissionMode::Interactive.requires_confirmation());
        assert!(PermissionMode::Default.requires_confirmation());
    }

    #[test]
    fn auto_does_not_require_confirmation() {
        assert!(!PermissionMode::Auto.requires_confirmation());
        assert!(!PermissionMode::Bypass.requires_confirmation());
        assert!(!PermissionMode::Plan.requires_confirmation());
    }

    #[test]
    fn auto_and_bypass_auto_approve() {
        assert!(PermissionMode::Auto.auto_approves());
        assert!(PermissionMode::Bypass.auto_approves());
    }

    #[test]
    fn interactive_does_not_auto_approve() {
        assert!(!PermissionMode::Interactive.auto_approves());
        assert!(!PermissionMode::Plan.auto_approves());
        assert!(!PermissionMode::Default.auto_approves());
    }

    #[test]
    fn only_bypass_requires_sandbox() {
        assert!(PermissionMode::Bypass.requires_sandbox());
        assert!(!PermissionMode::Interactive.requires_sandbox());
        assert!(!PermissionMode::Auto.requires_sandbox());
        assert!(!PermissionMode::Plan.requires_sandbox());
        assert!(!PermissionMode::Default.requires_sandbox());
    }

    #[test]
    fn display_format() {
        assert_eq!(format!("{}", PermissionMode::Interactive), "interactive");
        assert_eq!(format!("{}", PermissionMode::Auto), "auto");
        assert_eq!(format!("{}", PermissionMode::Plan), "plan");
        assert_eq!(format!("{}", PermissionMode::Bypass), "bypass");
        assert_eq!(format!("{}", PermissionMode::Default), "default");
    }

    #[test]
    fn serde_roundtrip() {
        let mode = PermissionMode::Auto;
        let json = serde_json::to_string(&mode).unwrap();
        assert_eq!(json, "\"auto\"");
        let deserialized: PermissionMode = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, PermissionMode::Auto);
    }

    #[test]
    fn serde_all_variants() {
        for (variant, expected_str) in [
            (PermissionMode::Interactive, "\"interactive\""),
            (PermissionMode::Auto, "\"auto\""),
            (PermissionMode::Plan, "\"plan\""),
            (PermissionMode::Bypass, "\"bypass\""),
            (PermissionMode::Default, "\"default\""),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected_str);
            let back: PermissionMode = serde_json::from_str(&json).unwrap();
            assert_eq!(back, variant);
        }
    }
}
