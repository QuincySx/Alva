//! Diagnostic aggregation for LSP diagnostics.
//!
//! [`DiagnosticRegistry`] collects diagnostics published by language servers
//! and provides query / summary methods for the agent to consume.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// LSP diagnostic severity levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DiagnosticSeverity {
    Error = 1,
    Warning = 2,
    Information = 3,
    Hint = 4,
}

/// A single diagnostic entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Zero-based start line.
    pub start_line: u32,
    /// Zero-based start character.
    pub start_char: u32,
    /// Zero-based end line.
    pub end_line: u32,
    /// Zero-based end character.
    pub end_char: u32,
    /// Severity level.
    pub severity: DiagnosticSeverity,
    /// Human-readable message.
    pub message: String,
    /// Optional diagnostic code (e.g. `"E0308"`).
    #[serde(default)]
    pub code: Option<String>,
    /// Optional source (e.g. `"rust-analyzer"`).
    #[serde(default)]
    pub source: Option<String>,
}

/// Aggregates diagnostics keyed by file URI.
#[derive(Debug, Default)]
pub struct DiagnosticRegistry {
    entries: HashMap<String, Vec<Diagnostic>>,
}

impl DiagnosticRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set (replace) the diagnostics for a given file URI.
    pub fn set(&mut self, uri: impl Into<String>, diagnostics: Vec<Diagnostic>) {
        let uri = uri.into();
        if diagnostics.is_empty() {
            self.entries.remove(&uri);
        } else {
            self.entries.insert(uri, diagnostics);
        }
    }

    /// Get diagnostics for a specific file URI.
    pub fn get(&self, uri: &str) -> Option<&[Diagnostic]> {
        self.entries.get(uri).map(|v| v.as_slice())
    }

    /// Iterate over all (uri, diagnostics) pairs.
    pub fn all(&self) -> impl Iterator<Item = (&str, &[Diagnostic])> {
        self.entries
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_slice()))
    }

    /// Total number of error-level diagnostics across all files.
    pub fn error_count(&self) -> usize {
        self.entries
            .values()
            .flat_map(|v| v.iter())
            .filter(|d| d.severity == DiagnosticSeverity::Error)
            .count()
    }

    /// Total number of diagnostics across all files.
    pub fn total_count(&self) -> usize {
        self.entries.values().map(|v| v.len()).sum()
    }

    /// Clear all diagnostics.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Number of files with diagnostics.
    pub fn file_count(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error_diag(msg: &str) -> Diagnostic {
        Diagnostic {
            start_line: 0,
            start_char: 0,
            end_line: 0,
            end_char: 1,
            severity: DiagnosticSeverity::Error,
            message: msg.to_string(),
            code: None,
            source: None,
        }
    }

    fn warning_diag(msg: &str) -> Diagnostic {
        Diagnostic {
            start_line: 0,
            start_char: 0,
            end_line: 0,
            end_char: 1,
            severity: DiagnosticSeverity::Warning,
            message: msg.to_string(),
            code: None,
            source: None,
        }
    }

    #[test]
    fn set_and_get() {
        let mut reg = DiagnosticRegistry::new();
        reg.set("file:///a.rs", vec![error_diag("oops")]);
        assert_eq!(reg.get("file:///a.rs").unwrap().len(), 1);
    }

    #[test]
    fn set_empty_removes() {
        let mut reg = DiagnosticRegistry::new();
        reg.set("file:///a.rs", vec![error_diag("oops")]);
        reg.set("file:///a.rs", vec![]);
        assert!(reg.get("file:///a.rs").is_none());
    }

    #[test]
    fn error_count_filters_correctly() {
        let mut reg = DiagnosticRegistry::new();
        reg.set(
            "file:///a.rs",
            vec![error_diag("e1"), warning_diag("w1"), error_diag("e2")],
        );
        assert_eq!(reg.error_count(), 2);
        assert_eq!(reg.total_count(), 3);
    }

    #[test]
    fn clear_empties_all() {
        let mut reg = DiagnosticRegistry::new();
        reg.set("file:///a.rs", vec![error_diag("e")]);
        reg.set("file:///b.rs", vec![warning_diag("w")]);
        assert_eq!(reg.file_count(), 2);
        reg.clear();
        assert_eq!(reg.file_count(), 0);
        assert_eq!(reg.total_count(), 0);
    }

    #[test]
    fn all_iterates_entries() {
        let mut reg = DiagnosticRegistry::new();
        reg.set("file:///a.rs", vec![error_diag("e")]);
        reg.set("file:///b.rs", vec![warning_diag("w")]);
        let items: Vec<_> = reg.all().collect();
        assert_eq!(items.len(), 2);
    }
}
