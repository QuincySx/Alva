// INPUT:  filesystem under ~/.config/alva/hub (or ALVA_HUB_HOME), serde_json
// OUTPUT: SessionStore, StoredSession — durable product-session journal for resume
// POS:    Process-restart rehydration for Hub product sessions (engine must still honor engine_session_id).

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// One durable Hub product session record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredSession {
    pub product_session_id: String,
    pub engine_id: String,
    pub engine_session_id: String,
    pub workspace: String,
    /// `light` / `full` when engine is claude; ignored otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_mode: Option<String>,
    /// Engine used before the latest handoff (if any).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_engine_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_engine_session_id: Option<String>,
    /// Unix seconds when last updated.
    pub updated_at: u64,
    /// Last user prompt snippet (diagnostics only; not secret-safe for all content).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_prompt_preview: Option<String>,
    /// Last assistant final-message preview (for handoff preambles).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_result_preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct StoreFile {
    /// product_session_id → record
    #[serde(default)]
    sessions: BTreeMap<String, StoredSession>,
}

/// JSON journal of Hub product sessions.
pub struct SessionStore {
    path: PathBuf,
    write_lock: Mutex<()>,
}

impl SessionStore {
    /// Default location: `$ALVA_HUB_HOME/sessions.json` or
    /// `~/.config/alva/hub/sessions.json`.
    pub fn default_path() -> Option<PathBuf> {
        if let Ok(home) = std::env::var("ALVA_HUB_HOME") {
            return Some(PathBuf::from(home).join("sessions.json"));
        }
        dirs::config_dir().map(|d| d.join("alva").join("hub").join("sessions.json"))
    }

    pub fn open(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            write_lock: Mutex::new(()),
        }
    }

    pub fn open_default() -> Option<Self> {
        Self::default_path().map(Self::open)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load_all(&self) -> Result<BTreeMap<String, StoredSession>, String> {
        validate_parent(&self.path)?;
        match fs::symlink_metadata(&self.path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "refuse symlinked session store {}",
                    self.path.display()
                ));
            }
            Ok(metadata) if !metadata.is_file() => {
                return Err(format!(
                    "session store is not a regular file: {}",
                    self.path.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(BTreeMap::new());
            }
            Err(error) => {
                return Err(format!("inspect {}: {error}", self.path.display()));
            }
        }
        if !self.path.exists() {
            return Ok(BTreeMap::new());
        }
        let bytes =
            fs::read(&self.path).map_err(|e| format!("read {}: {e}", self.path.display()))?;
        let file: StoreFile = serde_json::from_slice(&bytes)
            .map_err(|e| format!("parse {}: {e}", self.path.display()))?;
        Ok(file.sessions)
    }

    pub fn get(&self, product_session_id: &str) -> Result<Option<StoredSession>, String> {
        Ok(self.load_all()?.get(product_session_id).cloned())
    }

    pub fn upsert(&self, session: StoredSession) -> Result<(), String> {
        let _guard = self
            .write_lock
            .lock()
            .map_err(|_| "session store write lock poisoned".to_string())?;
        ensure_parent(&self.path)?;
        let lock_file = open_lock_file(&self.path)?;
        lock_file
            .lock()
            .map_err(|e| format!("lock {}: {e}", lock_path(&self.path).display()))?;

        let mut all = self.load_all()?;
        all.insert(session.product_session_id.clone(), session);
        self.save_all(&all)
    }

    pub fn list_recent(&self, limit: usize) -> Result<Vec<StoredSession>, String> {
        let mut sessions: Vec<_> = self.load_all()?.into_values().collect();
        sessions.sort_by_key(|session| std::cmp::Reverse(session.updated_at));
        sessions.truncate(limit);
        Ok(sessions)
    }

    fn save_all(&self, sessions: &BTreeMap<String, StoredSession>) -> Result<(), String> {
        ensure_parent(&self.path)?;
        let file = StoreFile {
            sessions: sessions.clone(),
        };
        let json =
            serde_json::to_string_pretty(&file).map_err(|e| format!("serialize sessions: {e}"))?;
        atomic_write_private(&self.path, json.as_bytes())
    }
}

fn explicit_parent(path: &Path) -> Option<&Path> {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
}

fn validate_parent(path: &Path) -> Result<(), String> {
    let Some(parent) = explicit_parent(path) else {
        return Ok(());
    };
    match fs::symlink_metadata(parent) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(format!(
            "refuse symlinked session directory {}",
            parent.display()
        )),
        Ok(metadata) if !metadata.is_dir() => Err(format!(
            "session directory is not a directory: {}",
            parent.display()
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("inspect {}: {error}", parent.display())),
    }
}

fn ensure_parent(path: &Path) -> Result<(), String> {
    let Some(parent) = explicit_parent(path) else {
        return Ok(());
    };
    validate_parent(path)?;
    fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    set_private_dir_permissions(parent)
}

fn lock_path(path: &Path) -> PathBuf {
    path.with_file_name(format!(
        ".{}.lock",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("sessions.json")
    ))
}

