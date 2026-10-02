// INPUT: Explicit user terminal actions, saved session cwd, native Unix PTY
// OUTPUT: TerminalManager and bounded TerminalSnapshot for Desktop-only IPC
// POS: User-operated line-mode terminal; separate from Agent shell permissions.

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct TerminalSnapshot {
    pub output: String,
    pub dropped_bytes: u64,
    pub exited: bool,
    pub exit_code: Option<i32>,
}

#[cfg(unix)]
mod native {
    use std::collections::{HashMap, VecDeque};
    use std::fs::File;
    use std::io::{Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command, Stdio};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    };
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    use super::TerminalSnapshot;
    use crate::{workspace_files::session_workspace, HubStore};

    const OUTPUT_LIMIT: usize = 256 * 1024;
    #[derive(Default)]
    struct Output {
        bytes: VecDeque<u8>,
        dropped: u64,
    }

    struct Terminal {
        session: String,
        master: File,
        child: Child,
        output: Arc<Mutex<Output>>,
        stop: Arc<AtomicBool>,
        reader_done: Arc<AtomicBool>,
        reader: Option<JoinHandle<()>>,
        exit_code: Option<i32>,
    }

    impl Terminal {
        fn reap_if_exited(&mut self) -> Result<(), String> {
            if self.exit_code.is_some() {
                return Ok(());
            }
            // Peek without reaping: the PID cannot be reused until the owned
            // process group has been cleaned, avoiding a late kill of a new PID.
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            let result = unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.child.id(),
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if result != 0 {
                return Err("terminal process status unavailable".into());
            }
            if unsafe { info.si_pid() } != 0 {
                self.terminate();
            }
            Ok(())
        }

        fn terminate(&mut self) {
            if self.exit_code.is_none() {
                // +m disables shell job-control groups; ordinary foreground
                // and background jobs remain in the owned group. No name-based kill.
                unsafe {
                    libc::kill(-(self.child.id() as i32), libc::SIGKILL);
                }
                self.exit_code = Some(
                    self.child
                        .wait()
                        .ok()
                        .and_then(|status| status.code())
                        .unwrap_or(-1),
                );
            }
        }
    }

    impl Drop for Terminal {
        fn drop(&mut self) {
            self.terminate();
            self.stop.store(true, Ordering::Release);
            if let Some(reader) = self.reader.take() {
                let _ = reader.join();
            }
        }
    }

    #[derive(Default)]
    pub struct TerminalManager {
        terminals: Mutex<HashMap<String, Terminal>>,
    }

    fn size(rows: u16, columns: u16) -> Result<libc::winsize, String> {
        if !(2..=200).contains(&rows) || !(10..=500).contains(&columns) {
            return Err("terminal size out of bounds".into());
        }
        Ok(libc::winsize {
            ws_row: rows,
            ws_col: columns,
            ws_xpixel: 0,
            ws_ypixel: 0,
        })
    }

    impl TerminalManager {
        pub fn open(
            &self,
            store: &HubStore,
            session: &str,
            rows: u16,
            columns: u16,
        ) -> Result<String, String> {
            let workspace = session_workspace(store, session)?
                .canonicalize()
                .map_err(|_| "terminal workspace unavailable")?;
            if !workspace.is_dir() {
                return Err("terminal workspace is not a directory".into());
            }
            let mut terminals = self
                .terminals
                .lock()
                .map_err(|_| "terminal manager unavailable")?;
            if terminals.len() >= 4 {
                return Err("close an existing terminal before opening another".into());
            }
            let mut dimensions = size(rows, columns)?;
            let (mut master, mut slave) = (-1, -1);
            if unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut dimensions,
                )
            } != 0
            {
                return Err("PTY creation failed".into());
            }
            // Own descriptors immediately so every error path closes them.
            let master = unsafe { File::from_raw_fd(master) };
            let slave = unsafe { File::from_raw_fd(slave) };
            for file in [&master, &slave] {
                if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
                    return Err("PTY descriptor setup failed".into());
                }
            }
            if unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) } < 0 {
                return Err("PTY nonblocking setup failed".into());
            }
            let clone_slave = || {
                slave
                    .try_clone()
                    .map(Stdio::from)
                    .map_err(|_| "PTY stream unavailable".to_string())
            };
            let mut command = Command::new("/bin/sh");
            command
                .args(["+m", "-i"])
                .current_dir(workspace)
                .env_clear()
                .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/bin")
                .env("TERM", "dumb")
                .env("LANG", "en_US.UTF-8")
                .stdin(clone_slave()?)
                .stdout(clone_slave()?)
                .stderr(clone_slave()?);
            unsafe {
                command.pre_exec(|| {
                    if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            let child = command
                .spawn()
                .map_err(|_| "terminal shell could not start")?;
            drop(command);
            drop(slave);
            let output = Arc::new(Mutex::new(Output::default()));
            let stop = Arc::new(AtomicBool::new(false));
            let reader_done = Arc::new(AtomicBool::new(false));
            let mut terminal = Terminal {
                session: session.into(),
                master,
                child,
                output: output.clone(),
                stop: stop.clone(),
                reader_done: reader_done.clone(),
                reader: None,
                exit_code: None,
            };
            let mut reader = terminal
                .master
                .try_clone()
                .map_err(|_| "PTY reader unavailable")?;
            terminal.reader = Some(
                std::thread::Builder::new()
                    .name("hub-terminal".into())
                    .spawn(move || {
                        let mut bytes = [0; 4096];
                        while !stop.load(Ordering::Acquire) {
                            match reader.read(&mut bytes) {
                                Ok(0) => break,
                                Ok(count) => {
                                    let mut buffer =
                                        output.lock().expect("terminal output lock poisoned");
                                    buffer.bytes.extend(&bytes[..count]);
                                    let excess = buffer.bytes.len().saturating_sub(OUTPUT_LIMIT);
                                    buffer.bytes.drain(..excess);
                                    buffer.dropped += excess as u64;
                                }
                                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                    std::thread::sleep(Duration::from_millis(10))
                                }
                                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {
                                    continue
                                }
                                Err(_) => break,
                            }
                        }
                        reader_done.store(true, Ordering::Release);
                    })
                    .map_err(|_| "PTY reader could not start")?,
            );
            let id = uuid::Uuid::new_v4().to_string();
            terminals.insert(id.clone(), terminal);
            Ok(id)
        }

        pub fn read(&self, session: &str, id: &str) -> Result<TerminalSnapshot, String> {
            let mut terminals = self
                .terminals
                .lock()
                .map_err(|_| "terminal manager unavailable")?;
            let terminal = terminals
                .get_mut(id)
                .filter(|terminal| terminal.session == session)
                .ok_or("terminal not found")?;
            terminal.reap_if_exited()?;
            let output = terminal
                .output
                .lock()
                .map_err(|_| "terminal output unavailable")?;
            let bytes: Vec<u8> = output.bytes.iter().copied().collect();
            Ok(TerminalSnapshot {
                output: String::from_utf8_lossy(&bytes).into_owned(),
                dropped_bytes: output.dropped,
                exited: terminal.exit_code.is_some()
                    && terminal.reader_done.load(Ordering::Acquire),
                exit_code: terminal.exit_code,
            })
        }

        pub fn write(&self, session: &str, id: &str, input: &str) -> Result<(), String> {
            if input.len() > 16 * 1024 {
                return Err("terminal input exceeds 16 KiB".into());
            }
            let mut terminals = self
                .terminals
                .lock()
                .map_err(|_| "terminal manager unavailable")?;
            let terminal = terminals
                .get_mut(id)
                .filter(|terminal| terminal.session == session)
                .ok_or("terminal not found")?;
            terminal.reap_if_exited()?;
            if terminal.exit_code.is_some() {
                return Err("terminal process has exited".into());
            }
            let deadline = Instant::now() + Duration::from_millis(250);
            let mut remaining = input.as_bytes();
            while !remaining.is_empty() {
                match terminal.master.write(remaining) {
                    Ok(0) => return Err("terminal input closed".into()),
                    Ok(count) => remaining = &remaining[count..],
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    Err(_) => {
                        return Err("terminal input incomplete; inspect before retrying".into())
                    }
                }
            }
            Ok(())
        }

        pub fn resize(
            &self,
            session: &str,
            id: &str,
            rows: u16,
            columns: u16,
        ) -> Result<(), String> {
            let dimensions = size(rows, columns)?;
            let terminals = self
                .terminals
                .lock()
                .map_err(|_| "terminal manager unavailable")?;
            let terminal = terminals
                .get(id)
                .filter(|terminal| terminal.session == session)
                .ok_or("terminal not found")?;
            if unsafe {
                libc::ioctl(
                    terminal.master.as_raw_fd(),
                    libc::TIOCSWINSZ as _,
                    &dimensions,
                )
            } < 0
            {
                return Err("terminal resize failed".into());
            }
            Ok(())
        }

        pub fn close(&self, session: &str, id: &str) -> Result<(), String> {
            let terminal = {
                let mut terminals = self
                    .terminals
                    .lock()
                    .map_err(|_| "terminal manager unavailable")?;
                if terminals
                    .get(id)
                    .is_some_and(|terminal| terminal.session != session)
                {
                    return Err("terminal not found".into());
                }
                terminals.remove(id)
            };
            drop(terminal);
            Ok(())
        }

        pub fn close_all(&self) {
            if let Ok(mut terminals) = self.terminals.lock() {
                terminals.clear();
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::{EngineTarget, PermissionProfile};

        fn await_output(manager: &TerminalManager, id: &str, text: &str) -> TerminalSnapshot {
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                let output = manager.read("terminal", id).unwrap();
                if output.output.contains(text) {
                    return output;
                }
                assert!(
                    Instant::now() < deadline,
                    "missing expected terminal output: {text}"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        #[test]
        fn pty_input_resize_interrupt_exit_and_reopen_are_real() {
            let root = tempfile::tempdir().unwrap();
            let store = HubStore::open(root.path().join("private/hub.db")).unwrap();
            store
                .prepare_new_turn(
                    "terminal",
                    root.path(),
                    "test",
                    &EngineTarget::for_engine("alva").unwrap(),
                    PermissionProfile::WorkspaceWrite,
                )
                .unwrap();
            let manager = TerminalManager::default();
            let id = manager.open(&store, "terminal", 24, 80).unwrap();
            assert!(manager.read("other", &id).is_err());
            manager.write("terminal", &id, "pwd\n").unwrap();
            await_output(
                &manager,
                &id,
                root.path().file_name().unwrap().to_str().unwrap(),
            );
            manager.resize("terminal", &id, 40, 100).unwrap();
            manager.write("terminal", &id, "stty size\n").unwrap();
            await_output(&manager, &id, "40 100");
            manager.write("terminal", &id, "sleep 30\n").unwrap();
            std::thread::sleep(Duration::from_millis(50));
            manager.write("terminal", &id, "\u{3}").unwrap();
            manager
                .write("terminal", &id, "printf '\\116\\105\\130\\124\\n'\n")
                .unwrap();
            await_output(&manager, &id, "NEXT");
            manager.write("terminal", &id, "exit 7\n").unwrap();
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                let snapshot = manager.read("terminal", &id).unwrap();
                if snapshot.exited {
                    assert_eq!(snapshot.exit_code, Some(7));
                    break;
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(10));
            }
            manager.close("terminal", &id).unwrap();
            let next = manager.open(&store, "terminal", 24, 80).unwrap();
            manager.close("terminal", &id).unwrap(); // late close cannot kill replacement
            assert!(manager.read("terminal", &next).is_ok());
            manager.close("terminal", &next).unwrap();
            assert!(manager.read("terminal", &next).is_err());
        }

        #[test]
        fn closing_reaps_the_owned_group_and_output_retention_is_bounded() {
            let root = tempfile::tempdir().unwrap();
            let store = HubStore::open(root.path().join("private/hub.db")).unwrap();
            store
                .prepare_new_turn(
                    "terminal",
                    root.path(),
                    "test",
                    &EngineTarget::for_engine("alva").unwrap(),
                    PermissionProfile::WorkspaceWrite,
                )
                .unwrap();
            let manager = TerminalManager::default();
            let id = manager.open(&store, "terminal", 24, 80).unwrap();
            manager
                .write("terminal", &id, "head -c 300000 /dev/zero | tr '\\000' x\n")
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                let snapshot = manager.read("terminal", &id).unwrap();
                if snapshot.dropped_bytes > 0 {
                    assert!(snapshot.output.len() <= OUTPUT_LIMIT);
                    break;
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(10));
            }
            manager.write("terminal", &id, "sleep 30 &\n").unwrap();
            std::thread::sleep(Duration::from_millis(50));
            let pid = manager.terminals.lock().unwrap()[&id].child.id() as i32;
            manager.close("terminal", &id).unwrap();
            let deadline = Instant::now() + Duration::from_secs(3);
            while unsafe { libc::kill(-pid, 0) } == 0 {
                assert!(
                    Instant::now() < deadline,
                    "owned terminal group remains alive"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

#[cfg(unix)]
pub use native::TerminalManager;

#[cfg(not(unix))]
#[derive(Default)]
pub struct TerminalManager;

#[cfg(not(unix))]
impl TerminalManager {
    pub fn open(&self, _: &crate::HubStore, _: &str, _: u16, _: u16) -> Result<String, String> {
        Err("Native terminal is currently supported on macOS/Linux only".into())
    }
    pub fn read(&self, _: &str, _: &str) -> Result<TerminalSnapshot, String> {
        Err("Native terminal unsupported".into())
    }
    pub fn write(&self, _: &str, _: &str, _: &str) -> Result<(), String> {
        Err("Native terminal unsupported".into())
    }
    pub fn resize(&self, _: &str, _: &str, _: u16, _: u16) -> Result<(), String> {
        Err("Native terminal unsupported".into())
    }
    pub fn close(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    pub fn close_all(&self) {}
}
