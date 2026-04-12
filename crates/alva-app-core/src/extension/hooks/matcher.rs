//! Hook matching — determines which hooks fire for a given event + context.

use crate::settings::HookConfig;

/// Check whether a hook config matches the given query string.
///
/// Matching rules:
/// - `None` matcher → always matches
/// - Exact match (e.g., `"Bash"` matches `"Bash"`)
/// - Wildcard `"*"` matches everything
/// - Prefix wildcard `"Bash*"` matches `"BashTool"`, `"Bash"`
/// - Suffix wildcard `"*Tool"` matches `"BashTool"`, `"ReadTool"`
/// - Contains wildcard `"*edit*"` matches `"file_edit"`, `"FileEditTool"`
pub fn matches_hook(config: &HookConfig, query: Option<&str>) -> bool {
    let matcher = match &config.matcher {
        None => return true, // No matcher = always match
        Some(m) if m == "*" => return true,
        Some(m) => m.as_str(),
    };

    let query = match query {
        Some(q) => q,
        None => return false, // Has matcher but no query to compare
    };

    // Exact match
    if matcher == query {
        return true;
    }

    // Case-insensitive exact match
    if matcher.eq_ignore_ascii_case(query) {
        return true;
    }

    // Wildcard patterns
    let lower_matcher = matcher.to_ascii_lowercase();
    let lower_query = query.to_ascii_lowercase();

    if lower_matcher.starts_with('*') && lower_matcher.ends_with('*') && lower_matcher.len() > 2 {
        // *pattern* → contains
        let pattern = &lower_matcher[1..lower_matcher.len() - 1];
        return lower_query.contains(pattern);
    }
    if lower_matcher.ends_with('*') {
        // pattern* → prefix
        let prefix = &lower_matcher[..lower_matcher.len() - 1];
        return lower_query.starts_with(prefix);
    }
    if lower_matcher.starts_with('*') {
        // *pattern → suffix
        let suffix = &lower_matcher[1..];
        return lower_query.ends_with(suffix);
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{HookConfig, HookEntry};

    fn config_with_matcher(matcher: Option<&str>) -> HookConfig {
        HookConfig {
            matcher: matcher.map(String::from),
            hooks: vec![HookEntry {
                hook_type: "command".to_string(),
                command: "echo test".to_string(),
                timeout: None,
            }],
        }
    }

    #[test]
    fn no_matcher_always_matches() {
        let config = config_with_matcher(None);
        assert!(matches_hook(&config, Some("Bash")));
        assert!(matches_hook(&config, None));
    }

    #[test]
    fn wildcard_star_matches_everything() {
        let config = config_with_matcher(Some("*"));
        assert!(matches_hook(&config, Some("Bash")));
        assert!(matches_hook(&config, Some("anything")));
    }

    #[test]
    fn exact_match() {
        let config = config_with_matcher(Some("Bash"));
        assert!(matches_hook(&config, Some("Bash")));
        assert!(!matches_hook(&config, Some("Read")));
    }

    #[test]
    fn case_insensitive_exact() {
        let config = config_with_matcher(Some("bash"));
        assert!(matches_hook(&config, Some("Bash")));
        assert!(matches_hook(&config, Some("BASH")));
    }

    #[test]
    fn prefix_wildcard() {
        let config = config_with_matcher(Some("Bash*"));
        assert!(matches_hook(&config, Some("Bash")));
        assert!(matches_hook(&config, Some("BashTool")));
        assert!(!matches_hook(&config, Some("Read")));
    }

    #[test]
    fn suffix_wildcard() {
        let config = config_with_matcher(Some("*Tool"));
        assert!(matches_hook(&config, Some("BashTool")));
        assert!(matches_hook(&config, Some("ReadTool")));
        assert!(!matches_hook(&config, Some("Bash")));
    }

    #[test]
    fn contains_wildcard() {
        let config = config_with_matcher(Some("*edit*"));
        assert!(matches_hook(&config, Some("file_edit")));
        assert!(matches_hook(&config, Some("FileEditTool")));
        assert!(!matches_hook(&config, Some("Bash")));
    }

    #[test]
    fn matcher_with_no_query() {
        let config = config_with_matcher(Some("Bash"));
        assert!(!matches_hook(&config, None));
    }
}
