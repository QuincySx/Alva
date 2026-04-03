// INPUT:  std::collections, std::sync, std::hash, serde_json
// OUTPUT: PermissionCache, CachedDecision
// POS:    Thread-safe cache for permission decisions, keyed by tool name + deterministic input hash.

use std::collections::HashMap;
use std::sync::RwLock;

/// Caches permission decisions for repeated tool uses.
///
/// Uses a deterministic hash of tool name + JSON input as the cache key.
/// Thread-safe via `RwLock`.
#[derive(Debug)]
pub struct PermissionCache {
    /// Cache key: "tool_name:input_hash" -> decision
    decisions: RwLock<HashMap<String, CachedDecision>>,
}

/// A cached permission decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachedDecision {
    /// Always allow this tool use.
    AllowAlways,
    /// Always deny this tool use.
    DenyAlways,
}

impl PermissionCache {
    /// Create a new empty permission cache.
    pub fn new() -> Self {
        Self {
            decisions: RwLock::new(HashMap::new()),
        }
    }

    /// Generate cache key from tool name and input.
    fn cache_key(tool_name: &str, input: &serde_json::Value) -> String {
        let input_str = serde_json::to_string(input).unwrap_or_default();
        let hash = simple_hash(&input_str);
        format!("{}:{}", tool_name, hash)
    }

    /// Check cache for a decision.
    pub fn get(&self, tool_name: &str, input: &serde_json::Value) -> Option<CachedDecision> {
        let key = Self::cache_key(tool_name, input);
        self.decisions.read().ok()?.get(&key).copied()
    }

    /// Store a decision in cache.
    pub fn set(&self, tool_name: &str, input: &serde_json::Value, decision: CachedDecision) {
        let key = Self::cache_key(tool_name, input);
        if let Ok(mut map) = self.decisions.write() {
            map.insert(key, decision);
        }
    }

    /// Remove a cached decision for a specific tool use.
    pub fn remove(&self, tool_name: &str, input: &serde_json::Value) {
        let key = Self::cache_key(tool_name, input);
        if let Ok(mut map) = self.decisions.write() {
            map.remove(&key);
        }
    }

    /// Clear all cached decisions.
    pub fn clear(&self) {
        if let Ok(mut map) = self.decisions.write() {
            map.clear();
        }
    }

    /// Return the number of cached decisions.
    pub fn len(&self) -> usize {
        self.decisions.read().map(|m| m.len()).unwrap_or(0)
    }

    /// Return whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for PermissionCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Simple deterministic hash using `DefaultHasher`.
fn simple_hash(s: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_cache_returns_none() {
        let cache = PermissionCache::new();
        assert_eq!(cache.get("Bash", &json!({"command": "ls"})), None);
    }

    #[test]
    fn set_and_get() {
        let cache = PermissionCache::new();
        let input = json!({"command": "ls"});
        cache.set("Bash", &input, CachedDecision::AllowAlways);
        assert_eq!(cache.get("Bash", &input), Some(CachedDecision::AllowAlways));
    }

    #[test]
    fn different_inputs_different_keys() {
        let cache = PermissionCache::new();
        let input_a = json!({"command": "ls"});
        let input_b = json!({"command": "rm -rf /"});
        cache.set("Bash", &input_a, CachedDecision::AllowAlways);
        cache.set("Bash", &input_b, CachedDecision::DenyAlways);
        assert_eq!(cache.get("Bash", &input_a), Some(CachedDecision::AllowAlways));
        assert_eq!(cache.get("Bash", &input_b), Some(CachedDecision::DenyAlways));
    }

    #[test]
    fn different_tools_different_keys() {
        let cache = PermissionCache::new();
        let input = json!({"path": "file.txt"});
        cache.set("Read", &input, CachedDecision::AllowAlways);
        assert_eq!(cache.get("Read", &input), Some(CachedDecision::AllowAlways));
        assert_eq!(cache.get("Edit", &input), None);
    }

    #[test]
    fn clear_removes_all() {
        let cache = PermissionCache::new();
        cache.set("Bash", &json!({"command": "ls"}), CachedDecision::AllowAlways);
        cache.set("Read", &json!({"path": "f.txt"}), CachedDecision::DenyAlways);
        assert_eq!(cache.len(), 2);

        cache.clear();
        assert!(cache.is_empty());
        assert_eq!(cache.get("Bash", &json!({"command": "ls"})), None);
    }

    #[test]
    fn remove_specific_entry() {
        let cache = PermissionCache::new();
        let input = json!({"command": "ls"});
        cache.set("Bash", &input, CachedDecision::AllowAlways);
        assert_eq!(cache.len(), 1);

        cache.remove("Bash", &input);
        assert!(cache.is_empty());
    }

    #[test]
    fn overwrite_decision() {
        let cache = PermissionCache::new();
        let input = json!({"command": "ls"});
        cache.set("Bash", &input, CachedDecision::AllowAlways);
        cache.set("Bash", &input, CachedDecision::DenyAlways);
        assert_eq!(cache.get("Bash", &input), Some(CachedDecision::DenyAlways));
    }

    #[test]
    fn default_is_empty() {
        let cache = PermissionCache::default();
        assert!(cache.is_empty());
    }
}
