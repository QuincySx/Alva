// INPUT:  PluginManifest, cap_std directory handles, tempfile, sha2, tokio::task
// OUTPUT: StagedPluginBundle, PluginBundleError, read_manifest
// POS:    Validates and copies an extension into an owner-only bounded snapshot before execution.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use cap_std::ambient_authority;
use cap_std::fs::{Dir as CapDir, File as CapFile, Metadata as CapMetadata};
use sha2::{Digest, Sha256};

use crate::manifest::PluginManifest;

pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
pub const MAX_BUNDLE_FILES: usize = 256;
pub const MAX_BUNDLE_ENTRIES: usize = 512;
pub const MAX_BUNDLE_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_BUNDLE_DEPTH: usize = 16;

/// Owner-only snapshot of one validated plugin bundle.
///
/// The subprocess runs from `root`, never from the user-mutable discovery
/// directory. `TempDir` removes the snapshot after the proxy is dropped.
pub struct StagedPluginBundle {
    _tempdir: tempfile::TempDir,
    root: PathBuf,
    entry: PathBuf,
    content_sha256: String,
}

impl StagedPluginBundle {
    pub async fn stage(
        source_dir: PathBuf,
        expected_manifest: PluginManifest,
    ) -> Result<Self, PluginBundleError> {
        tokio::task::spawn_blocking(move || stage_sync(&source_dir, &expected_manifest))
            .await
            .map_err(|error| PluginBundleError::Task(error.to_string()))?
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn entry(&self) -> &Path {
        &self.entry
    }

    pub fn content_sha256(&self) -> &str {
        &self.content_sha256
    }
}

/// Read and validate `alva.toml` without following a manifest symlink or
/// allocating an attacker-controlled amount of memory.
pub async fn read_manifest(plugin_dir: &Path) -> Result<PluginManifest, PluginBundleError> {
    let manifest_path = plugin_dir.join("alva.toml");
    let path_for_read = manifest_path.clone();
    let bytes =
        tokio::task::spawn_blocking(move || read_source_path(&path_for_read, MAX_MANIFEST_BYTES))
            .await
            .map_err(|error| PluginBundleError::Task(error.to_string()))??;
    parse_manifest(&manifest_path, &bytes)
}

fn stage_sync(
    source_dir: &Path,
    expected_manifest: &PluginManifest,
) -> Result<StagedPluginBundle, PluginBundleError> {
    expected_manifest
        .validate()
        .map_err(PluginBundleError::Manifest)?;
    let source = open_source_directory(source_dir)?;

    let tempdir = tempfile::Builder::new()
        .prefix("alva-plugin-")
        .tempdir()
        .map_err(|source| PluginBundleError::Io {
            path: std::env::temp_dir(),
            source,
        })?;
    let root = tempdir.path().join("bundle");
    create_private_dir(&root)?;

    let mut budget = CopyBudget::default();
    let mut files = Vec::new();
    copy_directory(
        &source,
        source_dir,
        &root,
        Path::new(""),
        0,
        &mut budget,
        &mut files,
    )?;
    files.sort();

    let staged_manifest_path = root.join("alva.toml");
    let staged_manifest_bytes = read_limited_file(&staged_manifest_path, MAX_MANIFEST_BYTES)?;
    let staged_manifest = parse_manifest(&staged_manifest_path, &staged_manifest_bytes)?;
    if &staged_manifest != expected_manifest {
        return Err(PluginBundleError::SourceChanged(
            "manifest changed while the plugin snapshot was being created".to_string(),
        ));
    }

    let content_sha256 = digest_bundle(&root, &files, &staged_manifest)?;
    if let Some(expected) = staged_manifest.content_sha256.as_deref() {
        if !expected.eq_ignore_ascii_case(&content_sha256) {
            return Err(PluginBundleError::DigestMismatch {
                expected: expected.to_string(),
                actual: content_sha256,
            });
        }
    }

    let entry = root.join(&staged_manifest.entry);
    let entry_metadata = fs::symlink_metadata(&entry).map_err(|source| PluginBundleError::Io {
        path: entry.clone(),
        source,
    })?;
    if entry_metadata.file_type().is_symlink() || !entry_metadata.is_file() {
        return Err(PluginBundleError::InvalidBundle(format!(
            "plugin entry must resolve to a regular staged file: {}",
            entry.display()
        )));
    }

    Ok(StagedPluginBundle {
        _tempdir: tempdir,
        root,
        entry,
        content_sha256,
    })
}

#[derive(Default)]
struct CopyBudget {
    entries: usize,
    files: usize,
    bytes: u64,
}

fn open_source_directory(path: &Path) -> Result<CapDir, PluginBundleError> {
    let name = path.file_name().ok_or_else(|| {
        PluginBundleError::InvalidBundle(format!(
            "plugin root must name a directory: {}",
            path.display()
        ))
    })?;
    let parent_path = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = CapDir::open_ambient_dir(parent_path, ambient_authority())
        .map_err(|source| io_error(parent_path, source))?;
    let expected_metadata = parent
        .symlink_metadata(name)
        .map_err(|source| io_error(path, source))?;
    if expected_metadata.file_type().is_symlink() || !expected_metadata.is_dir() {
        return Err(PluginBundleError::InvalidBundle(format!(
            "plugin root must be a regular non-symlink directory: {}",
            path.display()
        )));
    }
    let opened = parent
        .open_dir(name)
        .map_err(|source| io_error(path, source))?;
    let opened_metadata = opened
        .dir_metadata()
        .map_err(|source| io_error(path, source))?;
    if !same_source_file(&expected_metadata, &opened_metadata) {
        return Err(PluginBundleError::SourceChanged(format!(
            "plugin root was replaced while being opened: {}",
            path.display()
        )));
    }
    Ok(opened)
}

fn copy_directory(
    source_dir: &CapDir,
    source_display: &Path,
    target_root: &Path,
    relative: &Path,
    depth: usize,
    budget: &mut CopyBudget,
    files: &mut Vec<PathBuf>,
) -> Result<(), PluginBundleError> {
    if depth > MAX_BUNDLE_DEPTH {
        return Err(PluginBundleError::Limit(format!(
            "plugin directory nesting exceeds {MAX_BUNDLE_DEPTH} levels"
        )));
    }
    let mut entries = Vec::new();
    for entry in source_dir
        .entries()
        .map_err(|source| io_error(source_display, source))?
    {
        budget.entries += 1;
        if budget.entries > MAX_BUNDLE_ENTRIES {
            return Err(PluginBundleError::Limit(format!(
                "plugin bundle exceeds {MAX_BUNDLE_ENTRIES} files/directories"
            )));
        }
        entries.push(entry.map_err(|source| io_error(source_display, source))?);
    }
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let file_name = entry.file_name();
        let child_relative = relative.join(&file_name);
        let source_path = source_display.join(&file_name);
        let target_path = target_root.join(&child_relative);
        let metadata = source_dir
            .symlink_metadata(&file_name)
            .map_err(|source| io_error(&source_path, source))?;
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            return Err(PluginBundleError::InvalidBundle(format!(
                "symbolic links are not allowed in plugin bundles: {}",
                source_path.display()
            )));
        }
        if file_type.is_dir() {
            let child_dir = entry
                .open_dir()
                .map_err(|source| io_error(&source_path, source))?;
            let opened_metadata = child_dir
                .dir_metadata()
                .map_err(|source| io_error(&source_path, source))?;
            if !same_source_file(&metadata, &opened_metadata) {
                return Err(PluginBundleError::SourceChanged(format!(
                    "source directory was replaced while being opened: {}",
                    source_path.display()
                )));
            }
            create_private_dir(&target_path)?;
            copy_directory(
                &child_dir,
                &source_path,
                target_root,
                &child_relative,
                depth + 1,
                budget,
                files,
            )?;
            continue;
        }
        if !file_type.is_file() {
            return Err(PluginBundleError::InvalidBundle(format!(
                "only regular files and directories are allowed in plugin bundles: {}",
                source_path.display()
            )));
        }
        if metadata.len() > MAX_FILE_BYTES {
            return Err(PluginBundleError::Limit(format!(
                "plugin file exceeds {MAX_FILE_BYTES} bytes: {}",
                source_path.display()
            )));
        }
        budget.files += 1;
        budget.bytes = budget.bytes.saturating_add(metadata.len());
        if budget.files > MAX_BUNDLE_FILES || budget.bytes > MAX_BUNDLE_BYTES {
            return Err(PluginBundleError::Limit(format!(
                "plugin bundle exceeds {MAX_BUNDLE_FILES} files or {MAX_BUNDLE_BYTES} bytes"
            )));
        }

