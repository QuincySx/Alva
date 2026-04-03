// INPUT:  serde
// OUTPUT: PermissionRule, RuleDecision, PermissionRules, matches_pattern, glob_match
// POS:    Fine-grained permission rules system matching tool uses by pattern with deny > ask > allow priority.

use serde::{Deserialize, Serialize};

/// A permission rule that matches tool uses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    /// Tool name pattern (supports wildcards: "Bash", "Bash(git *)", "Read(*)")
    pub tool_pattern: String,
    /// Decision for matching tool uses
    pub decision: RuleDecision,
}

/// Decision outcome for a permission rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleDecision {
    /// Tool use is allowed without asking.
    Allow,
    /// Tool use is denied.
    Deny,
    /// Tool use requires asking the user.
    Ask,
}

/// Permission rules configuration.
///
/// Rules are checked with priority: deny > ask > allow > default(ask).
/// Each list contains tool patterns that may include argument matchers,
/// e.g. `"Bash"`, `"Bash(git *)"`, `"Read(*)"`, `"Edit(src/**)"`
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PermissionRules {
    /// Rules that allow tool use without asking.
    #[serde(default)]
    pub allow: Vec<String>,
    /// Rules that deny tool use.
    #[serde(default)]
    pub deny: Vec<String>,
    /// Rules that require asking (override allow).
    #[serde(default)]
    pub ask: Vec<String>,
}

impl PermissionRules {
    /// Check if a tool use matches any rule.
    ///
    /// Priority: deny > ask > allow > default(ask)
    pub fn check(&self, tool_name: &str, input_summary: &str) -> RuleDecision {
        let full_pattern = format!("{}({})", tool_name, input_summary);

        // Check deny rules first (highest priority)
        for pattern in &self.deny {
            if matches_pattern(pattern, tool_name, &full_pattern) {
                return RuleDecision::Deny;
            }
        }

        // Check ask rules (override allow)
        for pattern in &self.ask {
            if matches_pattern(pattern, tool_name, &full_pattern) {
                return RuleDecision::Ask;
            }
        }

        // Check allow rules
        for pattern in &self.allow {
            if matches_pattern(pattern, tool_name, &full_pattern) {
                return RuleDecision::Allow;
            }
        }

        // Default: ask
        RuleDecision::Ask
    }

    /// Returns true if no rules have been configured.
    pub fn is_empty(&self) -> bool {
        self.allow.is_empty() && self.deny.is_empty() && self.ask.is_empty()
    }
}

/// Match a pattern against tool name and full tool use string.
///
/// Supports patterns like:
/// - `"Bash"` — matches all Bash uses
/// - `"Bash(git *)"` — matches Bash with git commands
/// - `"Read(*)"` — matches all Read uses
/// - `"Edit(src/**)"` — matches Edit on src/ files
fn matches_pattern(pattern: &str, tool_name: &str, full: &str) -> bool {
    // Simple pattern: just tool name
    if !pattern.contains('(') {
        return pattern == tool_name;
    }

    // Pattern with arguments: "ToolName(arg_pattern)"
    if let Some(paren_pos) = pattern.find('(') {
        let pat_name = &pattern[..paren_pos];
        if pat_name != tool_name {
            return false;
        }

        // Extract the argument pattern between the parens
        if !pattern.ends_with(')') {
            return false;
        }
        let arg_pattern = &pattern[paren_pos + 1..pattern.len() - 1];
        // Match the argument pattern against the argument portion of full
        // full is "ToolName(args)", extract args
        if let Some(full_paren) = full.find('(') {
            if !full.ends_with(')') {
                return false;
            }
            let full_args = &full[full_paren + 1..full.len() - 1];
            glob_match(arg_pattern, full_args)
        } else {
            false
        }
    } else {
        false
    }
}

