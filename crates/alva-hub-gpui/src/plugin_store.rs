// INPUT:  ALVA extension roots, plugin package folders, loader manifest schema
// OUTPUT: Installed plugin catalog plus safe install and enable/disable operations
// POS:    Hub plugin management plane shared by the catalog UI and ALVA loader

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use alva_app_core::AlvaPaths;
use alva_app_extension_loader::{
    loader::DISABLED_MARKER,
    manifest::{PluginManifest, Runtime},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginScope {
    Project,
    Global,
}

impl PluginScope {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Project => "Project",
            Self::Global => "Global",
        }
    }
}

#[derive(Debug, Clone)]
pub struct InstalledPlugin {
    pub dir: PathBuf,
    pub manifest: PluginManifest,
    pub scope: PluginScope,
    pub enabled: bool,
}

impl InstalledPlugin {
    pub const fn runtime_label(&self) -> &'static str {
        match self.manifest.runtime {
            Runtime::Python => "Python",
            Runtime::Javascript => "JavaScript",
        }
    }
}

#[derive(Clone)]
pub struct PluginStore {
    project_dir: PathBuf,
    global_dir: PathBuf,
}

impl PluginStore {
    pub fn new(workspace: &Path) -> Self {
        let paths = AlvaPaths::new(workspace);
        Self {
            project_dir: paths.project_extensions_dir(),
            global_dir: paths.global_extensions_dir(),
        }
    }

    pub fn scan(&self) -> Result<Vec<InstalledPlugin>, String> {
        let mut plugins = Vec::new();
        let mut seen = HashSet::new();
        self.scan_root(
            &self.project_dir,
            PluginScope::Project,
            &mut seen,
            &mut plugins,
        )?;
        self.scan_root(
            &self.global_dir,
            PluginScope::Global,
            &mut seen,
            &mut plugins,
        )?;
        Ok(plugins)
    }

    pub fn install(&self, source: &Path) -> Result<InstalledPlugin, String> {
        let source = source
            .canonicalize()
            .map_err(|error| format!("open plugin folder: {error}"))?;
        if !source.is_dir() {
            return Err("The selected plugin path is not a directory.".into());
        }
        let manifest = read_manifest(&source)?;
        validate_manifest(&source, &manifest)?;
        validate_directory_name(&manifest.name)?;
        fs::create_dir_all(&self.project_dir)
            .map_err(|error| format!("create project plugin directory: {error}"))?;
        let destination = self.project_dir.join(&manifest.name);
        if destination.exists() {
            return Err(format!(
                "A project plugin named '{}' is already installed.",
                manifest.name
            ));
        }
        let temporary = self
            .project_dir
            .join(format!(".install-{}", uuid::Uuid::new_v4()));
        if let Err(error) = copy_directory(&source, &temporary) {
            let _ = fs::remove_dir_all(&temporary);
            return Err(error);
        }
        let _ = fs::remove_file(temporary.join(DISABLED_MARKER));
        fs::rename(&temporary, &destination)
            .map_err(|error| format!("finish plugin installation: {error}"))?;
        Ok(InstalledPlugin {
            dir: destination,
            manifest,
            scope: PluginScope::Project,
            enabled: true,
        })
    }

    pub fn set_enabled(&self, plugin: &InstalledPlugin, enabled: bool) -> Result<(), String> {
        self.verify_managed_path(&plugin.dir)?;
        let marker = plugin.dir.join(DISABLED_MARKER);
        if enabled {
            if marker.exists() {
                fs::remove_file(&marker).map_err(|error| {
                    format!("enable plugin '{}': {error}", plugin.manifest.name)
                })?;
            }
        } else {
            let mut options = OpenOptions::new();
            options.create(true).truncate(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt as _;
                options.mode(0o600);
            }
            options
                .open(&marker)
                .map_err(|error| format!("disable plugin '{}': {error}", plugin.manifest.name))?;
        }
        Ok(())
    }

