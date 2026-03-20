use regex::Regex;
use std::path::{Path, PathBuf};

/// Filters out access to sensitive paths such as secrets, certificates, and
/// private configuration directories.
///
/// Modeled after Wukong's `sensitive_paths.rs`:
///   - Hard-denied directories   (~/.gnupg, ~/.kube, ~/.ssh, internal dirs)
///   - Denied file extensions    (.pem, .key, .p12, .pfx, .jks, .keystore)
///   - Denied filenames          (.env, .env.local, .env.production, ...)
///   - Denied regex patterns     (catch-all for edge cases)
pub struct SensitivePathFilter {
    denied_dirs: Vec<PathBuf>,
    denied_extensions: Vec<String>,
    denied_filenames: Vec<String>,
    denied_patterns: Vec<Regex>,
}

impl SensitivePathFilter {
    /// Create with the default Wukong-derived ruleset.
    pub fn default_rules() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));

        let denied_dirs = vec![
            home.join(".gnupg"),
            home.join(".ssh"),
            home.join(".kube"),
            home.join(".aws"),
            home.join(".azure"),
            home.join(".gcloud"),
            home.join(".docker"),
            home.join(".real").join(".acp"), // Wukong internal
        ];

        let denied_extensions = vec![
            ".pem", ".key", ".p12", ".pfx", ".jks", ".keystore", ".cer", ".crt",
        ]
        .into_iter()
        .map(String::from)
        .collect();

        let denied_filenames = vec![
            ".env",
            ".env.local",
            ".env.development",
            ".env.production",
            ".env.staging",
            ".env.test",
            ".npmrc",         // may contain auth tokens
            ".pypirc",        // may contain auth tokens
            "credentials.json",
            "service-account.json",
        ]
        .into_iter()
        .map(String::from)
        .collect();

        // Regex patterns for more complex matching
        let denied_patterns = vec![
            Regex::new(r"\.env\.[a-zA-Z0-9_-]+$").expect("invalid regex"),         // .env.*
            Regex::new(r"id_[a-z]+$").expect("invalid regex"),                      // id_rsa, id_ed25519
            Regex::new(r"id_[a-z]+\.pub$").expect("invalid regex"),                 // public keys
            Regex::new(r"(?i)secret[s]?\.(?:ya?ml|json|toml)$").expect("invalid regex"),
        ];

        Self {
            denied_dirs,
            denied_extensions,
            denied_filenames,
            denied_patterns,
        }
    }

    /// Check whether `path` is sensitive. Returns `Some(reason)` if blocked.
    pub fn check(&self, path: &Path) -> Option<String> {
        // Canonicalize to resolve symlinks and relative components.
        // If canonicalization fails (file doesn't exist yet), normalize manually.
        let resolved = path
            .canonicalize()
            .unwrap_or_else(|_| self.normalize(path));

        // 1. Directory check — is path inside (or equal to) a denied directory?
        for dir in &self.denied_dirs {
            if resolved.starts_with(dir) {
                return Some(format!(
                    "path is inside denied directory: {}",
                    dir.display()
                ));
            }
        }

        // 2. Filename check
        if let Some(name) = resolved.file_name().and_then(|n| n.to_str()) {
            let name_lower = name.to_lowercase();
            for denied in &self.denied_filenames {
                if name_lower == denied.to_lowercase() {
                    return Some(format!("denied filename: {}", denied));
                }
            }
        }

        // 3. Extension check
        if let Some(name) = resolved.file_name().and_then(|n| n.to_str()) {
            let name_lower = name.to_lowercase();
            for ext in &self.denied_extensions {
                if name_lower.ends_with(ext) {
                    return Some(format!("denied extension: {}", ext));
                }
            }
        }

        // 4. Regex pattern check
        if let Some(name) = resolved.file_name().and_then(|n| n.to_str()) {
            for pat in &self.denied_patterns {
                if pat.is_match(name) {
                    return Some(format!("matches denied pattern: {}", pat.as_str()));
                }
            }
        }

        None
    }

    /// Best-effort normalization for paths that may not exist yet.
    fn normalize(&self, path: &Path) -> PathBuf {
        let mut out = PathBuf::new();
        for component in path.components() {
            match component {
                std::path::Component::ParentDir => {
                    out.pop();
                }
                std::path::Component::CurDir => {}
                other => out.push(other),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_env_files() {
        let filter = SensitivePathFilter::default_rules();
        assert!(filter.check(Path::new("/project/.env")).is_some());
        assert!(filter.check(Path::new("/project/.env.local")).is_some());
        assert!(filter.check(Path::new("/project/.env.production")).is_some());
    }

    #[test]
    fn blocks_certificate_files() {
        let filter = SensitivePathFilter::default_rules();
        assert!(filter.check(Path::new("/project/server.pem")).is_some());
        assert!(filter.check(Path::new("/project/server.key")).is_some());
        assert!(filter.check(Path::new("/project/cert.p12")).is_some());
        assert!(filter.check(Path::new("/project/keystore.pfx")).is_some());
    }

    #[test]
    fn blocks_gnupg_directory() {
        let filter = SensitivePathFilter::default_rules();
        let home = dirs::home_dir().unwrap();
        let path = home.join(".gnupg").join("pubring.kbx");
        assert!(filter.check(&path).is_some());
    }

    #[test]
    fn blocks_ssh_directory() {
        let filter = SensitivePathFilter::default_rules();
        let home = dirs::home_dir().unwrap();
        let path = home.join(".ssh").join("config");
        assert!(filter.check(&path).is_some());
    }

    #[test]
    fn allows_normal_files() {
        let filter = SensitivePathFilter::default_rules();
        assert!(filter.check(Path::new("/project/src/main.rs")).is_none());
        assert!(filter.check(Path::new("/project/Cargo.toml")).is_none());
        assert!(filter.check(Path::new("/project/README.md")).is_none());
    }

    #[test]
    fn blocks_env_variant_via_regex() {
        let filter = SensitivePathFilter::default_rules();
        assert!(filter.check(Path::new("/project/.env.custom")).is_some());
    }
}
