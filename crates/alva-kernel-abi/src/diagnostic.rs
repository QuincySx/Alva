// INPUT:  serde, std::path
// OUTPUT: Diagnostic, Severity
// POS:    Shared structured-diagnostic schema. Used by `alva-app-extension-tooling`
//         (cargo / tsc / eslint / ruff runners) and any future LSP-backed source.
//         Lives in kernel-abi so producers and consumers in different crates can
//         agree on the shape without dragging in a heavy dependency.

//! Structured diagnostics from any source — build tools, linters, LSP, etc.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Information,
    Hint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Absolute file path the diagnostic applies to.
    pub path: PathBuf,
    /// Zero-based line number (matches LSP convention).
    pub line: u32,
    /// Zero-based column on the start position.
    pub col: u32,
    pub severity: Severity,
    /// Source-specific code (e.g. `"E0308"` for rustc, `"no-unused-vars"` for ESLint).
    /// `None` if the source doesn't surface a stable code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Human-readable message.
    pub message: String,
    /// Origin tag (e.g. `"rustc"`, `"clippy"`, `"eslint"`, `"ruff"`, `"pyright"`,
    /// `"tsc"`, `"go-vet"`, `"rust-analyzer"`).
    pub source: String,
    /// Optional automated suggestion (clippy / eslint / ruff / biome surface these).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_minimal() {
        let d = Diagnostic {
            path: PathBuf::from("/x.rs"),
            line: 10,
            col: 4,
            severity: Severity::Error,
            code: None,
            message: "boom".into(),
            source: "rustc".into(),
            suggestion: None,
        };
        let json = serde_json::to_string(&d).unwrap();
        let back: Diagnostic = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d);
        // Optional fields omitted when None.
        assert!(!json.contains("\"code\""));
        assert!(!json.contains("\"suggestion\""));
    }

    #[test]
    fn round_trip_full() {
        let d = Diagnostic {
            path: PathBuf::from("/x.ts"),
            line: 5,
            col: 8,
            severity: Severity::Warning,
            code: Some("no-unused-vars".into()),
            message: "'x' is defined but never used".into(),
            source: "eslint".into(),
            suggestion: Some("remove unused variable".into()),
        };
        let json = serde_json::to_string(&d).unwrap();
        let back: Diagnostic = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn severity_serializes_snake_case() {
        let json = serde_json::to_string(&Severity::Information).unwrap();
        assert_eq!(json, "\"information\"");
    }
}
