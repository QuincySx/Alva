// INPUT: HubStore workspace, capability-relative files, canonical SKILL.md parser
// OUTPUT: WorkspaceSkillsManager, workspace Skills catalog/documents and explicit edit actions
// POS: L7 project Skills file manager; no ambient global scan, download or script execution

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Mutex;

use alva_protocol_skill::{fs::FsSkillRepository, types::SkillMeta};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use serde::{Deserialize, Serialize};

use crate::workspace_files::{allowed_path, open_directory, relative_path, session_workspace};
use crate::HubStore;

const MAX_SOURCE: usize = 128 * 1024;
const MAX_SKILLS: usize = 200;
const MANAGED: &str = ".alva/skills/user";
const ARCHIVE: &str = ".alva/skills/archive";
// Low → high local precedence, matching SkillsPlugin discovery. Global roots
// intentionally stay out of this workspace file-management surface.
const ROOTS: &[&str] = &[
    ".alva/skills/bundled",
    ".alva/skills/mbb",
    ".agents/skills",
    ".claude/skills",
    ".alva/skills",
    MANAGED,
    ARCHIVE,
];

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceSkill {
    pub name: String,
    pub description: String,
    pub path: String,
    pub editable: bool,
    pub archived: bool,
    pub can_move: bool,
    /// None means the project enable-list could not be read, not 'enabled'.
    pub enabled: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceSkillCatalog {
    pub skills: Vec<WorkspaceSkill>,
    pub warnings: usize,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceSkillDocument {
    #[serde(flatten)]
    pub skill: WorkspaceSkill,
    pub content: String,
    pub revision: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkspaceSkillAction {
    Read {
        path: String,
    },
    Create {
        content: String,
    },
    Save {
        path: String,
        content: String,
        revision: String,
    },
    Archive {
        path: String,
        revision: String,
    },
    Restore {
        path: String,
        revision: String,
    },
}

#[derive(Default)]
pub struct WorkspaceSkillsManager(Mutex<()>);

fn manifest(content: &str) -> Result<SkillMeta, String> {
    if content.len() > MAX_SOURCE || content.contains('\0') {
        return Err("SKILL.md exceeds the 128 KiB limit or contains NUL".into());
    }
    let meta = FsSkillRepository::parse_frontmatter(content)
        .map_err(|_| "SKILL.md requires valid YAML frontmatter with name and description")?;
    if meta.name.is_empty()
        || meta.name.len() > 64
        || !meta
            .name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        || meta.name.starts_with('-')
        || meta.name.ends_with('-')
        || meta.description.trim().is_empty()
        || meta.description.chars().count() > 1024
        || FsSkillRepository::parse_body(content).markdown.is_empty()
    {
        return Err("Use a lowercase kebab-case name (1–64 characters), a description (1–1024 characters), and non-empty instructions".into());
    }
    Ok(meta)
}

fn revision(content: &str) -> String {
    let mut hash = DefaultHasher::new();
    content.hash(&mut hash);
    format!("{:016x}", hash.finish())
}

fn read_source(dir: &Dir, name: &str) -> Result<String, String> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = dir
        .open_with(name, &options)
        .map_err(|_| "Skill file unavailable or symbolic link")?;
    if !file
        .metadata()
        .map_err(|_| "Skill metadata unavailable")?
        .is_file()
    {
        return Err("Skill content must be a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take((MAX_SOURCE + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "Skill cannot be read")?;
    if bytes.len() > MAX_SOURCE {
        return Err("Skill exceeds the 128 KiB limit".into());
    }
    String::from_utf8(bytes).map_err(|_| "Skill requires UTF-8 text".into())
}

// Missing state.json means canonical default-enable discovery; valid existing
// lists are preserved verbatim, including global names not shown by this UI.
fn optional_directory(root: &Path, path: &str) -> Result<Option<Dir>, String> {
    let relative = relative_path(path)?;
    if !allowed_path(root, &relative) {
        return Err("Skill path is protected".into());
    }
    let mut dir = open_directory(root, Path::new(""))?;
    for part in relative.components() {
        dir = match dir.open_dir_nofollow(part.as_os_str()) {
            Ok(child) => child,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("Skill directory unavailable or symbolic link".into()),
        };
    }
    Ok(Some(dir))
}

fn enabled_state(root: &Path) -> Result<Option<serde_json::Value>, String> {
    let Some(dir) = optional_directory(root, ".alva/skills")? else {
        return Ok(None);
    };
    match dir.symlink_metadata("state.json") {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Skill enable-list unavailable".into()),
        Ok(_) => {}
    }
    let value: serde_json::Value = serde_json::from_str(&read_source(&dir, "state.json")?)
        .map_err(|_| "Skill enable-list is invalid; repair state.json before adding Skills")?;
    if !value["enabled"].as_array().is_some_and(|names| {
        names.len() <= 2000 && names.iter().all(|name| name.as_str().is_some())
    }) {
        return Err("Skill enable-list is invalid; repair state.json before adding Skills".into());
    }
    Ok(Some(value))
}

fn skill_entry(
    meta: SkillMeta,
    path: String,
    state: &Result<Option<serde_json::Value>, String>,
) -> WorkspaceSkill {
    let archived = path.starts_with(&format!("{ARCHIVE}/"));
    let enabled = if archived {
        Some(false)
    } else {
        state.as_ref().ok().map(|state| {
            state.as_ref().map_or(true, |state| {
                state["enabled"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|name| name.as_str() == Some(meta.name.as_str()))
            })
        })
    };
    WorkspaceSkill {
        can_move: (archived || path.starts_with(&format!("{MANAGED}/")))
            && cfg!(any(target_os = "macos", target_os = "linux")),
        editable: path.starts_with(&format!("{MANAGED}/")),
        name: meta.name,
        description: meta.description,
        path,
        archived,
        enabled,
    }
}

fn catalog(root: &Path) -> Result<WorkspaceSkillCatalog, String> {
    let state = enabled_state(root);
    let mut result = WorkspaceSkillCatalog {
        skills: Vec::new(),
        warnings: usize::from(state.is_err()),
        truncated: false,
    };
    let mut inspected = 0;
    for source in ROOTS {
        let dir = match optional_directory(root, source) {
            Ok(Some(dir)) => dir,
            Ok(None) => continue,
            Err(_) => {
                result.warnings += 1;
                continue;
            }
        };
        for entry in dir
            .entries()
            .map_err(|_| "Skill directory cannot be listed")?
        {
            inspected += 1;
            if inspected > MAX_SKILLS {
                result.truncated = true;
                break;
            }
            let Ok(entry) = entry else {
                result.warnings += 1;
                continue;
            };
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            if *source == ".alva/skills"
                && matches!(name.as_str(), "user" | "bundled" | "mbb" | "archive")
            {
                continue;
            }
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let path = format!("{source}/{name}/SKILL.md");
            if !allowed_path(root, &relative_path(&path)?) {
                result.warnings += 1;
                continue;
            }
            let child = match dir.open_dir_nofollow(&name) {
                Ok(dir) => dir,
                Err(_) => {
                    result.warnings += 1;
                    continue;
                }
            };
            // A non-skill folder is not itself an invalid manifest.
            match child.symlink_metadata("SKILL.md") {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => {
                    result.warnings += 1;
                    continue;
                }
            }
            let meta = match read_source(&child, "SKILL.md").and_then(|text| manifest(&text)) {
                Ok(meta) => meta,
                Err(_) => {
                    result.warnings += 1;
                    continue;
                }
            };
            if *source != ARCHIVE {
                result
                    .skills
                    .retain(|skill| skill.archived || skill.name != meta.name);
            }
            result.skills.push(skill_entry(meta, path, &state));
        }
        if result.truncated {
            break;
        }
    }
    result
        .skills
        .sort_by(|a, b| (a.archived, &a.name).cmp(&(b.archived, &b.name)));
    Ok(result)
}

fn document(root: &Path, path: &str) -> Result<WorkspaceSkillDocument, String> {
    let relative = relative_path(path)?;
    let allowed = ROOTS.iter().any(|source| {
        path.strip_prefix(&format!("{source}/"))
            .is_some_and(|rest| {
                rest.strip_suffix("/SKILL.md")
                    .is_some_and(|folder| !folder.is_empty() && !folder.contains('/'))
            })
    });
    if !allowed {
        return Err("Select a workspace SKILL.md from the catalog".into());
    }
    let dir = open_directory(root, relative.parent().ok_or("Invalid Skill path")?)?;
    let content = read_source(&dir, "SKILL.md")?;
    let meta = manifest(&content)?;
    Ok(WorkspaceSkillDocument {
        skill: skill_entry(meta, path.into(), &enabled_state(root)),
        revision: revision(&content),
        content,
    })
}

fn ensure_directory(root: &Path, path: &str) -> Result<Dir, String> {
    if !allowed_path(root, &relative_path(path)?) {
        return Err("Skill path is protected".into());
    }
    let mut dir = open_directory(root, Path::new(""))?;
    for component in relative_path(path)?.components() {
        match dir.create_dir(component.as_os_str()) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err("Skill directory cannot be created".into()),
        }
        dir = dir
            .open_dir_nofollow(component.as_os_str())
            .map_err(|_| "Skill directory unavailable or symbolic link")?;
    }
    Ok(dir)
}

fn atomic_write(dir: &Dir, name: &str, content: &str) -> Result<(), String> {
    if content.len() > MAX_SOURCE {
        return Err("Skill file exceeds the 128 KiB limit".into());
    }
    let temporary = format!(".hub-skill-{}.tmp", uuid::Uuid::new_v4());
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result: Result<(), String> = (|| {
        let mut file = dir
            .open_with(&temporary, &options)
            .map_err(|_| "Skill cannot be saved")?;
        file.write_all(content.as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(|_| "Skill cannot be saved")?;
        dir.rename(&temporary, dir, name)
            .map_err(|_| "Skill cannot be saved".to_string())
    })();
    if result.is_err() {
        let _ = dir.remove_file(&temporary);
    }
    result
}

fn enable_created(root: &Path, name: &str, state: Option<serde_json::Value>) -> Result<(), String> {
    if let Some(mut state) = state {
        let names = state["enabled"].as_array_mut().unwrap();
        if !names.iter().any(|value| value.as_str() == Some(name)) {
            if names.len() >= 2000 {
                return Err("Skill enable-list is full".into());
            }
            names.push(name.into());
        }
        let dir = open_directory(root, Path::new(".alva/skills"))?;
        atomic_write(
            &dir,
            "state.json",
            &serde_json::to_string_pretty(&state)
                .map_err(|_| "Skill enable-list cannot be saved")?,
        )?;
    }
    Ok(())
}

fn move_directory_no_replace(
    from: &Dir,
    name: &std::ffi::OsStr,
    to: &Dir,
    destination: &str,
) -> Result<(), String> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        use std::os::fd::AsRawFd;
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::CString::new(name.as_bytes()).map_err(|_| "Invalid Skill path")?;
        let destination =
            std::ffi::CString::new(destination).map_err(|_| "Invalid Skill destination")?;
        #[cfg(target_os = "macos")]
        let result = unsafe {
            libc::renameatx_np(
                from.as_raw_fd(),
                name.as_ptr(),
                to.as_raw_fd(),
                destination.as_ptr(),
                libc::RENAME_EXCL,
            )
        };
        #[cfg(target_os = "linux")]
        let result = unsafe {
            libc::renameat2(
                from.as_raw_fd(),
                name.as_ptr(),
                to.as_raw_fd(),
                destination.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        return if result == 0 {
            Ok(())
        } else {
            Err("Skill cannot be moved or destination already exists".into())
        };
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (from, name, to, destination);
        Err("Safe Skill archive/restore requires macOS or Linux".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_protocol_skill::repository::SkillRepository;

    fn fixture() -> (
        tempfile::TempDir,
        HubStore,
        std::path::PathBuf,
        WorkspaceSkillsManager,
    ) {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let store = HubStore::open(temp.path().join("private/hub.db")).unwrap();
        store
            .prepare_new_turn(
                "skills",
                &workspace,
                "test",
                &crate::EngineTarget::for_engine("alva").unwrap(),
                crate::PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        (temp, store, workspace, WorkspaceSkillsManager::default())
    }

    fn source(name: &str) -> String {
        format!("---\nname: {name}\ndescription: Review safely\n---\nRead changes and report evidence.\n")
    }
    fn create(
        manager: &WorkspaceSkillsManager,
        store: &HubStore,
        name: &str,
    ) -> WorkspaceSkillDocument {
        manager
            .action(
                store,
                "skills",
                WorkspaceSkillAction::Create {
                    content: source(name),
                },
            )
            .unwrap()
    }

    #[tokio::test]
    async fn create_and_edit_are_discovered_by_the_existing_skill_repository() {
        let (_temp, store, root, manager) = fixture();
        assert!(manager.list(&store, "skills").unwrap().skills.is_empty());
        let doc = create(&manager, &store, "review-code");
        assert_eq!(doc.skill.enabled, Some(true));
        let primary = root.join(".alva/skills");
        let repo = FsSkillRepository::new(
            primary.join("bundled"),
            primary.join("mbb"),
            primary.join("user"),
            primary.join("state.json"),
        )
        .with_flat_primary(&primary);
        let skills = repo.list_skills().await.unwrap();
        assert_eq!(skills.len(), 1);
        assert!(skills[0].enabled);
        assert_eq!(
            repo.load_body("review-code").await.unwrap().markdown,
            "Read changes and report evidence."
        );
        let content = source("review-code").replace("report evidence.", "report tested evidence.");
        let saved = manager
            .action(
                &store,
                "skills",
                WorkspaceSkillAction::Save {
                    path: doc.skill.path.clone(),
                    content: content.clone(),
                    revision: doc.revision,
                },
            )
            .unwrap();
        assert_eq!(saved.content, content);
        assert!(repo
            .load_body("review-code")
            .await
            .unwrap()
            .markdown
            .contains("tested evidence"));
        assert!(manager
            .action(&store, "skills", WorkspaceSkillAction::Create { content })
            .is_err());
    }

    #[test]
    fn state_preserves_unknown_global_names_and_invalid_state_does_not_create_a_skill() {
        let (_temp, store, root, manager) = fixture();
        std::fs::create_dir_all(root.join(".alva/skills")).unwrap();
        let state = root.join(".alva/skills/state.json");
        std::fs::write(
            &state,
            r#"{"enabled":["global-skill"],"extra":"preserved"}"#,
        )
        .unwrap();
        create(&manager, &store, "local-skill");
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&state).unwrap()).unwrap();
        assert_eq!(
            value["enabled"],
            serde_json::json!(["global-skill", "local-skill"])
        );
        assert_eq!(value["extra"], "preserved");
        std::fs::write(&state, "invalid").unwrap();
        assert!(manager
            .action(
                &store,
                "skills",
                WorkspaceSkillAction::Create {
                    content: source("other")
                }
            )
            .is_err());
        assert!(!root.join(".alva/skills/user/other").exists());
        let list = manager.list(&store, "skills").unwrap();
        assert_eq!(list.skills[0].enabled, None);
        assert!(list.warnings > 0);
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[tokio::test]
    async fn archive_restore_preserves_resources_and_never_overwrites_a_destination() {
        let (_temp, store, root, manager) = fixture();
        let doc = create(&manager, &store, "review");
        let dir = root.join(".alva/skills/user/review");
        std::fs::write(dir.join("resource.txt"), "keep me").unwrap();
        let archived = manager
            .action(
                &store,
                "skills",
                WorkspaceSkillAction::Archive {
                    path: doc.skill.path,
                    revision: doc.revision,
                },
            )
            .unwrap();
        assert!(archived.skill.archived);
        assert!(!dir.exists());
        let archive_dir = root
            .join(&archived.skill.path)
            .parent()
            .unwrap()
            .to_path_buf();
        assert_eq!(
            std::fs::read_to_string(archive_dir.join("resource.txt")).unwrap(),
            "keep me"
        );
        let primary = root.join(".alva/skills");
        let repo = FsSkillRepository::new(
            primary.join("bundled"),
            primary.join("mbb"),
            primary.join("user"),
            primary.join("state.json"),
        )
        .with_flat_primary(&primary);
        assert!(repo.list_skills().await.unwrap().is_empty());
        std::fs::create_dir(&dir).unwrap();
        assert!(manager
            .action(
                &store,
                "skills",
                WorkspaceSkillAction::Restore {
                    path: archived.skill.path.clone(),
                    revision: archived.revision.clone()
                }
            )
            .is_err());
        std::fs::remove_dir(&dir).unwrap();
        let restored = manager
            .action(
                &store,
                "skills",
                WorkspaceSkillAction::Restore {
                    path: archived.skill.path,
                    revision: archived.revision,
                },
            )
            .unwrap();
        assert!(restored.skill.editable);
        assert_eq!(
            std::fs::read_to_string(dir.join("resource.txt")).unwrap(),
            "keep me"
        );
        assert!(repo.list_skills().await.unwrap()[0].enabled);
    }

    #[test]
    fn stale_revision_read_only_sources_and_name_changes_are_rejected() {
        let (_temp, store, root, manager) = fixture();
        let doc = create(&manager, &store, "review");
        std::fs::write(
            root.join(&doc.skill.path),
            source("review") + "External change",
        )
        .unwrap();
        assert!(manager
            .action(
                &store,
                "skills",
                WorkspaceSkillAction::Save {
                    path: doc.skill.path.clone(),
                    content: source("review"),
                    revision: doc.revision
                }
            )
            .unwrap_err()
            .contains("changed on disk"));
        let current = manager
            .action(
                &store,
                "skills",
                WorkspaceSkillAction::Read {
                    path: doc.skill.path.clone(),
                },
            )
            .unwrap();
        assert!(manager
            .action(
                &store,
                "skills",
                WorkspaceSkillAction::Save {
                    path: current.skill.path,
                    content: source("renamed"),
                    revision: current.revision
                }
            )
            .is_err());
        let external = root.join(".agents/skills/external");
        std::fs::create_dir_all(&external).unwrap();
        std::fs::write(external.join("SKILL.md"), source("external")).unwrap();
        let readonly = manager
            .action(
                &store,
                "skills",
                WorkspaceSkillAction::Read {
                    path: ".agents/skills/external/SKILL.md".into(),
                },
            )
            .unwrap();
        assert!(!readonly.skill.editable);
        assert!(manager
            .action(
                &store,
                "skills",
                WorkspaceSkillAction::Archive {
                    path: readonly.skill.path,
                    revision: readonly.revision
                }
            )
            .is_err());
    }

    #[test]
    fn invalid_traversal_oversized_and_unknown_session_requests_do_not_write() {
        let (_temp, store, root, manager) = fixture();
        for content in [
            "bad".into(),
            source("../../outside"),
            source("review") + &"x".repeat(MAX_SOURCE),
            source("BadName"),
        ] {
            assert!(manager
                .action(&store, "skills", WorkspaceSkillAction::Create { content })
                .is_err());
        }
        assert!(manager
            .action(
                &store,
                "missing",
                WorkspaceSkillAction::Create {
                    content: source("review")
                }
            )
            .is_err());
        assert!(manager
            .action(
                &store,
                "skills",
                WorkspaceSkillAction::Read {
                    path: ".alva/skills/user/../../config.json".into()
                }
            )
            .is_err());
        assert!(!root.join(".alva").exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_ancestors_and_manifests_cannot_escape_the_workspace() {
        use std::os::unix::fs::symlink;
        let (temp, store, root, manager) = fixture();
        let outside = temp.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("SKILL.md"), source("secret")).unwrap();
        std::fs::create_dir_all(root.join(".agents/skills/link")).unwrap();
        symlink(
            outside.join("SKILL.md"),
            root.join(".agents/skills/link/SKILL.md"),
        )
        .unwrap();
        let list = manager.list(&store, "skills").unwrap();
        assert!(list.skills.is_empty());
        assert!(list.warnings > 0);
        symlink(&outside, root.join(".alva")).unwrap();
        assert!(manager
            .action(
                &store,
                "skills",
                WorkspaceSkillAction::Create {
                    content: source("review")
                }
            )
            .is_err());
        assert!(!outside.join("skills").exists());
    }
}

impl WorkspaceSkillsManager {
    pub fn list(&self, store: &HubStore, session: &str) -> Result<WorkspaceSkillCatalog, String> {
        let _guard = self.0.lock().map_err(|_| "Skill manager unavailable")?;
        catalog(&session_workspace(store, session)?)
    }

    pub fn action(
        &self,
        store: &HubStore,
        session: &str,
        action: WorkspaceSkillAction,
    ) -> Result<WorkspaceSkillDocument, String> {
        let _guard = self.0.lock().map_err(|_| "Skill manager unavailable")?;
        let root = session_workspace(store, session)?;
        let (path, content, expected, mode) = match action {
            WorkspaceSkillAction::Read { path } => return document(&root, &path),
            WorkspaceSkillAction::Create { content } => {
                let meta = manifest(&content)?;
                let existing = catalog(&root)?;
                if existing.truncated {
                    return Err("Refresh a complete Skill catalog before creating a Skill".into());
                }
                if existing
                    .skills
                    .iter()
                    .any(|skill| !skill.archived && skill.name == meta.name)
                {
                    return Err("A workspace Skill with this name already exists".into());
                }
                let state = enabled_state(&root)?;
                if !allowed_path(&root, Path::new(&format!("{MANAGED}/{}", meta.name))) {
                    return Err("Skill path is protected".into());
                }
                let parent = ensure_directory(&root, MANAGED)?;
                parent
                    .create_dir(&meta.name)
                    .map_err(|_| "Skill directory already exists or cannot be created")?;
                let dir = parent
                    .open_dir_nofollow(&meta.name)
                    .map_err(|_| "Skill directory unavailable")?;
                atomic_write(&dir, "SKILL.md", &content)?;
                if let Err(error) = enable_created(&root, &meta.name, state) {
                    let _ = dir.remove_file("SKILL.md");
                    return Err(error);
                }
                return document(&root, &format!("{MANAGED}/{}/SKILL.md", meta.name));
            }
            WorkspaceSkillAction::Save {
                path,
                content,
                revision,
            } => (path, Some(content), revision, "save"),
            WorkspaceSkillAction::Archive { path, revision } => (path, None, revision, "archive"),
            WorkspaceSkillAction::Restore { path, revision } => (path, None, revision, "restore"),
        };
        let current = document(&root, &path)?;
        if current.revision != expected {
            return Err("Skill changed on disk; reload before saving or moving it".into());
        }
        if mode == "save" {
            if !current.skill.editable {
                return Err("Only Skills under .alva/skills/user can be edited".into());
            }
            let content = content.unwrap();
            if manifest(&content)?.name != current.skill.name {
                return Err("Editing cannot rename a Skill".into());
            }
            let dir = open_directory(&root, relative_path(&path)?.parent().unwrap())?;
            atomic_write(&dir, "SKILL.md", &content)?;
            return document(&root, &path);
        }
        if (mode == "archive" && !current.skill.editable)
            || (mode == "restore" && !current.skill.archived)
        {
            return Err("Only managed project Skills can be archived or restored".into());
        }
        let state = if mode == "restore" {
            enabled_state(&root)?
        } else {
            None
        };
        let destination = if mode == "archive" { ARCHIVE } else { MANAGED };
        let target = ensure_directory(&root, destination)?;
        let folder = if mode == "archive" {
            uuid::Uuid::new_v4().to_string()
        } else {
            current.skill.name.clone()
        };
        if target.symlink_metadata(&folder).is_ok() {
            return Err("A Skill with this destination already exists".into());
        }
        let source = relative_path(&path)?;
        let source_folder = source.parent().unwrap();
        let parent = open_directory(&root, source_folder.parent().unwrap())?;
        if mode == "restore" {
            if catalog(&root)?
                .skills
                .iter()
                .any(|skill| !skill.archived && skill.name == current.skill.name)
            {
                return Err("An active workspace Skill with this name already exists".into());
            }
            enable_created(&root, &current.skill.name, state)?;
        }
        move_directory_no_replace(
            &parent,
            source_folder.file_name().unwrap(),
            &target,
            &folder,
        )?;
        document(&root, &format!("{destination}/{folder}/SKILL.md"))
    }
}
