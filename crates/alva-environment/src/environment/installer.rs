// INPUT:  std::fs, std::io, std::path, tracing, zip, tar, flate2, super::{config, manifest, versions}
// OUTPUT: Installer, InstallerError
// POS:    Extracts bundled archives (zip/tar.gz) to install runtime components and updates versions.json.
//! Runtime installer — extract/download/verify components.
//!
//! Supports three archive formats:
//! - `zip-flat` — standard zip, contents extracted flat into target directory
//! - `tar.gz`   — gzipped tarball
//! - `qwen-zip` — zip with npm package structure (special handling)

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use tracing::{debug, info};

use super::config::EnvironmentConfig;
use super::manifest::{ArchiveFormat, ArtifactConfig};
use super::versions::InstalledVersions;

/// Handles extracting bundled archives and (in the future) downloading remote ones.
pub struct Installer {
    config: EnvironmentConfig,
}

impl Installer {
    pub fn new(config: EnvironmentConfig) -> Self {
        Self { config }
    }

    /// Install a single component from its artifact config.
    ///
    /// 1. Locate the archive (bundled `packages/` dir, or download from URL).
    /// 2. Parse the archive format.
    /// 3. Extract into the component directory.
    /// 4. Update `versions.json`.
    pub async fn install_component(
        &self,
        name: &str,
        version: &str,
        artifact: &ArtifactConfig,
        installed: &mut InstalledVersions,
    ) -> Result<(), InstallerError> {
        let format = ArchiveFormat::from_str(&artifact.format)
            .ok_or_else(|| InstallerError::UnsupportedFormat(artifact.format.clone()))?;

        // Resolve the archive file path
        let archive_path = self.resolve_archive(name, artifact).await?;

        // Prepare target directory
        let target_dir = self.config.component_dir(name);
        if target_dir.exists() {
            info!(component = name, "Removing old installation");
            fs::remove_dir_all(&target_dir)
                .map_err(|e| InstallerError::Io(target_dir.clone(), e))?;
        }
        fs::create_dir_all(&target_dir)
            .map_err(|e| InstallerError::Io(target_dir.clone(), e))?;

        // Extract
        info!(
            component = name,
            version = version,
            format = ?format,
            "Extracting component"
        );
        match format {
            ArchiveFormat::ZipFlat => Self::extract_zip_flat(&archive_path, &target_dir)?,
            ArchiveFormat::TarGz => Self::extract_tar_gz(&archive_path, &target_dir)?,
            ArchiveFormat::QwenZip => Self::extract_qwen_zip(&archive_path, &target_dir)?,
        }

        // Record the installed version
        installed.set_version(name, version);
        installed.save(&self.config.versions_path()).map_err(|e| {
            InstallerError::Other(format!("Failed to save versions.json: {e}"))
        })?;

        info!(component = name, version = version, "Component installed");
        Ok(())
    }

    /// Resolve the path to the archive file.
    ///
    /// - If the artifact has no URL, look for it in the bundled `packages/` directory.
    /// - If the artifact has a URL, check the packages dir first (cached), then download.
    async fn resolve_archive(
        &self,
        name: &str,
        artifact: &ArtifactConfig,
    ) -> Result<PathBuf, InstallerError> {
        let packages_dir = self.config.packages_dir();
        let bundled_path = packages_dir.join(&artifact.file);

        if bundled_path.exists() {
            debug!(component = name, path = %bundled_path.display(), "Using bundled archive");
            return Ok(bundled_path);
        }

        if let Some(url) = &artifact.url {
            // Ensure the packages directory exists for caching.
            fs::create_dir_all(&packages_dir)
                .map_err(|e| InstallerError::Io(packages_dir.clone(), e))?;

            let target_path = packages_dir.join(&artifact.file);

            #[cfg(feature = "download")]
            {
                info!(
                    component = name,
                    url = url.as_str(),
                    target = %target_path.display(),
                    "Downloading component"
                );

                self.download_file(url, &target_path).await?;
                return Ok(target_path);
            }

            #[cfg(not(feature = "download"))]
            {
                info!(
                    component = name,
                    url = url.as_str(),
                    "Download feature not enabled — cannot fetch remote archive"
                );
                return Err(InstallerError::ArchiveNotFound {
                    component: name.to_string(),
                    expected_path: bundled_path,
                    url: Some(url.clone()),
                });
            }
        }

        Err(InstallerError::ArchiveNotFound {
            component: name.to_string(),
            expected_path: bundled_path,
            url: None,
        })
    }

    // -- Download implementation ----------------------------------------------

