// INPUT:  serde, std::collections, std::path, super::manifest
// OUTPUT: InstalledVersions, VersionStatus, VersionError
// POS:    Tracks installed component versions via versions.json and compares against the manifest.
//! Version management — tracks installed vs expected versions.
//!
//! Persists a `versions.json` alongside the environment directory so that the
//! installer can determine which components need updating.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use super::manifest::{ComponentVersion, ResourceManifest};

/// On-disk record of currently installed component versions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstalledVersions {
    /// component name -> installed version string.
    pub components: HashMap<String, String>,
}

/// Result of comparing a single component's installed version against the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionStatus {
    /// Installed version matches the manifest.
    UpToDate,
    /// Installed version differs from the manifest.
    NeedsUpdate {
        current: String,
        expected: String,
    },
    /// Component has no installed version.
    NotInstalled,
    /// Component is excluded on this platform (with reason).
    Excluded(String),
}

impl InstalledVersions {
    /// Load from a `versions.json` file.  Returns a default (empty) instance
    /// when the file does not exist.
    pub fn from_file(path: &Path) -> Result<Self, VersionError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)
            .map_err(|e| VersionError::Io(path.to_path_buf(), e))?;
        serde_json::from_str(&content).map_err(VersionError::Parse)
    }

    /// Persist to a `versions.json` file, creating parent directories as needed.
    pub fn save(&self, path: &Path) -> Result<(), VersionError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| VersionError::Io(parent.to_path_buf(), e))?;
        }
        let json = serde_json::to_string_pretty(self).map_err(VersionError::Parse)?;
        std::fs::write(path, json).map_err(|e| VersionError::Io(path.to_path_buf(), e))
    }

    /// Record that a component has been installed at a given version.
    pub fn set_version(&mut self, component: &str, version: &str) {
        self.components
            .insert(component.to_string(), version.to_string());
    }

    /// Compare all manifest components against what is installed and return a
    /// status map.
    pub fn check_all(&self, manifest: &ResourceManifest) -> HashMap<String, VersionStatus> {
        let mut result = HashMap::new();
        for (name, cv) in &manifest.components {
            result.insert(name.clone(), self.check_one(name, cv));
        }
        result
    }

    /// Compare a single component.
    pub fn check_one(&self, name: &str, expected: &ComponentVersion) -> VersionStatus {
        if expected.is_excluded() {
            return VersionStatus::Excluded(expected.version.clone());
        }
        match self.components.get(name) {
            None => VersionStatus::NotInstalled,
            Some(installed) if installed == &expected.version => VersionStatus::UpToDate,
            Some(installed) => VersionStatus::NeedsUpdate {
                current: installed.clone(),
                expected: expected.version.clone(),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum VersionError {
    #[error("Failed to read/write versions at {0}: {1}")]
    Io(std::path::PathBuf, std::io::Error),

    #[error("Failed to parse versions JSON: {0}")]
    Parse(serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_manifest() -> ResourceManifest {
        let json = r#"{
            "profile": "personal",
            "platform": "darwin-arm64",
            "components": {
                "bun": { "version": "1.2.17" },
                "node": { "version": "22.19.0" },
                "dotnet": { "version": "excluded(platform: darwin)" }
            },
            "artifacts": {}
        }"#;
        ResourceManifest::from_json(json).unwrap()
    }

    #[test]
    fn not_installed() {
        let installed = InstalledVersions::default();
        let manifest = make_manifest();
        let statuses = installed.check_all(&manifest);

        assert_eq!(statuses["bun"], VersionStatus::NotInstalled);
        assert_eq!(statuses["node"], VersionStatus::NotInstalled);
        assert_eq!(
            statuses["dotnet"],
            VersionStatus::Excluded("excluded(platform: darwin)".into())
        );
    }

    #[test]
    fn up_to_date() {
        let mut installed = InstalledVersions::default();
        installed.set_version("bun", "1.2.17");
        installed.set_version("node", "22.19.0");

        let manifest = make_manifest();
        let statuses = installed.check_all(&manifest);

        assert_eq!(statuses["bun"], VersionStatus::UpToDate);
        assert_eq!(statuses["node"], VersionStatus::UpToDate);
    }

    #[test]
    fn needs_update() {
        let mut installed = InstalledVersions::default();
        installed.set_version("bun", "1.0.0");

        let manifest = make_manifest();
        let statuses = installed.check_all(&manifest);

        assert_eq!(
            statuses["bun"],
            VersionStatus::NeedsUpdate {
                current: "1.0.0".into(),
                expected: "1.2.17".into(),
            }
        );
    }

    #[test]
    fn roundtrip_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("versions.json");

        let mut v = InstalledVersions::default();
        v.set_version("bun", "1.2.17");
        v.save(&path).unwrap();

        let loaded = InstalledVersions::from_file(&path).unwrap();
        assert_eq!(loaded.components["bun"], "1.2.17");
    }

    #[test]
    fn missing_file_returns_default() {
        let path = Path::new("/tmp/nonexistent_srow_test_versions.json");
        let v = InstalledVersions::from_file(path).unwrap();
        assert!(v.components.is_empty());
    }
}
