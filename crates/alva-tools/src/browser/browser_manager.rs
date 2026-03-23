// INPUT:  chromiumoxide, futures, std::collections, std::path, std::sync, tokio::sync, tracing
// OUTPUT: BrowserInstance, TabInfo, BrowserManager, SharedBrowserManager, shared_browser_manager
// POS:    Manages Chrome instance lifecycle, CDP connections, tab navigation, and provides shared access via Arc<Mutex>.
//! BrowserManager — manages Chrome instances and CDP connections
//!
//! Each instance is identified by a string ID (default: "default").
//! The manager handles launching Chrome with the correct flags,
//! establishing CDP connections, and tab lifecycle.

use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::page::Page;
use futures::StreamExt;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

/// A single browser instance with its CDP connection
pub struct BrowserInstance {
    pub id: String,
    pub browser: Browser,
    /// Background handler task — must be kept alive for the CDP connection to work
    _handle: tokio::task::JoinHandle<()>,
    pub profile_dir: Option<PathBuf>,
    pub proxy: Option<String>,
    pub headless: bool,
}

/// Tab info for status reporting
#[derive(Debug, Clone, serde::Serialize)]
pub struct TabInfo {
    pub index: usize,
    pub url: String,
    pub title: String,
}

/// Global browser manager — shared across all browser tools via Arc<Mutex<>>
pub struct BrowserManager {
    instances: HashMap<String, BrowserInstance>,
}

impl BrowserManager {
    pub fn new() -> Self {
        Self {
            instances: HashMap::new(),
        }
    }

    /// Start a new Chrome instance.
    ///
    /// - `id`: instance identifier (default: "default")
    /// - `headless`: run in headless mode (default: true)
    /// - `profile_dir`: optional user-data-dir for persistent profiles
    /// - `proxy`: optional proxy server (e.g. "socks5://127.0.0.1:1080")
    pub async fn start(
        &mut self,
        id: &str,
        headless: bool,
        profile_dir: Option<PathBuf>,
        proxy: Option<String>,
    ) -> Result<(), String> {
        if self.instances.contains_key(id) {
            return Err(format!("Browser instance '{}' is already running", id));
        }

        let mut builder = BrowserConfig::builder();

        if headless {
            builder = builder.arg("--headless=new");
        }

        // Common Chrome flags for automation
        builder = builder
            .arg("--disable-gpu")
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--disable-extensions")
            .arg("--disable-popup-blocking")
            .arg("--disable-translate")
            .arg("--disable-background-timer-throttling")
            .arg("--disable-renderer-backgrounding")
            .arg("--disable-backgrounding-occluded-windows")
            .window_size(1280, 900);

        if let Some(ref dir) = profile_dir {
            builder = builder.user_data_dir(dir.clone());
        }

        if let Some(ref proxy_server) = proxy {
            builder = builder.arg(format!("--proxy-server={}", proxy_server));
        }

        let config = builder
            .build()
            .map_err(|e| format!("Failed to build browser config: {}", e))?;

        let (browser, mut handler) = Browser::launch(config)
            .await
            .map_err(|e| format!("Failed to launch Chrome: {}", e))?;

        // Spawn a background task to drive the CDP event loop
        let handle = tokio::spawn(async move {
            loop {
                if handler.next().await.is_none() {
                    break;
                }
            }
        });

        info!("Browser instance '{}' started (headless={})", id, headless);

        self.instances.insert(
            id.to_string(),
            BrowserInstance {
                id: id.to_string(),
                browser,
                _handle: handle,
                profile_dir,
                proxy,
                headless,
            },
        );

        Ok(())
    }

    /// Stop and close a browser instance
    pub async fn stop(&mut self, id: &str) -> Result<(), String> {
        if let Some(mut instance) = self.instances.remove(id) {
            // Close browser — this also shuts down the Chrome process
            instance
                .browser
                .close()
                .await
                .map_err(|e| format!("Failed to close browser: {}", e))?;
            instance._handle.abort();
            info!("Browser instance '{}' stopped", id);
            Ok(())
        } else {
            Err(format!("Browser instance '{}' not found", id))
        }
    }

    /// Get a reference to a browser instance
    pub fn get(&self, id: &str) -> Option<&BrowserInstance> {
        self.instances.get(id)
    }

    /// Get the currently active page (first tab) for an instance
    pub async fn active_page(&self, id: &str) -> Result<Page, String> {
        let instance = self
            .instances
            .get(id)
            .ok_or_else(|| format!("Browser instance '{}' not found", id))?;

        let pages = instance
            .browser
            .pages()
            .await
            .map_err(|e| format!("Failed to get pages: {}", e))?;

        pages
            .into_iter()
            .last()
            .ok_or_else(|| "No active page/tab found".to_string())
    }

    /// Navigate to a URL — creates a new page if none exists
    pub async fn navigate(&self, id: &str, url: &str) -> Result<Page, String> {
        let instance = self
            .instances
            .get(id)
            .ok_or_else(|| format!("Browser instance '{}' not found", id))?;

        let page = instance
            .browser
            .new_page(url)
            .await
            .map_err(|e| format!("Failed to navigate to '{}': {}", url, e))?;

        // Wait for the page to finish loading
        page.wait_for_navigation()
            .await
            .map_err(|e| format!("Navigation wait failed: {}", e))?;

        Ok(page)
    }

    /// List all open tabs
    pub async fn list_tabs(&self, id: &str) -> Result<Vec<TabInfo>, String> {
        let instance = self
            .instances
            .get(id)
            .ok_or_else(|| format!("Browser instance '{}' not found", id))?;

        let pages = instance
            .browser
            .pages()
            .await
            .map_err(|e| format!("Failed to get pages: {}", e))?;

        let mut tabs = Vec::new();
        for (i, page) in pages.iter().enumerate() {
            let url = page
                .url()
                .await
                .map_err(|e| format!("Failed to get URL: {}", e))?
                .map(|u| u.to_string())
                .unwrap_or_else(|| "about:blank".to_string());

            let title = page
                .get_title()
                .await
                .map_err(|e| format!("Failed to get title: {}", e))?
                .unwrap_or_default();

            tabs.push(TabInfo {
                index: i,
                url,
                title,
            });
        }

        Ok(tabs)
    }

    /// Check if an instance is running
    pub fn is_running(&self, id: &str) -> bool {
        self.instances.contains_key(id)
    }

    /// List all running instance IDs
    pub fn instance_ids(&self) -> Vec<String> {
        self.instances.keys().cloned().collect()
    }

    /// Stop all instances
    pub async fn stop_all(&mut self) {
        let ids: Vec<String> = self.instances.keys().cloned().collect();
        for id in ids {
            if let Err(e) = self.stop(&id).await {
                warn!("Failed to stop browser '{}': {}", id, e);
            }
        }
    }
}

impl Default for BrowserManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared browser manager type used by all browser tools
pub type SharedBrowserManager = Arc<Mutex<BrowserManager>>;

/// Create a new shared browser manager
pub fn shared_browser_manager() -> SharedBrowserManager {
    Arc::new(Mutex::new(BrowserManager::new()))
}