    /// Download a file from a URL to the target path.
    #[cfg(feature = "download")]
    async fn download_file(&self, url: &str, target: &Path) -> Result<(), InstallerError> {
        use tokio::io::AsyncWriteExt;

        let response = reqwest::get(url)
            .await
            .map_err(|e| InstallerError::Other(format!("HTTP request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(InstallerError::Other(format!(
                "HTTP {} for {}",
                response.status(),
                url,
            )));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| InstallerError::Other(format!("failed to read response body: {}", e)))?;

        let mut file = tokio::fs::File::create(target)
            .await
            .map_err(|e| InstallerError::Io(target.to_path_buf(), e))?;

        file.write_all(&bytes)
            .await
            .map_err(|e| InstallerError::Io(target.to_path_buf(), e))?;

        file.flush()
            .await
            .map_err(|e| InstallerError::Io(target.to_path_buf(), e))?;

        info!(
            url,
            target = %target.display(),
            bytes = bytes.len(),
            "Download complete"
        );

        Ok(())
    }

    // -- Extraction implementations ------------------------------------------

    /// Extract a "zip-flat" archive: all entries are placed directly into `target_dir`,
    /// stripping any single top-level directory wrapper.
    fn extract_zip_flat(archive: &Path, target_dir: &Path) -> Result<(), InstallerError> {
        let file = fs::File::open(archive)
            .map_err(|e| InstallerError::Io(archive.to_path_buf(), e))?;
        let mut zip = zip::ZipArchive::new(file)
            .map_err(|e| InstallerError::Extract(format!("Invalid zip: {e}")))?;

        // Detect if all entries share a common top-level prefix (single root dir).
        let strip_prefix = Self::detect_zip_strip_prefix(&mut zip);

        for i in 0..zip.len() {
            let mut entry = zip
                .by_index(i)
                .map_err(|e| InstallerError::Extract(format!("Zip entry {i}: {e}")))?;

            let raw_name = entry.name().to_string();
            let relative = if let Some(ref prefix) = strip_prefix {
                match raw_name.strip_prefix(prefix.as_str()) {
                    Some(rest) => rest.to_string(),
                    None => raw_name.clone(),
                }
            } else {
                raw_name.clone()
            };

            if relative.is_empty() || relative == "/" {
                continue;
            }

            let out_path = target_dir.join(&relative);

            if entry.is_dir() {
                fs::create_dir_all(&out_path)
                    .map_err(|e| InstallerError::Io(out_path, e))?;
            } else {
                if let Some(parent) = out_path.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|e| InstallerError::Io(parent.to_path_buf(), e))?;
                }
                let mut out_file = fs::File::create(&out_path)
                    .map_err(|e| InstallerError::Io(out_path.clone(), e))?;
                io::copy(&mut entry, &mut out_file)
                    .map_err(|e| InstallerError::Io(out_path.clone(), e))?;

                // Preserve Unix permissions
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Some(mode) = entry.unix_mode() {
                        fs::set_permissions(&out_path, fs::Permissions::from_mode(mode))
                            .map_err(|e| InstallerError::Io(out_path, e))?;
                    }
                }
            }
        }

        debug!(archive = %archive.display(), "zip-flat extraction complete");
        Ok(())
    }

    /// Detect a common top-level directory in a zip archive.
    /// Returns `Some("dirname/")` if all entries start with the same directory prefix.
    fn detect_zip_strip_prefix(zip: &mut zip::ZipArchive<fs::File>) -> Option<String> {
        let mut first_dir: Option<String> = None;
        for i in 0..zip.len() {
            let entry = match zip.by_index_raw(i) {
                Ok(e) => e,
                Err(_) => return None,
            };
            let name = entry.name();
            if let Some(ref prefix) = first_dir {
                if !name.starts_with(prefix.as_str()) {
                    return None; // Not all entries share the same prefix
                }
            } else {
                // Take the first path component as the candidate prefix.
                if let Some(slash_pos) = name.find('/') {
                    first_dir = Some(name[..=slash_pos].to_string());
                } else {
                    return None; // Top-level file → no common directory
                }
            }
        }
        first_dir
    }

    /// Extract a tar.gz archive into `target_dir`.
    fn extract_tar_gz(archive: &Path, target_dir: &Path) -> Result<(), InstallerError> {
        let file = fs::File::open(archive)
            .map_err(|e| InstallerError::Io(archive.to_path_buf(), e))?;
        let gz = flate2::read::GzDecoder::new(file);
        let mut tar = tar::Archive::new(gz);

        tar.unpack(target_dir)
            .map_err(|e| InstallerError::Extract(format!("tar.gz extraction failed: {e}")))?;

        debug!(archive = %archive.display(), "tar.gz extraction complete");
        Ok(())
    }

    /// Extract a "qwen-zip" archive — same as zip-flat for now.
    /// Qwen Code uses an npm-package structure inside the zip; the resolver
    /// handles finding the correct binary within.
    fn extract_qwen_zip(archive: &Path, target_dir: &Path) -> Result<(), InstallerError> {
        // For now, treat it the same as zip-flat.
        // Future: may need to run `npm rebuild` or patch shebangs.
        Self::extract_zip_flat(archive, target_dir)
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum InstallerError {
    #[error("Unsupported archive format: {0}")]
    UnsupportedFormat(String),

    #[error("Archive not found for '{component}' at {expected_path}{}", url.as_ref().map(|u| format!(" (URL: {u})")).unwrap_or_default())]
    ArchiveNotFound {
        component: String,
        expected_path: PathBuf,
        url: Option<String>,
    },

    #[error("Extraction error: {0}")]
    Extract(String),

    #[error("IO error at {0}: {1}")]
    Io(PathBuf, io::Error),

    #[error("{0}")]
    Other(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Helper: create a minimal zip archive in memory.
    fn create_test_zip(dir: &Path, filename: &str, entries: &[(&str, &[u8])]) -> PathBuf {
        let packages_dir = dir.join("packages");
        fs::create_dir_all(&packages_dir).unwrap();
        let zip_path = packages_dir.join(filename);

        let file = fs::File::create(&zip_path).unwrap();
        let mut zip_writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        for (name, data) in entries {
            zip_writer.start_file(*name, options).unwrap();
            zip_writer.write_all(data).unwrap();
        }
        zip_writer.finish().unwrap();
        zip_path
    }

    /// Helper: create a minimal tar.gz archive in memory.
    fn create_test_tar_gz(dir: &Path, filename: &str, entries: &[(&str, &[u8])]) -> PathBuf {
        let packages_dir = dir.join("packages");
        fs::create_dir_all(&packages_dir).unwrap();
        let tar_path = packages_dir.join(filename);

        let file = fs::File::create(&tar_path).unwrap();
        let gz = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut tar_builder = tar::Builder::new(gz);

        for (name, data) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            tar_builder
                .append_data(&mut header, *name, &data[..])
                .unwrap();
        }
        tar_builder.finish().unwrap();
        tar_path
    }

    #[tokio::test]
    async fn extract_zip_flat_basic() {
        let tmp = tempfile::tempdir().unwrap();
        let config = EnvironmentConfig::new(tmp.path());

        create_test_zip(
            tmp.path(),
            "test.zip",
            &[("hello.txt", b"world"), ("subdir/nested.txt", b"nested")],
        );

        let installer = Installer::new(config.clone());
        let artifact = ArtifactConfig {
            file: "test.zip".to_string(),
            format: "zip-flat".to_string(),
            url: None,
        };
        let mut installed = InstalledVersions::default();

        installer
            .install_component("test-comp", "1.0.0", &artifact, &mut installed)
            .await
            .unwrap();

        let comp_dir = config.component_dir("test-comp");
        assert!(comp_dir.join("hello.txt").exists());
        assert!(comp_dir.join("subdir/nested.txt").exists());
        assert_eq!(installed.components["test-comp"], "1.0.0");
    }

    #[tokio::test]
    async fn extract_tar_gz_basic() {
        let tmp = tempfile::tempdir().unwrap();
        let config = EnvironmentConfig::new(tmp.path());

        create_test_tar_gz(
            tmp.path(),
            "test.tar.gz",
            &[("hello.txt", b"tar world")],
        );

        let installer = Installer::new(config.clone());
        let artifact = ArtifactConfig {
            file: "test.tar.gz".to_string(),
            format: "tar.gz".to_string(),
            url: None,
        };
        let mut installed = InstalledVersions::default();

        installer
            .install_component("tar-comp", "2.0.0", &artifact, &mut installed)
            .await
            .unwrap();

        let comp_dir = config.component_dir("tar-comp");
        assert!(comp_dir.join("hello.txt").exists());
        assert_eq!(installed.components["tar-comp"], "2.0.0");
    }

    #[tokio::test]
    async fn archive_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let config = EnvironmentConfig::new(tmp.path());
        let installer = Installer::new(config);

        let artifact = ArtifactConfig {
            file: "missing.zip".to_string(),
            format: "zip-flat".to_string(),
            url: None,
        };
        let mut installed = InstalledVersions::default();

        let result = installer
            .install_component("missing", "1.0.0", &artifact, &mut installed)
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, InstallerError::ArchiveNotFound { .. }),
            "Expected ArchiveNotFound, got: {err}"
        );
    }
}
