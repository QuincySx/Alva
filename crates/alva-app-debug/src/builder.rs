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
            shutdown_flag: self.shutdown_flag.unwrap_or_else(|| Arc::new(AtomicBool::new(false))),
        })
    }
}

impl DebugServer {
    pub fn start(self) -> DebugServerHandle {
        let server = Arc::new(self.server);
        let server_for_thread = Arc::clone(&server);
        let log_handle = self.log_handle;
        let inspector = self.inspector;
        let action_registry = self.action_registry;
        let shutdown_flag = self.shutdown_flag;

        let join_handle = thread::spawn(move || {
            tracing::info!("Debug server started");
            let router = Router::new(log_handle, inspector, action_registry, shutdown_flag.clone());

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
