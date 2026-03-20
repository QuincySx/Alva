//! Environment runtime management — Sub-8
//!
//! Manages embedded runtime components (Bun, Node.js, Python, uv, Chromium, Qwen).
//!
//! ## Architecture
//!
//! ```text
//! EnvironmentManager
//!   ├── ResourceManifest   (manifest.rs)  — expected versions & artifact configs
//!   ├── InstalledVersions  (versions.rs)  — currently installed versions
//!   ├── Installer          (installer.rs) — extract/download/verify
//!   ├── RuntimeResolver    (resolver.rs)  — find executable paths
//!   └── EnvironmentConfig  (config.rs)    — base directory & platform
//! ```
//!
//! ## Flow
//!
//! ```text
//! ensure_ready()
//!   → load resource_manifest.json
//!   → compare with versions.json
//!   → for each NeedsUpdate / NotInstalled:
//!       → if bundled archive exists → extract
//!       → if URL provided → download → extract  (placeholder)
//!       → update versions.json
//! ```

pub mod config;
pub mod installer;
pub mod manifest;
pub mod resolver;
pub mod versions;

use std::collections::HashMap;
use std::path::PathBuf;

use tracing::info;

use config::EnvironmentConfig;
use installer::Installer;
use manifest::ResourceManifest;
use resolver::RuntimeResolver;
use versions::{InstalledVersions, VersionStatus};

/// Central manager for the embedded runtime environment.
///
/// Coordinates manifest loading, version checking, installation, and path
/// resolution for all bundled runtime components.
pub struct EnvironmentManager {
    config: EnvironmentConfig,
    manifest: ResourceManifest,
    installed: InstalledVersions,
}

