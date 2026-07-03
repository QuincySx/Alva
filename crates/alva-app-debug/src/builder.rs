// INPUT:  std::sync, std::thread, tiny_http, crate::{ActionRegistry, Inspectable, LogHandle, Router, HttpServer}
// OUTPUT: pub struct DebugServer, pub struct DebugServerBuilder, pub struct DebugServerHandle
// POS:    Builder and lifecycle manager for the debug HTTP server, spawning it on a background thread.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use crate::action_registry::ActionRegistry;
use crate::inspect::Inspectable;
use crate::log_layer::LogHandle;
use crate::router::Router;
use crate::server::HttpServer;

pub struct DebugServer {
    server: tiny_http::Server,
    log_handle: Option<LogHandle>,
    inspector: Option<Arc<dyn Inspectable>>,
    action_registry: Option<Arc<ActionRegistry>>,
    shutdown_flag: Arc<AtomicBool>,
}

pub struct DebugServerBuilder {
    port: u16,
    log_handle: Option<LogHandle>,
    inspector: Option<Arc<dyn Inspectable>>,
    action_registry: Option<Arc<ActionRegistry>>,
    shutdown_flag: Option<Arc<AtomicBool>>,
}

pub struct DebugServerHandle {
    server: Option<Arc<tiny_http::Server>>,
    join_handle: Option<thread::JoinHandle<()>>,
}

impl DebugServer {
    pub fn builder() -> DebugServerBuilder {
        DebugServerBuilder {
            port: 9229,
            log_handle: None,
            inspector: None,
            action_registry: None,
            shutdown_flag: None,
        }
    }
}

impl DebugServerBuilder {
    pub fn port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn with_log_handle(mut self, handle: LogHandle) -> Self {
        self.log_handle = Some(handle);
        self
    }

    pub fn with_inspector(mut self, inspector: impl Inspectable + 'static) -> Self {
        self.inspector = Some(Arc::new(inspector));
        self
    }

    pub fn with_action_registry(mut self, registry: Arc<ActionRegistry>) -> Self {
        self.action_registry = Some(registry);
        self
    }

    pub fn with_shutdown_flag(mut self, flag: Arc<AtomicBool>) -> Self {
        self.shutdown_flag = Some(flag);
        self
    }

    pub fn build(self) -> Result<DebugServer, std::io::Error> {
        let http_server = HttpServer::new(self.port)?;
        Ok(DebugServer {
            server: http_server.into_inner(),
            log_handle: self.log_handle,
            inspector: self.inspector,
            action_registry: self.action_registry,
            shutdown_flag: self
                .shutdown_flag
                .unwrap_or_else(|| Arc::new(AtomicBool::new(false))),
        })
    }
}

impl DebugServer {
    /// The port the server is actually bound to. `build()` binds the
    /// listener, so this is already resolved — pass `.port(0)` to let the
    /// OS pick a free port (parallel-safe tests), then read it back here
    /// before calling [`Self::start`].
    pub fn local_port(&self) -> u16 {
        self.server
            .server_addr()
            .to_ip()
            .map(|a| a.port())
            .unwrap_or(0)
    }

    pub fn start(self) -> DebugServerHandle {
        let server = Arc::new(self.server);
        let server_for_thread = Arc::clone(&server);
        let log_handle = self.log_handle;
        let inspector = self.inspector;
        let action_registry = self.action_registry;
        let shutdown_flag = self.shutdown_flag;

        let join_handle = thread::spawn(move || {
            tracing::info!("Debug server started");
            let router = Router::new(
                log_handle,
                inspector,
                action_registry,
                shutdown_flag.clone(),
            );

            for request in server_for_thread.incoming_requests() {
                router.handle(request);
                if shutdown_flag.load(Ordering::SeqCst) {
                    break;
                }
            }
            tracing::info!("Debug server shut down");
        });

        DebugServerHandle {
            server: Some(server),
            join_handle: Some(join_handle),
        }
    }
}