        let file = entry
            .open()
            .map_err(|source| io_error(&source_path, source))?;
        let bytes = read_source_file(file, &source_path, &metadata, MAX_FILE_BYTES)?;
        write_private_file(&target_path, &bytes)?;
        files.push(child_relative);
    }
    Ok(())
}

fn digest_bundle(
    root: &Path,
    files: &[PathBuf],
    manifest: &PluginManifest,
) -> Result<String, PluginBundleError> {
    let mut hasher = Sha256::new();
    for relative in files {
        let relative_text = relative
            .to_str()
            .ok_or_else(|| {
                PluginBundleError::InvalidBundle(format!(
                    "plugin path is not valid UTF-8: {}",
                    relative.display()
                ))
            })?
            .replace('\\', "/");
        update_framed(&mut hasher, relative_text.as_bytes());

        if relative == Path::new("alva.toml") {
            let mut normalized_manifest = manifest.clone();
            normalized_manifest.content_sha256 = None;
            let normalized = toml::to_string(&normalized_manifest)
                .map_err(|error| PluginBundleError::Manifest(error.to_string()))?;
            update_framed(&mut hasher, normalized.as_bytes());
        } else {
            let path = root.join(relative);
            let bytes = read_limited_file(&path, MAX_FILE_BYTES)?;
            update_framed(&mut hasher, &bytes);
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn update_framed(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

fn parse_manifest(path: &Path, bytes: &[u8]) -> Result<PluginManifest, PluginBundleError> {
    let text = std::str::from_utf8(bytes).map_err(|error| {
        PluginBundleError::Manifest(format!("{} is not UTF-8: {error}", path.display()))
    })?;
    let manifest: PluginManifest = toml::from_str(text)
        .map_err(|error| PluginBundleError::Manifest(format!("{}: {error}", path.display())))?;
    manifest.validate().map_err(PluginBundleError::Manifest)?;
    Ok(manifest)
}

fn read_limited_file(path: &Path, limit: u64) -> Result<Vec<u8>, PluginBundleError> {
    let file = File::open(path).map_err(|source| PluginBundleError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut limited = file.take(limit + 1);
    let mut bytes = Vec::new();
    limited
        .read_to_end(&mut bytes)
        .map_err(|source| PluginBundleError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if bytes.len() as u64 > limit {
        return Err(PluginBundleError::Limit(format!(
            "file exceeds {limit} bytes: {}",
            path.display()
        )));
    }
    Ok(bytes)
}

fn read_source_path(path: &Path, limit: u64) -> Result<Vec<u8>, PluginBundleError> {
    let name = path.file_name().ok_or_else(|| {
        PluginBundleError::InvalidBundle(format!(
            "source path has no file name: {}",
            path.display()
        ))
    })?;
    let parent_path = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = CapDir::open_ambient_dir(parent_path, ambient_authority())
        .map_err(|source| io_error(parent_path, source))?;
    let expected_metadata = parent
        .symlink_metadata(name)
        .map_err(|source| io_error(path, source))?;
    let file = parent.open(name).map_err(|source| io_error(path, source))?;
    read_source_file(file, path, &expected_metadata, limit)
}

fn read_source_file(
    file: CapFile,
    path: &Path,
    expected_metadata: &CapMetadata,
    limit: u64,
) -> Result<Vec<u8>, PluginBundleError> {
    if expected_metadata.file_type().is_symlink() || !expected_metadata.is_file() {
        return Err(PluginBundleError::InvalidBundle(format!(
            "source must be a regular non-symlink file: {}",
            path.display()
        )));
    }
    if expected_metadata.len() > limit {
        return Err(PluginBundleError::Limit(format!(
            "file exceeds {limit} bytes: {}",
            path.display()
        )));
    }

    let opened_metadata = file.metadata().map_err(|source| io_error(path, source))?;
    if !opened_metadata.is_file() || !same_source_file(expected_metadata, &opened_metadata) {
        return Err(PluginBundleError::SourceChanged(format!(
            "source file was replaced while being opened: {}",
            path.display()
        )));
    }

    let mut limited = file.take(limit + 1);
    let mut bytes = Vec::new();
    limited
        .read_to_end(&mut bytes)
        .map_err(|source| io_error(path, source))?;
    if bytes.len() as u64 > limit {
        return Err(PluginBundleError::Limit(format!(
            "file exceeds {limit} bytes: {}",
            path.display()
        )));
    }
    if bytes.len() as u64 != opened_metadata.len() {
        return Err(PluginBundleError::SourceChanged(format!(
            "source file changed while being copied: {}",
            path.display()
        )));
    }
    let final_metadata = limited
        .get_ref()
        .metadata()
        .map_err(|source| io_error(path, source))?;
    if !same_source_file(&opened_metadata, &final_metadata) {
        return Err(PluginBundleError::SourceChanged(format!(
            "source file changed while being copied: {}",
            path.display()
        )));
    }
    Ok(bytes)
}

#[cfg(unix)]
fn same_source_file(expected: &CapMetadata, opened: &CapMetadata) -> bool {
    use cap_std::fs::MetadataExt;
    expected.dev() == opened.dev()
        && expected.ino() == opened.ino()
        && expected.len() == opened.len()
        && expected.mtime() == opened.mtime()
        && expected.mtime_nsec() == opened.mtime_nsec()
        && expected.ctime() == opened.ctime()
        && expected.ctime_nsec() == opened.ctime_nsec()
}

#[cfg(not(unix))]
fn same_source_file(expected: &CapMetadata, opened: &CapMetadata) -> bool {
    expected.len() == opened.len() && expected.modified().ok() == opened.modified().ok()
}

fn io_error(path: &Path, source: std::io::Error) -> PluginBundleError {
    PluginBundleError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn create_private_dir(path: &Path) -> Result<(), PluginBundleError> {
    fs::create_dir(path).map_err(|source| PluginBundleError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|source| {
            PluginBundleError::Io {
                path: path.to_path_buf(),
                source,
            }
        })?;
    }
    Ok(())
}

fn write_private_file(path: &Path, bytes: &[u8]) -> Result<(), PluginBundleError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|source| PluginBundleError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|source| PluginBundleError::Io {
            path: path.to_path_buf(),
            source,
        })
}

#[derive(Debug, thiserror::Error)]
pub enum PluginBundleError {
    #[error("plugin bundle I/O at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid plugin manifest: {0}")]
    Manifest(String),
    #[error("invalid plugin bundle: {0}")]
    InvalidBundle(String),
    #[error("plugin bundle limit exceeded: {0}")]
    Limit(String),
    #[error("plugin source changed during snapshot: {0}")]
    SourceChanged(String),
    #[error("plugin content digest mismatch: expected {expected}, got {actual}")]
    DigestMismatch { expected: String, actual: String },
    #[error("plugin bundle staging task failed: {0}")]
    Task(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_manifest(root: &Path, entry: &str, pin: Option<&str>) {
        let pin_line = pin
            .map(|value| format!("content_sha256 = \"{value}\"\n"))
            .unwrap_or_default();
        fs::write(
            root.join("alva.toml"),
            format!(
                "name = \"demo\"\nversion = \"0.1.0\"\nruntime = \"python\"\nentry = \"{entry}\"\n{pin_line}"
            ),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn staged_bundle_is_detached_from_mutable_source() {
        let source = tempfile::tempdir().unwrap();
        write_manifest(source.path(), "main.py", None);
        fs::write(source.path().join("main.py"), b"print('original')\n").unwrap();
        let manifest = read_manifest(source.path()).await.unwrap();

        let staged = StagedPluginBundle::stage(source.path().to_path_buf(), manifest)
            .await
            .unwrap();
        fs::write(source.path().join("main.py"), b"print('changed')\n").unwrap();

        assert_eq!(fs::read(staged.entry()).unwrap(), b"print('original')\n");
        assert_eq!(staged.content_sha256().len(), 64);
    }

    #[tokio::test]
    async fn digest_pin_mismatch_fails_closed() {
        let source = tempfile::tempdir().unwrap();
        write_manifest(source.path(), "main.py", Some(&"0".repeat(64)));
        fs::write(source.path().join("main.py"), b"print('demo')\n").unwrap();
        let manifest = read_manifest(source.path()).await.unwrap();

        let error = StagedPluginBundle::stage(source.path().to_path_buf(), manifest)
            .await
            .err()
            .expect("mismatched digest must fail");
        assert!(matches!(error, PluginBundleError::DigestMismatch { .. }));
    }

    #[tokio::test]
    async fn empty_directories_count_toward_bundle_entry_limit() {
        let source = tempfile::tempdir().unwrap();
        write_manifest(source.path(), "main.py", None);
        fs::write(source.path().join("main.py"), b"print('demo')\n").unwrap();
        for index in 0..MAX_BUNDLE_ENTRIES {
            fs::create_dir(source.path().join(format!("empty-{index:04}"))).unwrap();
        }
        let manifest = read_manifest(source.path()).await.unwrap();

        let error = StagedPluginBundle::stage(source.path().to_path_buf(), manifest)
            .await
            .err()
            .expect("empty directories must count toward the global entry limit");
        assert!(matches!(error, PluginBundleError::Limit(_)));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symbolic_link_in_bundle_is_rejected() {
        use std::os::unix::fs::symlink;

        let source = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        write_manifest(source.path(), "main.py", None);
        symlink(outside.path(), source.path().join("main.py")).unwrap();
        let manifest = read_manifest(source.path()).await.unwrap();

        let error = StagedPluginBundle::stage(source.path().to_path_buf(), manifest)
            .await
            .err()
            .expect("bundle symlink must fail");
        assert!(matches!(error, PluginBundleError::InvalidBundle(_)));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symbolic_link_directory_in_bundle_is_rejected() {
        use std::os::unix::fs::symlink;

        let source = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("main.py"), b"print('outside')\n").unwrap();
        write_manifest(source.path(), "nested/main.py", None);
        symlink(outside.path(), source.path().join("nested")).unwrap();
        let manifest = read_manifest(source.path()).await.unwrap();

        let error = StagedPluginBundle::stage(source.path().to_path_buf(), manifest)
            .await
            .err()
            .expect("bundle directory symlink must fail");
        assert!(matches!(error, PluginBundleError::InvalidBundle(_)));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symbolic_link_manifest_is_rejected_without_following_target() {
        use std::os::unix::fs::symlink;

        let source = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        symlink(outside.path(), source.path().join("alva.toml")).unwrap();

        let error = read_manifest(source.path()).await.unwrap_err();
        assert!(matches!(
            error,
            PluginBundleError::InvalidBundle(_) | PluginBundleError::Io { .. }
        ));
    }
}
