use std::sync::Arc;
use std::thread;

use crate::inspect::Inspectable;
use crate::log_layer::LogHandle;
use crate::router::Router;
use crate::server::HttpServer;

pub struct DebugServer {
    server: tiny_http::Server,
    log_handle: Option<LogHandle>,
    inspector: Option<Arc<dyn Inspectable>>,
}

pub struct DebugServerBuilder {
    port: u16,
    log_handle: Option<LogHandle>,
    inspector: Option<Arc<dyn Inspectable>>,
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

    pub fn build(self) -> Result<DebugServer, std::io::Error> {
        let http_server = HttpServer::new(self.port)?;
        Ok(DebugServer {
            server: http_server.into_inner(),
            log_handle: self.log_handle,
            inspector: self.inspector,
        })
    }
}

impl DebugServer {
    pub fn start(self) -> DebugServerHandle {
        let server = Arc::new(self.server);
        let server_for_thread = Arc::clone(&server);
        let log_handle = self.log_handle;
        let inspector = self.inspector;

        let join_handle = thread::spawn(move || {
            tracing::info!("Debug server started");
            let router = Router::new(log_handle, inspector);

            for request in server_for_thread.incoming_requests() {
                router.handle(request);
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