/// Simple glob matching (supports `*` and `**`).
///
/// - `*` matches any sequence of characters (non-greedy within a segment)
/// - `**` matches any sequence of characters (greedy, including path separators)
fn glob_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" || pattern == "**" {
        return true;
    }

    // Split on * and check if text contains all parts in order
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        // No wildcards
        return pattern == text;
    }

    let mut remaining = text;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            // First part must be at the start
            if !remaining.starts_with(part) {
                return false;
            }
            remaining = &remaining[part.len()..];
        } else if i == parts.len() - 1 {
            // Last part must be at the end
            if !remaining.ends_with(part) {
                return false;
            }
        } else {
            // Middle parts must exist somewhere
            if let Some(pos) = remaining.find(part) {
                remaining = &remaining[pos + part.len()..];
            } else {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- glob_match tests ----

    #[test]
    fn glob_star_matches_everything() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*", ""));
    }

    #[test]
    fn glob_double_star_matches_everything() {
        assert!(glob_match("**", "a/b/c"));
    }

    #[test]
    fn glob_exact_match() {
        assert!(glob_match("hello", "hello"));
        assert!(!glob_match("hello", "world"));
    }

    #[test]
    fn glob_prefix_wildcard() {
        assert!(glob_match("git *", "git status"));
        assert!(glob_match("git *", "git push --force"));
        assert!(!glob_match("git *", "npm install"));
    }

    #[test]
    fn glob_suffix_wildcard() {
        assert!(glob_match("*.rs", "main.rs"));
        assert!(!glob_match("*.rs", "main.py"));
    }

    #[test]
    fn glob_middle_wildcard() {
        assert!(glob_match("src/*.rs", "src/main.rs"));
        assert!(!glob_match("src/*.rs", "lib/main.rs"));
    }

    #[test]
    fn glob_double_star_path() {
        assert!(glob_match("src/**", "src/a/b/c.rs"));
    }

    // ---- matches_pattern tests ----

    #[test]
    fn simple_tool_name_match() {
        assert!(matches_pattern("Bash", "Bash", "Bash(ls)"));
        assert!(!matches_pattern("Bash", "Read", "Read(file.txt)"));
    }

    #[test]
    fn tool_pattern_with_wildcard_args() {
        assert!(matches_pattern("Read(*)", "Read", "Read(file.txt)"));
        assert!(!matches_pattern("Read(*)", "Bash", "Bash(ls)"));
    }

    #[test]
    fn tool_pattern_with_prefix_args() {
        assert!(matches_pattern("Bash(git *)", "Bash", "Bash(git status)"));
        assert!(!matches_pattern("Bash(git *)", "Bash", "Bash(npm install)"));
    }

    #[test]
    fn tool_pattern_with_path_args() {
        assert!(matches_pattern("Edit(src/*)", "Edit", "Edit(src/main.rs)"));
        assert!(!matches_pattern("Edit(src/*)", "Edit", "Edit(lib/main.rs)"));
    }

    // ---- PermissionRules tests ----

    #[test]
    fn default_decision_is_ask() {
        let rules = PermissionRules::default();
        assert_eq!(rules.check("Bash", "ls"), RuleDecision::Ask);
    }

    #[test]
    fn allow_rule_matches() {
        let rules = PermissionRules {
            allow: vec!["Read".to_string()],
            deny: vec![],
            ask: vec![],
        };
        assert_eq!(rules.check("Read", "file.txt"), RuleDecision::Allow);
    }

    #[test]
    fn deny_overrides_allow() {
        let rules = PermissionRules {
            allow: vec!["Bash".to_string()],
            deny: vec!["Bash".to_string()],
            ask: vec![],
        };
        assert_eq!(rules.check("Bash", "rm -rf /"), RuleDecision::Deny);
    }

    #[test]
    fn ask_overrides_allow() {
        let rules = PermissionRules {
            allow: vec!["Bash".to_string()],
            deny: vec![],
            ask: vec!["Bash(rm *)".to_string()],
        };
        assert_eq!(rules.check("Bash", "rm -rf /"), RuleDecision::Ask);
        assert_eq!(rules.check("Bash", "ls"), RuleDecision::Allow);
    }

    #[test]
    fn deny_overrides_ask() {
        let rules = PermissionRules {
            allow: vec![],
            deny: vec!["Bash(rm *)".to_string()],
            ask: vec!["Bash".to_string()],
        };
        assert_eq!(rules.check("Bash", "rm -rf /"), RuleDecision::Deny);
        assert_eq!(rules.check("Bash", "ls"), RuleDecision::Ask);
    }

    #[test]
    fn allow_with_arg_pattern() {
        let rules = PermissionRules {
            allow: vec!["Bash(git *)".to_string()],
            deny: vec![],
            ask: vec![],
        };
        assert_eq!(rules.check("Bash", "git status"), RuleDecision::Allow);
        assert_eq!(rules.check("Bash", "npm install"), RuleDecision::Ask);
    }

    #[test]
    fn is_empty_when_no_rules() {
        let rules = PermissionRules::default();
        assert!(rules.is_empty());
    }

    #[test]
    fn is_not_empty_with_rules() {
        let rules = PermissionRules {
            allow: vec!["Read".to_string()],
            deny: vec![],
            ask: vec![],
        };
        assert!(!rules.is_empty());
    }

    #[test]
    fn serde_roundtrip() {
        let rules = PermissionRules {
            allow: vec!["Read".to_string(), "Bash(git *)".to_string()],
            deny: vec!["Bash(rm *)".to_string()],
            ask: vec!["Edit".to_string()],
        };
        let json = serde_json::to_string(&rules).unwrap();
        let deserialized: PermissionRules = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.allow.len(), 2);
        assert_eq!(deserialized.deny.len(), 1);
        assert_eq!(deserialized.ask.len(), 1);
    }

    #[test]
    fn rule_decision_serde() {
        let rule = PermissionRule {
            tool_pattern: "Bash".to_string(),
            decision: RuleDecision::Allow,
        };
        let json = serde_json::to_string(&rule).unwrap();
        assert!(json.contains("\"allow\""));
        let deserialized: PermissionRule = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.decision, RuleDecision::Allow);
    }
}