fn open_lock_file(path: &Path) -> Result<File, String> {
    let path = lock_path(path);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(format!("refuse symlinked lock file {}", path.display()));
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err(format!(
                "lock path is not a regular file: {}",
                path.display()
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("inspect {}: {error}", path.display())),
    }

    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(&path)
        .map_err(|e| format!("open lock {}: {e}", path.display()))?;
    set_private_file_permissions(&file, &path)?;
    Ok(file)
}

fn atomic_write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(format!("refuse symlinked session store {}", path.display()));
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err(format!(
                "session store is not a regular file: {}",
                path.display()
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("inspect {}: {error}", path.display())),
    }

    let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let tmp = path.with_file_name(format!(
        ".{}.{}.{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("sessions.json"),
        std::process::id(),
        nanos,
        sequence
    ));

    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&tmp)
            .map_err(|e| format!("create {}: {e}", tmp.display()))?;
        file.write_all(bytes)
            .map_err(|e| format!("write {}: {e}", tmp.display()))?;
        file.sync_all()
            .map_err(|e| format!("sync {}: {e}", tmp.display()))?;
        drop(file);

        #[cfg(windows)]
        if path.exists() {
            fs::remove_file(path).map_err(|e| format!("remove {}: {e}", path.display()))?;
        }
        fs::rename(&tmp, path)
            .map_err(|e| format!("rename {} → {}: {e}", tmp.display(), path.display()))
    })();

    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(unix)]
fn set_private_dir_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("chmod 0700 {}: {e}", path.display()))
}

#[cfg(not(unix))]
fn set_private_dir_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_permissions(file: &File, path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("chmod 0600 {}: {e}", path.display()))
}

#[cfg(not(unix))]
fn set_private_file_permissions(_file: &File, _path: &Path) -> Result<(), String> {
    Ok(())
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn prompt_preview(prompt: &str, max: usize) -> Option<String> {
    let t = prompt.trim();
    if t.is_empty() {
        return None;
    }
    if t.chars().count() <= max {
        Some(t.to_string())
    } else {
        let s: String = t.chars().take(max.saturating_sub(1)).collect();
        Some(format!("{s}…"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn temp_store() -> (PathBuf, SessionStore) {
        let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let unique = format!(
            "alva-hub-sessions-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
            sequence
        );
        let dir = std::env::temp_dir().join(unique);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sessions.json");
        let store = SessionStore::open(&path);
        (dir, store)
    }

    fn stored_session(id: &str, updated_at: u64) -> StoredSession {
        StoredSession {
            product_session_id: id.into(),
            engine_id: "alva".into(),
            engine_session_id: format!("e-{id}"),
            workspace: "/w".into(),
            claude_mode: None,
            previous_engine_id: None,
            previous_engine_session_id: None,
            updated_at,
            last_prompt_preview: None,
            last_result_preview: None,
        }
    }

    #[test]
    fn upsert_and_get_roundtrip() {
        let (dir, store) = temp_store();
        let s = StoredSession {
            product_session_id: "p1".into(),
            engine_id: "claude".into(),
            engine_session_id: "e1".into(),
            workspace: "/tmp/ws".into(),
            claude_mode: Some("light".into()),
            previous_engine_id: None,
            previous_engine_session_id: None,
            updated_at: 42,
            last_prompt_preview: Some("hi".into()),
            last_result_preview: Some("ok".into()),
        };
        store.upsert(s.clone()).unwrap();
        let loaded = store.get("p1").unwrap().unwrap();
        assert_eq!(loaded, s);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn list_recent_orders_by_updated_at() {
        let (dir, store) = temp_store();
        for (id, ts) in [("a", 1u64), ("b", 3), ("c", 2)] {
            store.upsert(stored_session(id, ts)).unwrap();
        }
        let list = store.list_recent(2).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].product_session_id, "b");
        assert_eq!(list[1].product_session_id, "c");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn concurrent_upserts_preserve_every_record() {
        let (dir, store) = temp_store();
        let store = Arc::new(store);
        let threads: Vec<_> = (0..12)
            .map(|index| {
                let store = Arc::clone(&store);
                std::thread::spawn(move || {
                    let id = format!("session-{index}");
                    store.upsert(stored_session(&id, index)).unwrap();
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }

        assert_eq!(store.load_all().unwrap().len(), 12);
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn writes_private_directory_and_files() {
        use std::os::unix::fs::PermissionsExt;

        let (dir, store) = temp_store();
        store.upsert(stored_session("private", 1)).unwrap();

        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(store.path()).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(lock_path(store.path()))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinked_store_without_touching_target() {
        use std::os::unix::fs::symlink;

        let (dir, store) = temp_store();
        let outside = dir.with_extension("outside");
        fs::write(&outside, b"keep me").unwrap();
        symlink(&outside, store.path()).unwrap();

        let error = store
            .upsert(stored_session("blocked", 1))
            .expect_err("symlinked store must be rejected");
        assert!(error.contains("symlinked session store"));
        assert_eq!(fs::read(&outside).unwrap(), b"keep me");

        let _ = fs::remove_dir_all(dir);
        let _ = fs::remove_file(outside);
    }
}
