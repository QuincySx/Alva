// INPUT:  Hub application-support directory and custom provider records
// OUTPUT: Owner-only atomic persistence for provider metadata and API credentials
// POS:    Product settings store; secrets never enter SQLite, logs, or process arguments

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::models::StoredCustomProvider;

#[derive(Clone)]
pub struct ProviderStore {
    path: PathBuf,
}

impl ProviderStore {
    pub fn new(application_support: &Path) -> Self {
        Self {
            path: application_support.join("model-providers.json"),
        }
    }

    pub fn load(&self) -> Result<Vec<StoredCustomProvider>, String> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let bytes = fs::read(&self.path)
            .map_err(|error| format!("read custom model providers: {error}"))?;
        serde_json::from_slice(&bytes)
            .map_err(|error| format!("decode custom model providers: {error}"))
    }

    pub fn save(&self, providers: &[StoredCustomProvider]) -> Result<(), String> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| "provider store has no parent directory".to_string())?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("create provider settings directory: {error}"))?;
        set_owner_only_directory(parent)?;

        let temporary = self.path.with_extension("json.tmp");
        let payload = serde_json::to_vec_pretty(providers)
            .map_err(|error| format!("encode custom model providers: {error}"))?;
        let mut file = owner_only_file(&temporary)?;
        file.write_all(&payload)
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("write custom model providers: {error}"))?;
        fs::rename(&temporary, &self.path)
            .map_err(|error| format!("replace custom model providers: {error}"))?;
        set_owner_only_file(&self.path)
    }
}

fn owner_only_file(path: &Path) -> Result<std::fs::File, String> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(|error| format!("open custom model provider store: {error}"))
}

#[cfg(unix)]
fn set_owner_only_directory(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("protect provider settings directory: {error}"))
}

#[cfg(not(unix))]
fn set_owner_only_directory(_: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn set_owner_only_file(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("protect custom model provider store: {error}"))
}

#[cfg(not(unix))]
fn set_owner_only_file(_: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        AgentRuntime, ApiProtocol, CustomModelProfile, CustomProviderProfile, ReasoningEffort,
    };

    #[test]
    fn provider_records_round_trip_without_sqlite() {
        let root = tempfile::tempdir().unwrap();
        let store = ProviderStore::new(root.path());
        let providers = vec![StoredCustomProvider {
            profile: CustomProviderProfile {
                provider_id: "acme".into(),
                display_name: "Acme".into(),
                protocol: ApiProtocol::OpenAiResponses,
                base_url: "https://example.invalid/v1".into(),
                runtimes: vec![AgentRuntime::Alva, AgentRuntime::Codex],
                models: vec![CustomModelProfile {
                    model_id: "reasoner".into(),
                    display_name: "Reasoner".into(),
                    context_window: 128_000,
                    max_output_tokens: 16_000,
                    images: true,
                    tools: true,
                    reasoning_efforts: vec![ReasoningEffort::High],
                }],
            },
            api_key: "test-secret".into(),
        }];

        store.save(&providers).unwrap();
        assert_eq!(store.load().unwrap(), providers);
    }
}
