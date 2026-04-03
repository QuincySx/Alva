//! LSP request / response types.
//!
//! These types model the subset of LSP operations that the agent needs to
//! interact with language servers. They are intentionally simplified compared
//! to the full LSP specification.

use serde::{Deserialize, Serialize};

// ── Location ────────────────────────────────────────────────────────

/// A location in a source file, mirroring LSP's `Location`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LspLocation {
    /// File URI (e.g. `file:///path/to/file.rs`).
    pub uri: String,
    /// Zero-based start line.
    pub start_line: u32,
    /// Zero-based start character (column).
    pub start_char: u32,
    /// Zero-based end line.
    pub end_line: u32,
    /// Zero-based end character (column).
    pub end_char: u32,
}

// ── Hover ───────────────────────────────────────────────────────────

/// Result of a hover request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspHoverResult {
    /// The markdown / plain-text content of the hover.
    pub contents: String,
    /// Optional range that the hover applies to.
    pub range: Option<LspLocation>,
}

// ── Symbol ──────────────────────────────────────────────────────────

/// A symbol returned by document-symbol or workspace-symbol requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspSymbol {
    /// Symbol name.
    pub name: String,
    /// LSP SymbolKind numeric value.
    pub kind: u32,
    /// Location of the symbol.
    pub location: LspLocation,
    /// Optional container name (e.g. the struct a method belongs to).
    #[serde(default)]
    pub container_name: Option<String>,
}

// ── Request ─────────────────────────────────────────────────────────

/// An LSP request the agent can issue.
#[derive(Debug, Clone)]
pub enum LspRequest {
    /// Go to the definition of the symbol at the given location.
    GoToDefinition {
        uri: String,
        line: u32,
        character: u32,
    },
    /// Find all references to the symbol at the given location.
    FindReferences {
        uri: String,
        line: u32,
        character: u32,
        include_declaration: bool,
    },
    /// Retrieve hover information.
    Hover {
        uri: String,
        line: u32,
        character: u32,
    },
    /// List all symbols in a document.
    DocumentSymbol {
        uri: String,
    },
    /// Search for symbols across the workspace.
    WorkspaceSymbol {
        query: String,
    },
    /// Go to the implementation(s) of a symbol.
    GoToImplementation {
        uri: String,
        line: u32,
        character: u32,
    },
}

// ── Response ────────────────────────────────────────────────────────

/// The response to an [`LspRequest`].
#[derive(Debug, Clone)]
pub enum LspResponse {
    /// One or more locations (definition, references, implementation).
    Locations(Vec<LspLocation>),
    /// Hover content.
    Hover(Option<LspHoverResult>),
    /// A list of symbols (document or workspace).
    Symbols(Vec<LspSymbol>),
    /// The server returned an error.
    Error(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn location_equality() {
        let a = LspLocation {
            uri: "file:///a.rs".into(),
            start_line: 0,
            start_char: 0,
            end_line: 0,
            end_char: 5,
        };
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn request_variants_constructable() {
        let _ = LspRequest::GoToDefinition {
            uri: "file:///a.rs".into(),
            line: 10,
            character: 4,
        };
        let _ = LspRequest::FindReferences {
            uri: "file:///a.rs".into(),
            line: 10,
            character: 4,
            include_declaration: true,
        };
        let _ = LspRequest::Hover {
            uri: "file:///a.rs".into(),
            line: 10,
            character: 4,
        };
        let _ = LspRequest::DocumentSymbol {
            uri: "file:///a.rs".into(),
        };
        let _ = LspRequest::WorkspaceSymbol {
            query: "foo".into(),
        };
        let _ = LspRequest::GoToImplementation {
            uri: "file:///a.rs".into(),
            line: 10,
            character: 4,
        };
    }

    #[test]
    fn response_locations() {
        let resp = LspResponse::Locations(vec![LspLocation {
            uri: "file:///a.rs".into(),
            start_line: 1,
            start_char: 0,
            end_line: 1,
            end_char: 10,
        }]);
        match resp {
            LspResponse::Locations(locs) => assert_eq!(locs.len(), 1),
            _ => panic!("expected Locations"),
        }
    }
}
