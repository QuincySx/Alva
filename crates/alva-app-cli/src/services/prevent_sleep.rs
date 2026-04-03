//! macOS sleep-prevention service.
//!
//! Uses the system `caffeinate` command to prevent the machine from sleeping
//! while agent tasks are running. Employs reference counting so that nested
//! callers can each request wakefulness without conflicting.
//!
//! On non-macOS platforms the module compiles to no-ops.

#[cfg(target_os = "macos")]
mod inner {
    use std::process::{Child, Command};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    /// Prevents macOS from sleeping while at least one guard is alive.
    ///
    /// Internally spawns `caffeinate -i` (prevent idle sleep) when the first
    /// guard is acquired and kills the process when the last guard is dropped.
    pub struct SleepPreventer {
        refcount: AtomicUsize,
        child: Mutex<Option<Child>>,
    }

    impl SleepPreventer {
        /// Create a new preventer (not yet active).
        pub fn new() -> Self {
            Self {
                refcount: AtomicUsize::new(0),
                child: Mutex::new(None),
            }
        }

        /// Increment the reference count. If this is the first reference,
        /// spawns `caffeinate`.
        pub fn start(&self) {
            let prev = self.refcount.fetch_add(1, Ordering::SeqCst);
            if prev == 0 {
                let mut guard = self.child.lock().unwrap();
                if guard.is_none() {
                    match Command::new("caffeinate").arg("-i").spawn() {
                        Ok(child) => {
                            tracing::debug!("caffeinate started (pid={})", child.id());
                            *guard = Some(child);
                        }
                        Err(e) => {
                            tracing::warn!("failed to spawn caffeinate: {e}");
                        }
                    }
                }
            }
        }

        /// Decrement the reference count. If it reaches zero, kills
        /// `caffeinate`.
        pub fn stop(&self) {
            let prev = self.refcount.fetch_sub(1, Ordering::SeqCst);
            if prev == 1 {
                self.kill_child();
            }
        }

        /// Whether the preventer is currently active.
        pub fn is_active(&self) -> bool {
            self.refcount.load(Ordering::SeqCst) > 0
        }

        fn kill_child(&self) {
            let mut guard = self.child.lock().unwrap();
            if let Some(ref mut child) = *guard {
                let _ = child.kill();
                let _ = child.wait();
                tracing::debug!("caffeinate stopped");
            }
            *guard = None;
        }
    }

    impl Default for SleepPreventer {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Drop for SleepPreventer {
        fn drop(&mut self) {
            self.kill_child();
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod inner {
    /// No-op sleep preventer for non-macOS platforms.
    pub struct SleepPreventer;

    impl SleepPreventer {
        pub fn new() -> Self {
            Self
        }

        pub fn start(&self) {}

        pub fn stop(&self) {}

        pub fn is_active(&self) -> bool {
            false
        }
    }

    impl Default for SleepPreventer {
        fn default() -> Self {
            Self::new()
        }
    }
}

pub use inner::SleepPreventer;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_inactive() {
        let p = SleepPreventer::new();
        assert!(!p.is_active());
    }
}
