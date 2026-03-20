//! ResourceManifest — parses resource_manifest.json
//!
//! Defines what components are expected (runtime name, version, artifact format/URL).
//! Mirrors the Wukong `resource_manifest.json` structure.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Top-level manifest describing all bundled/downloadable runtime components.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceManifest {
    /// Profile identifier, e.g. "personal", "enterprise".
    pub profile: String,

    /// Target platform, e.g. "darwin-arm64", "darwin-x64", "windows-x64".
    pub platform: String,

    /// Component name -> expected version.
    /// A version value of `"excluded(platform: <p>)"` means the component is
    /// not available on that platform.
    pub components: HashMap<String, ComponentVersion>,

    /// Component name -> artifact packaging info (file name, format, optional URL).
    pub artifacts: HashMap<String, ArtifactConfig>,
}

/// Expected version for a single component.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentVersion {
    /// Semver-ish version string, or an exclusion marker like
    /// `"excluded(platform: darwin)"`.
    pub version: String,
}

impl ComponentVersion {
    /// Returns `true` when this component is excluded on the current platform.
    pub fn is_excluded(&self) -> bool {
        self.version.starts_with("excluded(")
    }

    /// If excluded, returns the reason string inside the parentheses.
    pub fn exclusion_reason(&self) -> Option<&str> {
        if self.version.starts_with("excluded(") && self.version.ends_with(')') {
            Some(&self.version["excluded(".len()..self.version.len() - 1])
        } else {
            None
        }
    }
}

/// Artifact packaging info for a component.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactConfig {
    /// Archive file name, e.g. "bun.zip", "node.tar.gz".
    pub file: String,

    /// Archive format: "zip-flat", "tar.gz", "qwen-zip".
    pub format: String,

    /// Optional download URL.  `None` means the artifact is bundled in the
    /// `packages/` directory alongside the application.
    pub url: Option<String>,
}

/// Supported archive formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    /// Standard zip — contents extracted flat into the target directory.
    ZipFlat,
    /// Gzipped tarball.
    TarGz,
    /// Special zip format for Qwen Code (npm package structure).
    QwenZip,
}

impl ArchiveFormat {
    /// Parse from the string stored in `ArtifactConfig::format`.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "zip-flat" => Some(Self::ZipFlat),
            "tar.gz" => Some(Self::TarGz),
            "qwen-zip" => Some(Self::QwenZip),
            _ => None,
        }
    }
}

impl ResourceManifest {
    /// Load a manifest from a JSON file on disk.
    pub fn from_file(path: &Path) -> Result<Self, ManifestError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| ManifestError::Io(path.to_path_buf(), e))?;
        Self::from_json(&content)
    }

    /// Parse a manifest from a JSON string.
    pub fn from_json(json: &str) -> Result<Self, ManifestError> {
        serde_json::from_str(json).map_err(ManifestError::Parse)
    }

    /// Serialize the manifest back to pretty-printed JSON.
    pub fn to_json(&self) -> Result<String, ManifestError> {
        serde_json::to_string_pretty(self).map_err(ManifestError::Parse)
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("Failed to read manifest at {0}: {1}")]
    Io(std::path::PathBuf, std::io::Error),

    #[error("Failed to parse manifest JSON: {0}")]
    Parse(serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_json() -> &'static str {
        r#"{
            "profile": "personal",
            "platform": "darwin-arm64",
            "components": {
                "bun": { "version": "1.2.17" },
                "node": { "version": "22.19.0" },
                "python": { "version": "3.12" },
                "uv": { "version": "0.7.13" },
                "chromium": { "version": "146" },
                "qwen": { "version": "0.10.0" },
                "dotnet": { "version": "excluded(platform: darwin)" }
            },
            "artifacts": {
                "bun": { "file": "bun.zip", "format": "zip-flat", "url": null },
                "node": { "file": "node.tar.gz", "format": "tar.gz", "url": "https://example.com/node.tar.gz" },
                "python": { "file": "python.tar.gz", "format": "tar.gz", "url": null },
                "uv": { "file": "uv.zip", "format": "zip-flat", "url": null },
                "chromium": { "file": "chromium.zip", "format": "zip-flat", "url": null },
                "qwen": { "file": "qwen.zip", "format": "qwen-zip", "url": null }
            }
        }"#
    }

    #[test]
    fn parse_manifest() {
        let manifest = ResourceManifest::from_json(sample_json()).unwrap();
        assert_eq!(manifest.profile, "personal");
        assert_eq!(manifest.platform, "darwin-arm64");
        assert_eq!(manifest.components.len(), 7);
        assert_eq!(manifest.components["bun"].version, "1.2.17");
        assert!(manifest.components["dotnet"].is_excluded());
        assert_eq!(
            manifest.components["dotnet"].exclusion_reason(),
            Some("platform: darwin")
        );
    }

    #[test]
    fn roundtrip_json() {
        let manifest = ResourceManifest::from_json(sample_json()).unwrap();
        let json = manifest.to_json().unwrap();
        let manifest2 = ResourceManifest::from_json(&json).unwrap();
        assert_eq!(manifest2.components.len(), manifest.components.len());
    }

    #[test]
    fn archive_format_parse() {
        assert_eq!(ArchiveFormat::from_str("zip-flat"), Some(ArchiveFormat::ZipFlat));
        assert_eq!(ArchiveFormat::from_str("tar.gz"), Some(ArchiveFormat::TarGz));
        assert_eq!(ArchiveFormat::from_str("qwen-zip"), Some(ArchiveFormat::QwenZip));
        assert_eq!(ArchiveFormat::from_str("unknown"), None);
    }
}
