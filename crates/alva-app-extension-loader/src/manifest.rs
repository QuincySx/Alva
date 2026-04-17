//! Plugin manifest — the `alva.toml` file found in every plugin
//! package directory (`~/.alva/extensions/<name>/alva.toml`).
//!
//! The host reads this during plugin discovery to decide which
//! runtime to spawn and which entry file to hand it.

use serde::{Deserialize, Serialize};

/// Top-level `alva.toml` schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub author: Option<String>,

    /// Which runtime should spawn this plugin.
    pub runtime: Runtime,

    /// Entry file relative to the plugin directory.
    ///
    /// Interpreted by the runtime: for `python` it's the `.py` file
    /// passed to `python -m alva_sdk <entry>`; for `javascript` it's
    /// the `.js` / `.mjs` entry passed to the node launcher.
    pub entry: String,

    /// Capabilities the plugin asks for (e.g. `"host:get_state"`,
    /// `"host:memory.write"`). In v1 this is observation-only —
    /// the host logs a warning when a plugin calls a method it did
    /// not declare, but does not block. v0.2 will switch to strict
    /// enforcement.
    #[serde(default)]
    pub requested_capabilities: Vec<String>,
}

/// Runtime backends recognised by the subprocess loader.
///
/// Each variant maps to a launcher command the host knows how to
/// exec. Adding a new variant is the only change needed to support
/// another language, provided an SDK exists for that language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Runtime {
    /// `python -m alva_sdk <entry>`
    Python,
    /// `node --enable-source-maps <alva-sdk-launcher> <entry>`
    Javascript,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_python_manifest() {
        let toml_like_json = serde_json::json!({
            "name": "my-memory",
            "version": "0.1.0",
            "description": "A simple key-value memory",
            "runtime": "python",
            "entry": "main.py",
            "requested_capabilities": ["host:log", "host:memory.write"]
        });

        let m: PluginManifest = serde_json::from_value(toml_like_json).unwrap();
        assert_eq!(m.name, "my-memory");
        assert_eq!(m.runtime, Runtime::Python);
        assert_eq!(m.entry, "main.py");
        assert_eq!(m.requested_capabilities.len(), 2);
    }

    #[test]
    fn defaults_empty_capabilities() {
        let json = serde_json::json!({
            "name": "bare",
            "version": "0.0.1",
            "runtime": "javascript",
            "entry": "main.js"
        });
        let m: PluginManifest = serde_json::from_value(json).unwrap();
        assert!(m.requested_capabilities.is_empty());
        assert!(m.description.is_none());
    }
}
