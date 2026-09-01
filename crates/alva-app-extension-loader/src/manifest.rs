// INPUT:  serde, std::path
// OUTPUT: PluginManifest, Runtime, PluginManifest::validate
// POS:    Validated declarative contract for one subprocess extension bundle.
//! Plugin manifest — the `alva.toml` file found in every plugin
//! package directory (`~/.alva/extensions/<name>/alva.toml`).
//!
//! The host reads this during plugin discovery to decide which
//! runtime to spawn and which entry file to hand it.

use std::path::{Component, Path};

use serde::{Deserialize, Serialize};

/// Top-level `alva.toml` schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// `"host:memory.write"`). Each `host/<name>` reverse call requires the
    /// matching `host:<name>` entry: an undeclared call is denied with
    /// `CAPABILITY_DENIED` (fail-closed), never silently observed.
    #[serde(default)]
    pub requested_capabilities: Vec<String>,

    /// Optional SHA-256 pin of the normalized bundle contents. The loader
    /// hashes the semantic manifest (with this field cleared) plus every
    /// regular bundle file in stable path order. A mismatch fails closed.
    #[serde(default)]
    pub content_sha256: Option<String>,
}

impl PluginManifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.is_empty()
            || self.name.len() > 64
            || !self
                .name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(
                "plugin name must be 1-64 ASCII letters, digits, '.', '-' or '_'".to_string(),
            );
        }
        if self.version.is_empty()
            || self.version.len() > 64
            || self.version.chars().any(char::is_control)
        {
            return Err("plugin version must be 1-64 printable characters".to_string());
        }

        let entry = Path::new(&self.entry);
        if self.entry.is_empty()
            || self.entry.len() > 512
            || entry.is_absolute()
            || !entry
                .components()
                .all(|component| matches!(component, Component::Normal(_)))
        {
            return Err(
                "plugin entry must be a relative path without '.', '..' or root components"
                    .to_string(),
            );
        }
        let extension = entry.extension().and_then(|value| value.to_str());
        let extension_ok = match self.runtime {
            Runtime::Python => extension == Some("py"),
            Runtime::Javascript => matches!(extension, Some("js" | "mjs" | "cjs")),
        };
        if !extension_ok {
            return Err(format!(
                "plugin entry extension does not match {:?} runtime",
                self.runtime
            ));
        }

        if self.requested_capabilities.len() > 32 {
            return Err("plugin requests more than 32 host capabilities".to_string());
        }
        let mut seen = std::collections::HashSet::new();
        for capability in &self.requested_capabilities {
            if capability.len() > 128
                || !capability.starts_with("host:")
                || !capability.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'.' | b'-' | b'_')
                })
            {
                return Err(format!(
                    "invalid host capability declaration: {capability:?}"
                ));
            }
            if !seen.insert(capability) {
                return Err(format!(
                    "duplicate host capability declaration: {capability}"
                ));
            }
        }

        if let Some(digest) = &self.content_sha256 {
            if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err("content_sha256 must be exactly 64 hexadecimal characters".to_string());
            }
        }
        Ok(())
    }
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

    #[test]
    fn entry_cannot_escape_plugin_directory() {
        for entry in [
            "../outside.py",
            "nested/../../outside.py",
            "/tmp/outside.py",
        ] {
            let manifest = PluginManifest {
                name: "escape-attempt".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                author: None,
                runtime: Runtime::Python,
                entry: entry.to_string(),
                requested_capabilities: Vec::new(),
                content_sha256: None,
            };

            assert!(
                manifest.validate().is_err(),
                "entry {entry:?} must be rejected"
            );
        }
    }

    #[test]
    fn digest_pin_requires_exact_sha256_hex_shape() {
        for digest in ["abc".to_string(), "g".repeat(64), "0".repeat(63)] {
            let manifest = PluginManifest {
                name: "bad-pin".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                author: None,
                runtime: Runtime::Python,
                entry: "main.py".to_string(),
                requested_capabilities: Vec::new(),
                content_sha256: Some(digest.clone()),
            };

            assert!(
                manifest.validate().is_err(),
                "digest {digest:?} must be rejected"
            );
        }
    }
}