    fn scan_root(
        &self,
        root: &Path,
        scope: PluginScope,
        seen: &mut HashSet<String>,
        output: &mut Vec<InstalledPlugin>,
    ) -> Result<(), String> {
        if !root.exists() {
            return Ok(());
        }
        let mut entries = fs::read_dir(root)
            .map_err(|error| format!("scan plugin directory '{}': {error}", root.display()))?
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let dir = entry.path();
            let Ok(manifest) = read_manifest(&dir) else {
                continue;
            };
            if !seen.insert(manifest.name.clone()) {
                continue;
            }
            output.push(InstalledPlugin {
                enabled: !dir.join(DISABLED_MARKER).is_file(),
                dir,
                manifest,
                scope,
            });
        }
        Ok(())
    }

    fn verify_managed_path(&self, plugin_dir: &Path) -> Result<(), String> {
        let plugin_dir = plugin_dir
            .canonicalize()
            .map_err(|error| format!("open installed plugin: {error}"))?;
        let managed = [&self.project_dir, &self.global_dir]
            .into_iter()
            .filter_map(|root| root.canonicalize().ok())
            .any(|root| plugin_dir.starts_with(root));
        if managed {
            Ok(())
        } else {
            Err("Refusing to change a plugin outside ALVA extension directories.".into())
        }
    }
}

fn read_manifest(plugin_dir: &Path) -> Result<PluginManifest, String> {
    let path = plugin_dir.join("alva.toml");
    let source =
        fs::read_to_string(&path).map_err(|error| format!("read '{}': {error}", path.display()))?;
    toml::from_str(&source).map_err(|error| format!("parse '{}': {error}", path.display()))
}

fn validate_manifest(plugin_dir: &Path, manifest: &PluginManifest) -> Result<(), String> {
    if manifest.name.trim().is_empty() || manifest.version.trim().is_empty() {
        return Err("Plugin name and version are required.".into());
    }
    let entry = plugin_dir.join(&manifest.entry);
    let canonical = entry
        .canonicalize()
        .map_err(|error| format!("open plugin entry '{}': {error}", entry.display()))?;
    if !canonical.starts_with(plugin_dir) || !canonical.is_file() {
        return Err("Plugin entry must be a file inside the selected folder.".into());
    }
    Ok(())
}

fn validate_directory_name(name: &str) -> Result<(), String> {
    let valid = !name.is_empty()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'));
    if valid {
        Ok(())
    } else {
        Err("Plugin name may only contain letters, digits, '-' and '_'.".into())
    }
}

fn copy_directory(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir(destination)
        .map_err(|error| format!("create plugin install directory: {error}"))?;
    for entry in fs::read_dir(source)
        .map_err(|error| format!("read plugin package '{}': {error}", source.display()))?
    {
        let entry = entry.map_err(|error| format!("read plugin package entry: {error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("inspect plugin package entry: {error}"))?;
        let target = destination.join(entry.file_name());
        if file_type.is_symlink() {
            return Err("Plugin packages containing symbolic links are not accepted.".into());
        }
        if file_type.is_dir() {
            copy_directory(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), target)
                .map_err(|error| format!("copy plugin package file: {error}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_plugin(root: &Path, name: &str) -> PathBuf {
        let plugin = root.join(name);
        fs::create_dir_all(&plugin).unwrap();
        fs::write(plugin.join("main.py"), "print('ok')").unwrap();
        fs::write(
            plugin.join("alva.toml"),
            format!(
                "name = \"{name}\"\nversion = \"0.1.0\"\nruntime = \"python\"\nentry = \"main.py\"\n"
            ),
        )
        .unwrap();
        plugin
    }

    #[test]
    fn project_plugin_shadows_global_and_toggle_is_persistent() {
        let root = tempfile::tempdir().unwrap();
        let store = PluginStore {
            project_dir: root.path().join("project"),
            global_dir: root.path().join("global"),
        };
        write_plugin(&store.project_dir, "reviewer");
        write_plugin(&store.global_dir, "reviewer");

        let plugins = store.scan().unwrap();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].scope, PluginScope::Project);
        store.set_enabled(&plugins[0], false).unwrap();
        assert!(!store.scan().unwrap()[0].enabled);
        store.set_enabled(&plugins[0], true).unwrap();
        assert!(store.scan().unwrap()[0].enabled);
    }

    #[test]
    fn install_copies_a_valid_package_into_project_scope() {
        let root = tempfile::tempdir().unwrap();
        let source_root = root.path().join("source");
        let source = write_plugin(&source_root, "memory-helper");
        let store = PluginStore {
            project_dir: root.path().join("project"),
            global_dir: root.path().join("global"),
        };

        let installed = store.install(&source).unwrap();
        assert_eq!(installed.manifest.name, "memory-helper");
        assert!(installed.dir.join("alva.toml").is_file());
        assert_eq!(store.scan().unwrap().len(), 1);
    }
}
