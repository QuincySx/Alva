// INPUT:  std::path::{Path, PathBuf}, tokio::fs
// OUTPUT: ExtractionConfig, ExtractedMemory, MemoryType, write_memory_file(), update_memory_index(), sanitize_filename()
// POS:    Memory extraction and persistence — writes memory entries as markdown files with frontmatter,
//         maintains a MEMORY.md index file, matching Claude Code's auto-memory system.
//! Memory extraction — persist extracted memories as markdown files.
//!
//! This module handles the file-level persistence of memories extracted from
//! conversations. Each memory is written as a markdown file with YAML frontmatter
//! containing metadata (name, description, type). A MEMORY.md index file is
//! maintained listing all memories with descriptions.
//!
//! This complements the SQLite-backed `MemoryService` by providing a
//! human-readable, git-friendly memory format.

use std::path::{Path, PathBuf};

/// Configuration for memory extraction.
#[derive(Debug, Clone)]
pub struct ExtractionConfig {
    /// Directory for auto-memory files.
    pub memory_dir: PathBuf,
    /// Minimum messages since last extraction before triggering.
    pub threshold: usize,
}

impl Default for ExtractionConfig {
    fn default() -> Self {
        Self {
            memory_dir: PathBuf::from(".claude/memory"),
            threshold: alva_types::constants::MEMORY_EXTRACTION_THRESHOLD,
        }
    }
}

/// A memory entry to be persisted to disk.
#[derive(Debug, Clone)]
pub struct ExtractedMemory {
    /// Short name/title for the memory (used in filename and index).
    pub name: String,
    /// One-line description (shown in MEMORY.md index).
    pub description: String,
    /// Classification of the memory.
    pub memory_type: MemoryType,
    /// Full content of the memory.
    pub content: String,
}

/// Classification of extracted memories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryType {
    /// User preferences and habits.
    User,
    /// Feedback on agent behavior (corrections, preferences).
    Feedback,
    /// Project-specific knowledge (architecture, conventions).
    Project,
    /// Reference information (API docs, configs).
    Reference,
}

impl MemoryType {
    /// Return the string identifier for this memory type.
    pub fn as_str(&self) -> &str {
        match self {
            Self::User => "user",
            Self::Feedback => "feedback",
            Self::Project => "project",
            Self::Reference => "reference",
        }
    }
}

/// Write a memory file with YAML frontmatter.
///
/// Creates the memory directory if it doesn't exist.
/// Returns the path of the written file.
pub async fn write_memory_file(
    memory_dir: &Path,
    memory: &ExtractedMemory,
) -> Result<PathBuf, std::io::Error> {
    tokio::fs::create_dir_all(memory_dir).await?;

    let filename = format!(
        "{}_{}.md",
        memory.memory_type.as_str(),
        sanitize_filename(&memory.name)
    );
    let path = memory_dir.join(&filename);

    let content = format!(
        "---\nname: {}\ndescription: {}\ntype: {}\n---\n\n{}",
        memory.name,
        memory.description,
        memory.memory_type.as_str(),
        memory.content
    );

    tokio::fs::write(&path, content).await?;
    Ok(path)
}

/// Update the MEMORY.md index file in the given directory.
///
/// Scans all `.md` files (except MEMORY.md itself) in the directory,
/// extracts their description from frontmatter, and writes a bulleted
/// list to `MEMORY.md`.
pub async fn update_memory_index(memory_dir: &Path) -> Result<(), std::io::Error> {
    // Ensure directory exists before reading
    if !memory_dir.exists() {
        return Ok(());
    }

    let mut entries = Vec::new();
    let mut dir = tokio::fs::read_dir(memory_dir).await?;

    while let Some(entry) = dir.next_entry().await? {
        let path = entry.path();
        if path.extension().map_or(false, |e| e == "md")
            && path
                .file_name()
                .map_or(false, |n| n != "MEMORY.md")
        {
            if let Ok(content) = tokio::fs::read_to_string(&path).await {
                if let Some(desc) = extract_description(&content) {
                    let filename = path.file_name().unwrap().to_string_lossy();
                    entries.push(format!(
                        "- [{}]({}) — {}",
                        filename.trim_end_matches(".md"),
                        filename,
                        desc
                    ));
                }
            }
        }
    }

    // Sort entries for deterministic output
    entries.sort();

    let index_content = format!("# Memory Index\n\n{}\n", entries.join("\n"));
    tokio::fs::write(memory_dir.join("MEMORY.md"), index_content).await?;
    Ok(())
}

/// Extract the `description:` value from YAML frontmatter.
fn extract_description(content: &str) -> Option<String> {
    // Only look within frontmatter (between --- delimiters)
    let mut in_frontmatter = false;
    for line in content.lines() {
        if line.trim() == "---" {
            if !in_frontmatter {
                in_frontmatter = true;
                continue;
            } else {
                // End of frontmatter
                break;
            }
        }
        if in_frontmatter && line.starts_with("description:") {
            return Some(
                line.trim_start_matches("description:")
                    .trim()
                    .to_string(),
            );
        }
    }
    None
}