/// Aggregate error type for environment operations.
#[derive(Debug, thiserror::Error)]
pub enum EnvironmentError {
    #[error("Manifest error: {0}")]
    Manifest(#[from] manifest::ManifestError),

    #[error("Version error: {0}")]
    Version(#[from] versions::VersionError),

    #[error("Installer error: {0}")]
    Installer(#[from] installer::InstallerError),

    #[error("No manifest found at {0}")]
    ManifestNotFound(PathBuf),

    #[error("{0}")]
    Other(String),
}

impl EnvironmentManager {
    /// Create a new manager by loading the manifest and installed versions
    /// from the given configuration.
    pub fn new(config: EnvironmentConfig) -> Result<Self, EnvironmentError> {
        let manifest_path = config.manifest_path();
        if !manifest_path.exists() {
            return Err(EnvironmentError::ManifestNotFound(manifest_path));
        }

        let manifest = ResourceManifest::from_file(&manifest_path)?;
        let installed = InstalledVersions::from_file(&config.versions_path())?;

        Ok(Self {
            config,
            manifest,
            installed,
        })
    }

    /// Create a manager from an in-memory manifest (useful for testing or
    /// when the manifest is embedded in the binary).
    pub fn from_manifest(
        config: EnvironmentConfig,
        manifest: ResourceManifest,
    ) -> Result<Self, EnvironmentError> {
        let installed = InstalledVersions::from_file(&config.versions_path())?;
        Ok(Self {
            config,
            manifest,
            installed,
        })
    }

    // -- Version checking ----------------------------------------------------

    /// Check the status of all components.
    pub fn check_versions(&self) -> HashMap<String, VersionStatus> {
        self.installed.check_all(&self.manifest)
    }

    /// Check whether the environment is fully up-to-date.
    pub fn is_ready(&self) -> bool {
        self.check_versions().values().all(|s| {
            matches!(s, VersionStatus::UpToDate | VersionStatus::Excluded(_))
        })
    }

    // -- Installation --------------------------------------------------------

    /// Ensure all non-excluded components are installed and up-to-date.
    ///
    /// This is the main entry point called at application startup.
    pub async fn ensure_ready(&mut self) -> Result<(), EnvironmentError> {
        let statuses = self.installed.check_all(&self.manifest);
        let installer = Installer::new(self.config.clone());

        for (name, status) in &statuses {
            match status {
                VersionStatus::UpToDate => {
                    info!(component = name.as_str(), "Already up-to-date");
                }
                VersionStatus::Excluded(reason) => {
                    info!(component = name.as_str(), reason = reason.as_str(), "Excluded");
                }
                VersionStatus::NotInstalled => {
                    info!(component = name.as_str(), "Not installed — installing");
                    self.install_one(&installer, name).await?;
                }
                VersionStatus::NeedsUpdate { current, expected } => {
                    info!(
                        component = name.as_str(),
                        current = current.as_str(),
                        expected = expected.as_str(),
                        "Version changed — updating"
                    );
                    self.install_one(&installer, name).await?;
                }
            }
        }

        Ok(())
    }

    /// Install/update a single component.
    pub async fn install_component(&mut self, name: &str) -> Result<(), EnvironmentError> {
        let installer = Installer::new(self.config.clone());
        self.install_one(&installer, name).await
    }

    /// Internal: install one component via the installer.
    async fn install_one(
        &mut self,
        installer: &Installer,
        name: &str,
    ) -> Result<(), EnvironmentError> {
        let version_entry = self.manifest.components.get(name).ok_or_else(|| {
            EnvironmentError::Other(format!("Component '{name}' not in manifest"))
        })?;

        if version_entry.is_excluded() {
            return Ok(());
        }

        let artifact = self.manifest.artifacts.get(name).ok_or_else(|| {
            EnvironmentError::Other(format!("No artifact config for component '{name}'"))
        })?;

        installer
            .install_component(name, &version_entry.version, artifact, &mut self.installed)
            .await?;

        Ok(())
    }

    // -- Path resolution -----------------------------------------------------

    /// Get a resolver for looking up runtime executable paths.
    pub fn resolver(&self) -> RuntimeResolver {
        RuntimeResolver::new(self.config.clone())
    }

    /// Shorthand: resolve the Bun executable path.
    pub fn resolve_bun(&self) -> Option<PathBuf> {
        self.resolver().resolve_bun()
    }

    /// Shorthand: resolve the Node.js executable path.
    pub fn resolve_node(&self) -> Option<PathBuf> {
        self.resolver().resolve_node()
    }

    /// Shorthand: resolve the Python executable path.
    pub fn resolve_python(&self) -> Option<PathBuf> {
        self.resolver().resolve_python()
    }

    /// Shorthand: resolve the uv executable path.
    pub fn resolve_uv(&self) -> Option<PathBuf> {
        self.resolver().resolve_uv()
    }

    /// Shorthand: resolve the Chromium executable path.
    pub fn resolve_chromium(&self) -> Option<PathBuf> {
        self.resolver().resolve_chromium()
    }

    /// Shorthand: resolve the Qwen executable path.
    pub fn resolve_qwen(&self) -> Option<PathBuf> {
        self.resolver().resolve_qwen()
    }

    // -- Accessors -----------------------------------------------------------

    /// Access the loaded manifest.
    pub fn manifest(&self) -> &ResourceManifest {
        &self.manifest
    }

    /// Access the currently installed versions.
    pub fn installed_versions(&self) -> &InstalledVersions {
        &self.installed
    }

    /// Access the environment config.
    pub fn config(&self) -> &EnvironmentConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn sample_manifest_json() -> &'static str {
        r#"{
            "profile": "personal",
            "platform": "darwin-arm64",
            "components": {
                "bun": { "version": "1.2.17" },
                "node": { "version": "22.19.0" },
                "dotnet": { "version": "excluded(platform: darwin)" }
            },
            "artifacts": {
                "bun": { "file": "bun.zip", "format": "zip-flat", "url": null },
                "node": { "file": "node.tar.gz", "format": "tar.gz", "url": null }
            }
        }"#
    }

    fn setup_env(dir: &std::path::Path) -> EnvironmentConfig {
        let config = EnvironmentConfig::new(dir);
        fs::write(config.manifest_path(), sample_manifest_json()).unwrap();
        config
    }

    #[test]
    fn load_manager() {
        let tmp = tempfile::tempdir().unwrap();
        let config = setup_env(tmp.path());
        let mgr = EnvironmentManager::new(config).unwrap();

        assert_eq!(mgr.manifest().profile, "personal");
        assert!(!mgr.is_ready()); // nothing installed yet
    }

    #[test]
    fn check_versions_initial() {
        let tmp = tempfile::tempdir().unwrap();
        let config = setup_env(tmp.path());
        let mgr = EnvironmentManager::new(config).unwrap();

        let statuses = mgr.check_versions();
        assert_eq!(statuses["bun"], VersionStatus::NotInstalled);
        assert_eq!(statuses["node"], VersionStatus::NotInstalled);
        assert!(matches!(statuses["dotnet"], VersionStatus::Excluded(_)));
    }

    #[test]
    fn is_ready_when_all_installed() {
        let tmp = tempfile::tempdir().unwrap();
        let config = setup_env(tmp.path());

        // Pre-populate versions.json
        let mut installed = InstalledVersions::default();
        installed.set_version("bun", "1.2.17");
        installed.set_version("node", "22.19.0");
        installed.save(&config.versions_path()).unwrap();

        let mgr = EnvironmentManager::new(config).unwrap();
        assert!(mgr.is_ready());
    }

    #[test]
    fn manifest_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let config = EnvironmentConfig::new(tmp.path());
        let result = EnvironmentManager::new(config);
        assert!(result.is_err());
    }

    #[test]
    fn from_manifest_in_memory() {
        let tmp = tempfile::tempdir().unwrap();
        let config = EnvironmentConfig::new(tmp.path());
        let manifest = ResourceManifest::from_json(sample_manifest_json()).unwrap();
        let mgr = EnvironmentManager::from_manifest(config, manifest).unwrap();
        assert_eq!(mgr.manifest().components.len(), 3);
    }
}
