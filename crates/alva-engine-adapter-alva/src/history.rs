// INPUT:  serde_json, std::fs, AgentMessage
// OUTPUT: load_history / save_history for durable Alva Hub session transcripts
// POS:    Disk-backed message history so Hub can resume Alva sessions across process restarts.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use alva_kernel_abi::AgentMessage;

static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Directory of per-session JSON transcripts (`{session_id}.json`).
#[derive(Debug, Clone)]
pub struct HistoryStore {
    root: PathBuf,
    write_lock: Arc<Mutex<()>>,
}

impl HistoryStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            write_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path_for(&self, session_id: &str) -> PathBuf {
        // Preserve readable UUID-like ids. Encode every byte of any other id
        // behind a prefix that cannot occur in the readable form, preventing
        // collisions such as `a/b` versus `a_b`.
        let safe = if !session_id.is_empty()
            && session_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            session_id.to_string()
        } else {
            let mut encoded = String::with_capacity(1 + session_id.len() * 2);
            encoded.push('~');
            for byte in session_id.bytes() {
                use std::fmt::Write as _;
                write!(encoded, "{byte:02x}").expect("writing to String cannot fail");
            }
            encoded
        };
        self.root.join(format!("{safe}.json"))
    }

    pub fn load(&self, session_id: &str) -> Result<Option<Vec<AgentMessage>>, String> {
        validate_history_root(&self.root)?;
        let path = self.path_for(session_id);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!("refuse symlinked history {}", path.display()));
            }
            Ok(metadata) if !metadata.is_file() => {
                return Err(format!("history is not a regular file: {}", path.display()));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("inspect history {}: {error}", path.display())),
        }
        let bytes = fs::read(&path).map_err(|e| format!("read history {}: {e}", path.display()))?;
        let messages: Vec<AgentMessage> = serde_json::from_slice(&bytes)
            .map_err(|e| format!("parse history {}: {e}", path.display()))?;
        Ok(Some(messages))
    }

    pub fn save(&self, session_id: &str, messages: &[AgentMessage]) -> Result<(), String> {
        let _guard = self
            .write_lock
            .lock()
            .map_err(|_| "history store write lock poisoned".to_string())?;
        ensure_private_history_root(&self.root)?;
        let lock_file = open_history_lock(&self.root)?;
        lock_file
            .lock()
            .map_err(|e| format!("lock history directory {}: {e}", self.root.display()))?;

        let path = self.path_for(session_id);
        let json =
            serde_json::to_vec_pretty(messages).map_err(|e| format!("serialize history: {e}"))?;
        atomic_write_private(&path, &json)
    }
}

fn validate_history_root(root: &Path) -> Result<(), String> {
    match fs::symlink_metadata(root) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(format!(
            "refuse symlinked history directory {}",
            root.display()
        )),
        Ok(metadata) if !metadata.is_dir() => Err(format!(
            "history root is not a directory: {}",
            root.display()
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("inspect history root {}: {error}", root.display())),
    }
}

fn ensure_private_history_root(root: &Path) -> Result<(), String> {
    validate_history_root(root)?;
    fs::create_dir_all(root).map_err(|e| format!("create {}: {e}", root.display()))?;
    set_private_dir_permissions(root)
}

fn history_lock_path(root: &Path) -> PathBuf {
    root.join(".history.lock")
}

fn open_history_lock(root: &Path) -> Result<File, String> {
    let path = history_lock_path(root);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(format!("refuse symlinked history lock {}", path.display()));
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err(format!(
                "history lock is not a regular file: {}",
                path.display()
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("inspect history lock {}: {error}", path.display())),
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
        .map_err(|e| format!("open history lock {}: {e}", path.display()))?;
    set_private_file_permissions(&file, &path)?;
    Ok(file)
}

fn atomic_write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(format!("refuse symlinked history {}", path.display()));
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err(format!("history is not a regular file: {}", path.display()));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("inspect history {}: {error}", path.display())),
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
            .unwrap_or("history.json"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use alva_kernel_abi::{AgentMessage, Message};

    #[test]
    fn save_load_roundtrip() {
        let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "alva-hist-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
            sequence
        ));
        fs::create_dir_all(&dir).unwrap();
        let store = HistoryStore::new(&dir);
        let msgs = vec![AgentMessage::Standard(Message::user("hi"))];
        store.save("sess-1", &msgs).unwrap();
        let loaded = store.load("sess-1").unwrap().unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(matches!(loaded[0], AgentMessage::Standard(_)));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn unsafe_session_ids_have_distinct_encoded_paths() {
        let store = HistoryStore::new("/tmp/alva-history-path-test");
        assert_ne!(store.path_for("a/b"), store.path_for("a_b"));
        assert_ne!(store.path_for(""), store.path_for("~"));
        assert_eq!(
            store.path_for("uuid-like_1"),
            store.root().join("uuid-like_1.json")
        );
    }

    #[cfg(unix)]
    #[test]
    fn writes_private_directory_and_files() {
        use std::os::unix::fs::PermissionsExt;

        let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "alva-hist-perms-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
            sequence
        ));
        fs::create_dir_all(&dir).unwrap();
        let store = HistoryStore::new(&dir);
        store
            .save(
                "session",
                &[AgentMessage::Standard(Message::user("private"))],
            )
            .unwrap();

        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(store.path_for("session"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(history_lock_path(&dir))
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
    fn refuses_symlinked_history_without_touching_target() {
        use std::os::unix::fs::symlink;

        let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "alva-hist-symlink-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
            sequence
        ));
        fs::create_dir_all(&dir).unwrap();
        let store = HistoryStore::new(&dir);
        let outside = dir.with_extension("outside");
        fs::write(&outside, b"keep me").unwrap();
        symlink(&outside, store.path_for("session")).unwrap();

        let error = store
            .save(
                "session",
                &[AgentMessage::Standard(Message::user("blocked"))],
            )
            .expect_err("symlinked history must be rejected");
        assert!(error.contains("symlinked history"));
        assert_eq!(fs::read(&outside).unwrap(), b"keep me");

        let _ = fs::remove_dir_all(dir);
        let _ = fs::remove_file(outside);
    }
}