/// Sanitize a string for use as a filename.
///
/// Replaces non-alphanumeric characters (except `_` and `-`) with `_`,
/// then lowercases the result.
pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .to_lowercase()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_filename_basic() {
        assert_eq!(sanitize_filename("Hello World!"), "hello_world_");
        assert_eq!(sanitize_filename("my-project_v2"), "my-project_v2");
        assert_eq!(sanitize_filename("foo/bar:baz"), "foo_bar_baz");
    }

    #[test]
    fn sanitize_filename_empty() {
        assert_eq!(sanitize_filename(""), "");
    }

    #[test]
    fn extract_description_from_frontmatter() {
        let content = "---\nname: test\ndescription: A test memory\ntype: project\n---\n\nContent here.";
        assert_eq!(
            extract_description(content),
            Some("A test memory".to_string())
        );
    }

    #[test]
    fn extract_description_missing() {
        let content = "---\nname: test\ntype: project\n---\n\nNo description field.";
        assert_eq!(extract_description(content), None);
    }

    #[test]
    fn extract_description_no_frontmatter() {
        let content = "# Just a regular markdown file\n\nSome content.";
        assert_eq!(extract_description(content), None);
    }

    #[test]
    fn extract_description_ignores_body() {
        // description: in the body should not be picked up
        let content = "---\nname: test\n---\n\ndescription: not in frontmatter";
        assert_eq!(extract_description(content), None);
    }

    #[test]
    fn memory_type_as_str() {
        assert_eq!(MemoryType::User.as_str(), "user");
        assert_eq!(MemoryType::Feedback.as_str(), "feedback");
        assert_eq!(MemoryType::Project.as_str(), "project");
        assert_eq!(MemoryType::Reference.as_str(), "reference");
    }

    #[tokio::test]
    async fn write_and_read_memory_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let memory = ExtractedMemory {
            name: "test memory".to_string(),
            description: "A test memory entry".to_string(),
            memory_type: MemoryType::Project,
            content: "This project uses Rust with tokio.".to_string(),
        };

        let path = write_memory_file(tmp.path(), &memory).await.unwrap();

        assert!(path.exists());
        assert_eq!(
            path.file_name().unwrap().to_string_lossy(),
            "project_test_memory.md"
        );

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("name: test memory"));
        assert!(content.contains("description: A test memory entry"));
        assert!(content.contains("type: project"));
        assert!(content.contains("This project uses Rust with tokio."));
    }

    #[tokio::test]
    async fn write_creates_directory() {
        let tmp = tempfile::TempDir::new().unwrap();
        let nested = tmp.path().join("deep").join("nested").join("memory");
        let memory = ExtractedMemory {
            name: "nested".to_string(),
            description: "Nested dir test".to_string(),
            memory_type: MemoryType::User,
            content: "Content".to_string(),
        };

        let path = write_memory_file(&nested, &memory).await.unwrap();
        assert!(path.exists());
    }

    #[tokio::test]
    async fn update_memory_index_basic() {
        let tmp = tempfile::TempDir::new().unwrap();

        // Write two memory files
        let m1 = ExtractedMemory {
            name: "alpha".to_string(),
            description: "First memory".to_string(),
            memory_type: MemoryType::Project,
            content: "Content A".to_string(),
        };
        let m2 = ExtractedMemory {
            name: "beta".to_string(),
            description: "Second memory".to_string(),
            memory_type: MemoryType::Feedback,
            content: "Content B".to_string(),
        };
        write_memory_file(tmp.path(), &m1).await.unwrap();
        write_memory_file(tmp.path(), &m2).await.unwrap();

        // Update index
        update_memory_index(tmp.path()).await.unwrap();

        let index = tokio::fs::read_to_string(tmp.path().join("MEMORY.md"))
            .await
            .unwrap();

        assert!(index.starts_with("# Memory Index"));
        assert!(index.contains("First memory"));
        assert!(index.contains("Second memory"));
        assert!(index.contains("project_alpha"));
        assert!(index.contains("feedback_beta"));
    }

    #[tokio::test]
    async fn update_memory_index_empty_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Should not error on empty directory
        update_memory_index(tmp.path()).await.unwrap();

        let index = tokio::fs::read_to_string(tmp.path().join("MEMORY.md"))
            .await
            .unwrap();
        assert!(index.contains("# Memory Index"));
    }

    #[tokio::test]
    async fn update_memory_index_nonexistent_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let nonexistent = tmp.path().join("does_not_exist");
        // Should not error on nonexistent directory
        let result = update_memory_index(&nonexistent).await;
        assert!(result.is_ok());
    }

    #[test]
    fn extraction_config_default() {
        let config = ExtractionConfig::default();
        assert_eq!(
            config.threshold,
            alva_types::constants::MEMORY_EXTRACTION_THRESHOLD
        );
    }
}
