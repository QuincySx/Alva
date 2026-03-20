//! Path resolver — locate executable files for each runtime component.
//!
//! Given an installed environment, the resolver knows where each runtime's
//! binary lives relative to the component directory.

use std::path::PathBuf;

use super::config::EnvironmentConfig;

/// Resolves paths to runtime executables within the environment.
pub struct RuntimeResolver {
    config: EnvironmentConfig,
}

impl RuntimeResolver {
    pub fn new(config: EnvironmentConfig) -> Self {
        Self { config }
    }

    /// Resolve the Bun executable path.
    pub fn resolve_bun(&self) -> Option<PathBuf> {
        self.find_executable("bun", &Self::bun_candidates())
    }

    /// Resolve the Node.js executable path.
    pub fn resolve_node(&self) -> Option<PathBuf> {
        self.find_executable("node", &Self::node_candidates())
    }

    /// Resolve the Python executable path.
    pub fn resolve_python(&self) -> Option<PathBuf> {
        self.find_executable("python", &Self::python_candidates())
    }

    /// Resolve the uv (Python package manager) executable path.
    pub fn resolve_uv(&self) -> Option<PathBuf> {
        self.find_executable("uv", &Self::uv_candidates())
    }

    /// Resolve the Chromium executable path.
    pub fn resolve_chromium(&self) -> Option<PathBuf> {
        self.find_executable("chromium", &Self::chromium_candidates())
    }

    /// Resolve the Qwen executable path.
    pub fn resolve_qwen(&self) -> Option<PathBuf> {
        self.find_executable("qwen", &Self::qwen_candidates())
    }

    /// Resolve an arbitrary component by name with custom candidate paths.
    pub fn resolve_component(&self, name: &str, candidates: &[&str]) -> Option<PathBuf> {
        self.find_executable(name, candidates)
    }

    // -- Candidate paths per component (platform-specific) -------------------

    fn bun_candidates() -> Vec<&'static str> {
        if cfg!(target_os = "windows") {
            vec!["bun.exe"]
        } else {
            vec!["bun"]
        }
    }

    fn node_candidates() -> Vec<&'static str> {
        if cfg!(target_os = "windows") {
            vec!["bin/node.exe", "node.exe"]
        } else {
            vec!["bin/node", "node"]
        }
    }

    fn python_candidates() -> Vec<&'static str> {
        if cfg!(target_os = "windows") {
            vec!["python.exe", "bin/python.exe", "Scripts/python.exe"]
        } else {
            vec!["bin/python3", "bin/python", "python3", "python"]
        }
    }

    fn uv_candidates() -> Vec<&'static str> {
        if cfg!(target_os = "windows") {
            vec!["uv.exe"]
        } else {
            vec!["uv"]
        }
    }

    fn chromium_candidates() -> Vec<&'static str> {
        if cfg!(target_os = "macos") {
            vec![
                "Chromium.app/Contents/MacOS/Chromium",
                "chrome-mac/Chromium.app/Contents/MacOS/Chromium",
                "chromium",
            ]
        } else if cfg!(target_os = "windows") {
            vec!["chrome.exe", "chrome-win/chrome.exe"]
        } else {
            vec!["chrome", "chromium", "chrome-linux/chrome"]
        }
    }

    fn qwen_candidates() -> Vec<&'static str> {
        if cfg!(target_os = "windows") {
            vec!["qwen.cmd", "bin/qwen.cmd", "node_modules/.bin/qwen.cmd"]
        } else {
            vec!["qwen", "bin/qwen", "node_modules/.bin/qwen"]
        }
    }

    // -- Internal helpers ----------------------------------------------------

    /// Search for an executable in the component directory, trying each
    /// candidate relative path in order.  Returns the first path that exists.
    fn find_executable(&self, component: &str, candidates: &[&str]) -> Option<PathBuf> {
        let comp_dir = self.config.component_dir(component);
        if !comp_dir.exists() {
            return None;
        }
        for candidate in candidates {
            let full = comp_dir.join(candidate);
            if full.exists() {
                return Some(full);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn resolve_missing_component() {
        let tmp = tempfile::tempdir().unwrap();
        let config = EnvironmentConfig::new(tmp.path());
        let resolver = RuntimeResolver::new(config);
        assert!(resolver.resolve_bun().is_none());
    }

    #[test]
    fn resolve_existing_component() {
        let tmp = tempfile::tempdir().unwrap();
        let config = EnvironmentConfig::new(tmp.path());

        // Simulate installed bun
        let bun_dir = tmp.path().join("bun");
        fs::create_dir_all(&bun_dir).unwrap();
        let bun_exe = bun_dir.join("bun");
        fs::write(&bun_exe, "#!/bin/sh\n").unwrap();

        let resolver = RuntimeResolver::new(config);
        let resolved = resolver.resolve_bun();
        assert!(resolved.is_some());
        assert_eq!(resolved.unwrap(), bun_exe);
    }

    #[test]
    fn resolve_node_with_bin_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let config = EnvironmentConfig::new(tmp.path());

        // Simulate installed node in bin/ subdirectory
        let node_bin_dir = tmp.path().join("node").join("bin");
        fs::create_dir_all(&node_bin_dir).unwrap();
        let node_exe = node_bin_dir.join("node");
        fs::write(&node_exe, "#!/bin/sh\n").unwrap();

        let resolver = RuntimeResolver::new(config);
        let resolved = resolver.resolve_node();
        assert!(resolved.is_some());
        assert_eq!(resolved.unwrap(), node_exe);
    }

    #[test]
    fn resolve_custom_component() {
        let tmp = tempfile::tempdir().unwrap();
        let config = EnvironmentConfig::new(tmp.path());

        let custom_dir = tmp.path().join("my-tool");
        fs::create_dir_all(&custom_dir).unwrap();
        let bin = custom_dir.join("run.sh");
        fs::write(&bin, "#!/bin/sh\n").unwrap();

        let resolver = RuntimeResolver::new(config);
        let resolved = resolver.resolve_component("my-tool", &["run.sh", "run"]);
        assert!(resolved.is_some());
        assert_eq!(resolved.unwrap(), bin);
    }
}
