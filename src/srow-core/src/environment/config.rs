//! Environment configuration — paths and platform detection.

use std::path::PathBuf;

/// Central configuration for the environment manager.
///
/// All paths are derived from a single `base_dir` (typically `~/.srow/environment/`).
#[derive(Debug, Clone)]
pub struct EnvironmentConfig {
    /// Root directory for the environment, e.g. `~/.srow/environment/`.
    pub base_dir: PathBuf,
}

impl EnvironmentConfig {
    /// Create a new config rooted at the given directory.
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }

    /// Default config using the platform data directory:
    ///   - macOS: `~/Library/Application Support/com.smallraw.app.srow-agent/environment/`
    ///   - Windows: `%APPDATA%/com.smallraw.app.srow-agent/environment/`
    pub fn default_config() -> Option<Self> {
        dirs::data_dir().map(|d| {
            Self::new(
                d.join("com.smallraw.app.srow-agent")
                    .join("environment"),
            )
        })
    }

    // -- Derived paths -------------------------------------------------------

    /// Path to `resource_manifest.json`.
    pub fn manifest_path(&self) -> PathBuf {
        self.base_dir.join("resource_manifest.json")
    }

    /// Path to `versions.json` (installed version tracking).
    pub fn versions_path(&self) -> PathBuf {
        self.base_dir.join("versions.json")
    }

    /// Directory holding bundled packages (zip/tar.gz archives).
    pub fn packages_dir(&self) -> PathBuf {
        self.base_dir.join("packages")
    }

    /// Installation directory for a specific component.
    pub fn component_dir(&self, component: &str) -> PathBuf {
        self.base_dir.join(component)
    }
}

// ---------------------------------------------------------------------------
// Platform detection
// ---------------------------------------------------------------------------

/// Detect the current platform string, e.g. `"darwin-arm64"`, `"windows-x64"`.
pub fn detect_platform() -> String {
    let os = if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    };

    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_arch = "x86_64") {
        "x64"
    } else {
        "unknown"
    };

    format!("{os}-{arch}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn derived_paths() {
        let cfg = EnvironmentConfig::new("/tmp/srow-env-test");
        assert_eq!(
            cfg.manifest_path(),
            Path::new("/tmp/srow-env-test/resource_manifest.json")
        );
        assert_eq!(
            cfg.versions_path(),
            Path::new("/tmp/srow-env-test/versions.json")
        );
        assert_eq!(
            cfg.packages_dir(),
            Path::new("/tmp/srow-env-test/packages")
        );
        assert_eq!(
            cfg.component_dir("bun"),
            Path::new("/tmp/srow-env-test/bun")
        );
    }

    #[test]
    fn platform_detection_non_empty() {
        let platform = detect_platform();
        assert!(!platform.is_empty());
        // On macOS ARM this would be "darwin-arm64"
        assert!(platform.contains('-'));
    }
}