impl DebugServerHandle {
    pub fn shutdown(&mut self) {
        if let Some(server) = self.server.take() {
            server.unblock();
        }
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for DebugServerHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    //! Tests for DebugServerBuilder.
    //!
    //! Four contracts, four tests:
    //!
    //! 1. **Default port = 9229** — matches V8 inspector port; user-
    //!    facing contract (browser bookmarks, CLI defaults). A silent
    //!    change breaks tooling. Other default-None fields are checked
    //!    in the same test (they encode the opt-in posture).
    //!
    //! 2. **build() without with_shutdown_flag creates fresh false
    //!    AtomicBool** — refcount==1 guards against shared global
    //!    state regression.
    //!
    //! 3. **build() preserves supplied shutdown_flag Arc identity** —
    //!    CRITICAL: external SIGINT handlers store=true on the
    //!    caller's Arc; if build() forwarded a different Arc, the
    //!    server's accept loop would never see the signal (silent
    //!    deadlock).
    //!
    //! 4. **build() threads all with_* options through to DebugServer**
    //!    — catches a build() that forgot to copy a field. Implicitly
    //!    covers individual `is_some` setter contracts; per-setter
    //!    tests are subsumed.
    //!
    //! start()/shutdown()/Drop covered by tests/integration.rs.
    use super::*;
    use crate::inspect::{InspectNode, Inspectable};
    use crate::log_layer::LogCaptureLayer;
    use std::collections::HashMap;

    /// Stub Inspectable used only to exercise `with_inspector`'s
    /// `impl Inspectable + 'static` bound — the inspect() call path
    /// is exercised by integration.rs.
    struct NoopInspectable;
    impl Inspectable for NoopInspectable {
        fn inspect(&self) -> InspectNode {
            InspectNode {
                id: "noop".into(),
                type_name: "Noop".into(),
                bounds: None,
                properties: HashMap::new(),
                children: vec![],
            }
        }
    }

    #[test]
    fn builder_default_port_is_9229_and_all_options_none() {
        // 9229 matches V8 inspector port (intentional for tool reuse);
        // silent change breaks browser bookmarks + CLI defaults. The
        // other 4 fields encode the opt-in posture — silently flipping
        // any to a default value would silently change behavior for
        // unconfigured callers.
        let b = DebugServer::builder();
        assert_eq!(b.port, 9229, "default port must be 9229");
        assert!(b.log_handle.is_none(), "default log_handle must be None");
        assert!(b.inspector.is_none(), "default inspector must be None");
        assert!(
            b.action_registry.is_none(),
            "default action_registry must be None"
        );
        assert!(
            b.shutdown_flag.is_none(),
            "default shutdown_flag must be None"
        );
    }

    #[test]
    fn port_zero_exposes_the_real_bound_port() {
        // Integration tests bind with port 0 (OS-assigned) to stay
        // parallel-safe; they need the REAL port back to talk to the
        // server. build() binds, so the port is known immediately.
        let server = DebugServer::builder()
            .port(0)
            .build()
            .expect("port 0 must bind");
        assert_ne!(server.local_port(), 0, "bound port must be resolved");
    }

    #[test]
    fn build_with_no_supplied_shutdown_flag_creates_fresh_false_atomic() {
        // Pin: build() with no with_shutdown_flag MUST create a
        // FRESH AtomicBool::new(false). A silent change to a
        // shared/cached flag would leak state across server
        // instances and produce surprising shutdown behavior.
        //
        // Use port 0 so the OS picks an ephemeral port; we never
        // call .start() so the bound socket is dropped immediately
        // when `server` goes out of scope.
        let server = DebugServer::builder()
            .port(0)
            .build()
            .expect("port 0 must bind");
        assert!(
            !server.shutdown_flag.load(Ordering::SeqCst),
            "default shutdown_flag must start as false"
        );
        // Sanity: it's a freshly-constructed Arc (refcount 1).
        assert_eq!(
            Arc::strong_count(&server.shutdown_flag),
            1,
            "default shutdown_flag must be a fresh Arc (refcount 1)"
        );
    }

    #[test]
    fn build_preserves_supplied_shutdown_flag_via_arc_ptr_eq() {
        // CRITICAL: the caller-supplied Arc<AtomicBool> MUST reach
        // the built DebugServer unchanged. Otherwise external
        // shutdown triggers (SIGINT handler, parent app lifecycle)
        // signal a different AtomicBool than the server checks in
        // its accept loop — silent deadlock on shutdown.
        let flag = Arc::new(AtomicBool::new(false));
        let server = DebugServer::builder()
            .port(0)
            .with_shutdown_flag(Arc::clone(&flag))
            .build()
            .expect("port 0 must bind");
        assert!(
            Arc::ptr_eq(&server.shutdown_flag, &flag),
            "build() must preserve the supplied shutdown_flag's Arc identity"
        );
        // External signal propagates to the server's view.
        flag.store(true, Ordering::SeqCst);
        assert!(server.shutdown_flag.load(Ordering::SeqCst));
    }

    #[test]
    fn build_threads_options_through_to_server() {
        // Pin: every with_* setting reaches the DebugServer struct.
        // Catches a refactor that forgot to copy a field in build().
        let (_layer, handle) = LogCaptureLayer::new(64);
        let reg = Arc::new(ActionRegistry::new());
        let server = DebugServer::builder()
            .port(0)
            .with_log_handle(handle)
            .with_inspector(NoopInspectable)
            .with_action_registry(Arc::clone(&reg))
            .build()
            .expect("port 0 must bind");
        assert!(server.log_handle.is_some(), "build must thread log_handle");
        assert!(server.inspector.is_some(), "build must thread inspector");
        assert!(
            server.action_registry.is_some(),
            "build must thread action_registry"
        );
    }
}
