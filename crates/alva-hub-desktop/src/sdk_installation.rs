// INPUT: Trusted user-home root, task workspace, managed package relative path and manifest
// OUTPUT: managed_sdk_entry — validated absolute SDK entry, without executing package code
// POS: Product-owned runtime dependency boundary shared by assembly and readiness

use std::path::{Component, Path, PathBuf};

/// Only an explicitly installed user-level runtime is eligible. Never search
/// task node_modules, cwd, NODE_PATH, or execute npm/import during discovery.
pub(crate) fn managed_sdk_entry(
    home: &Path,
    workspace: &Path,
    relative_package: &Path,
    expected_name: &str,
) -> Result<PathBuf, String> {
    let unavailable = || {
        "Managed SDK installation missing or unsafe; install the SDK in the Hub runtime directory"
            .to_string()
    };
    let home = home.canonicalize().map_err(|_| unavailable())?;
    let workspace = workspace.canonicalize().map_err(|_| unavailable())?;
    let runtime: PathBuf = relative_package
        .components()
        .take_while(|component| component.as_os_str() != "node_modules")
        .collect();
    let runtime = home.join(runtime);
    if runtime.starts_with(&workspace) || workspace.starts_with(&runtime) {
        return Err("SDK runtime must be outside the task workspace".into());
    }
    let owner = std::fs::metadata(&home).map_err(|_| unavailable())?;
    let check = |relative: &Path| -> Result<PathBuf, String> {
        let mut current = home.clone();
        for component in relative.components() {
            let Component::Normal(part) = component else {
                return Err(unavailable());
            };
            current.push(part);
            let metadata = std::fs::symlink_metadata(&current).map_err(|_| unavailable())?;
            if metadata.file_type().is_symlink() {
                return Err(unavailable());
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if metadata.uid() != owner.uid() || metadata.mode() & 0o022 != 0 {
                    return Err(unavailable());
                }
            }
        }
        Ok(current)
    };
    let manifest_path = check(&relative_package.join("package.json"))?;
    let metadata = std::fs::metadata(&manifest_path).map_err(|_| unavailable())?;
    if !metadata.is_file() || metadata.len() > 128 * 1024 {
        return Err(unavailable());
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(manifest_path).map_err(|_| unavailable())?)
            .map_err(|_| unavailable())?;
    if manifest["name"].as_str() != Some(expected_name)
        || !manifest["version"].as_str().is_some_and(|version| {
            let core = version.split(['-', '+']).next().unwrap_or_default();
            let parts: Vec<_> = core.split('.').collect();
            parts.len() == 3
                && parts
                    .iter()
                    .all(|part| !part.is_empty() && part.bytes().all(|c| c.is_ascii_digit()))
        })
    {
        return Err("Managed SDK package identity or version is invalid".into());
    }
    fn import_entry(value: &serde_json::Value) -> Option<&str> {
        value.as_str().or_else(|| {
            ["node", "import", "default"]
                .iter()
                .find_map(|key| value.get(*key).and_then(import_entry))
        })
    }
    let entry = if let Some(exports) = manifest.get("exports") {
        import_entry(exports.get(".").unwrap_or(exports))
    } else {
        manifest.get("main").and_then(|v| v.as_str())
    }
    .ok_or_else(unavailable)?;
    let entry = entry.strip_prefix("./").unwrap_or(entry);
    let entry_path = Path::new(entry);
    if entry_path
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(unavailable());
    }
    let entry = check(&relative_package.join(entry_path))?;
    if !entry.is_file() {
        return Err(unavailable());
    }
    Ok(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installation(home: &Path) -> PathBuf {
        let relative = PathBuf::from(".local/share/hub/runtime/node_modules/test-sdk");
        std::fs::create_dir_all(home.join(&relative)).unwrap();
        std::fs::write(
            home.join(&relative).join("package.json"),
            r#"{"name":"test-sdk","version":"1.2.3","exports":{".":{"import":"./entry.mjs"}}}"#,
        )
        .unwrap();
        std::fs::write(
            home.join(&relative).join("entry.mjs"),
            "throw new Error('must not execute in readiness');",
        )
        .unwrap();
        relative
    }

    #[test]
    fn resolves_only_managed_installation_without_executing_it() {
        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let relative = installation(home.path());
        assert_eq!(
            managed_sdk_entry(home.path(), workspace.path(), &relative, "test-sdk").unwrap(),
            home.path()
                .canonicalize()
                .unwrap()
                .join(relative)
                .join("entry.mjs")
        );
    }

    #[test]
    fn project_package_cannot_replace_missing_runtime_or_own_the_runtime() {
        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let relative = installation(workspace.path());
        assert!(managed_sdk_entry(home.path(), workspace.path(), &relative, "test-sdk").is_err());
        let relative = installation(home.path());
        assert!(managed_sdk_entry(home.path(), home.path(), &relative, "test-sdk").is_err());
        assert!(managed_sdk_entry(home.path(), workspace.path(), &relative, "wrong-sdk").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_linked_entry_and_group_writable_runtime() {
        use std::os::unix::{fs::symlink, fs::PermissionsExt};
        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let relative = installation(home.path());
        let entry = home.path().join(&relative).join("entry.mjs");
        std::fs::remove_file(&entry).unwrap();
        std::fs::write(
            workspace.path().join("fake.mjs"),
            "export const query = () => {};",
        )
        .unwrap();
        symlink(workspace.path().join("fake.mjs"), &entry).unwrap();
        assert!(managed_sdk_entry(home.path(), workspace.path(), &relative, "test-sdk").is_err());
        std::fs::remove_file(&entry).unwrap();
        std::fs::write(&entry, "").unwrap();
        std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o664)).unwrap();
        assert!(managed_sdk_entry(home.path(), workspace.path(), &relative, "test-sdk").is_err());
    }
}
