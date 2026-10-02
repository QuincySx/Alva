// INPUT: HubStore session ownership, Tauri native webviews, explicit user navigation
// OUTPUT: UserBrowserManager and bounded BrowserSnapshot for isolated native windows
// POS: Desktop user browser, separate from Agent automation and privileged Hub UI.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindowBuilder};

use crate::HubStore;

#[derive(Clone, Serialize)]
pub struct BrowserSnapshot {
    pub url: String,
    pub loading: bool,
    pub slow: bool,
    pub blocked_navigation: bool,
    pub closed: bool,
}

struct Browser {
    session: String,
    snapshot: BrowserSnapshot,
    loading_since: Instant,
}

#[derive(Default)]
pub struct UserBrowserManager {
    browsers: Arc<Mutex<HashMap<String, Browser>>>,
}

/// Application commands are not capability-restricted by default in Tauri.
/// Keep this independent guard at the global invoke boundary, not only Open.
pub fn privileged_window(label: &str) -> bool {
    label == "main"
}

fn permitted_url(url: &Url, dev_url: Option<&Url>) -> bool {
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.as_str().len() > 4096
    {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    if host == "tauri.localhost" || host.ends_with(".tauri.localhost") {
        return false;
    }
    if let Some(dev) = dev_url {
        let loopback =
            |host: Option<&str>| matches!(host, Some("localhost" | "127.0.0.1" | "[::1]"));
        if url.origin() == dev.origin()
            || (loopback(url.host_str())
                && loopback(dev.host_str())
                && url.port_or_known_default() == dev.port_or_known_default())
        {
            return false;
        }
    }
    true
}

impl UserBrowserManager {
    pub fn open(
        &self,
        app: &AppHandle,
        store: &HubStore,
        session: &str,
        input: &str,
    ) -> Result<String, String> {
        if store.get_session(session)?.is_none() {
            return Err("browser session not found".into());
        }
        let url = Url::parse(input.trim()).map_err(|_| "browser requires an HTTP(S) address")?;
        let dev_url = app.config().build.dev_url.clone();
        if !permitted_url(&url, dev_url.as_ref()) {
            return Err("browser address is not permitted".into());
        }
        let id = format!("user-browser-{}", uuid::Uuid::new_v4());
        {
            let mut browsers = self.browsers.lock().map_err(|_| "browser unavailable")?;
            if browsers.len() >= 4 {
                return Err("browser window limit reached".into());
            }
            browsers.insert(
                id.clone(),
                Browser {
                    session: session.into(),
                    snapshot: BrowserSnapshot {
                        url: url.to_string(),
                        loading: true,
                        slow: false,
                        blocked_navigation: false,
                        closed: false,
                    },
                    loading_since: Instant::now(),
                },
            );
        }
        let navigation_state = self.browsers.clone();
        let navigation_id = id.clone();
        let loading_state = self.browsers.clone();
        let loading_id = id.clone();
        let window = WebviewWindowBuilder::new(app, &id, WebviewUrl::External(url))
            .title("ALVA · User browser")
            .inner_size(1000.0, 750.0)
            .incognito(true)
            .on_navigation(move |url| {
                let allowed = permitted_url(url, dev_url.as_ref());
                if let Ok(mut browsers) = navigation_state.lock() {
                    if let Some(browser) = browsers.get_mut(&navigation_id) {
                        browser.snapshot.blocked_navigation = !allowed;
                        if allowed {
                            browser.snapshot.url = url.to_string();
                            browser.snapshot.loading = true;
                            browser.loading_since = Instant::now();
                        }
                    }
                }
                allowed
            })
            .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
            .on_download(|_, _| false)
            .on_page_load(move |_, payload| {
                if let Ok(mut browsers) = loading_state.lock() {
                    if let Some(browser) = browsers.get_mut(&loading_id) {
                        browser.snapshot.loading =
                            matches!(payload.event(), tauri::webview::PageLoadEvent::Started);
                        if browser.snapshot.loading {
                            browser.loading_since = Instant::now();
                        }
                    }
                }
            })
            .build();
        let window = match window {
            Ok(window) => window,
            Err(_) => {
                self.browsers
                    .lock()
                    .map_err(|_| "browser unavailable")?
                    .remove(&id);
                return Err("browser window could not be opened".into());
            }
        };
        let state = self.browsers.clone();
        let closed_id = id.clone();
        window.on_window_event(move |event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                if let Ok(mut browsers) = state.lock() {
                    browsers.remove(&closed_id);
                }
            }
        });
        Ok(id)
    }

    fn check_owner(&self, session: &str, id: &str) -> Result<bool, String> {
        let browsers = self.browsers.lock().map_err(|_| "browser unavailable")?;
        match browsers.get(id) {
            Some(browser) if browser.session != session => Err("browser session mismatch".into()),
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    pub fn read(&self, session: &str, id: &str) -> Result<BrowserSnapshot, String> {
        let browsers = self.browsers.lock().map_err(|_| "browser unavailable")?;
        let Some(browser) = browsers.get(id) else {
            return Ok(BrowserSnapshot {
                url: String::new(),
                loading: false,
                slow: false,
                blocked_navigation: false,
                closed: true,
            });
        };
        if browser.session != session {
            return Err("browser session mismatch".into());
        }
        let mut snapshot = browser.snapshot.clone();
        snapshot.slow =
            snapshot.loading && browser.loading_since.elapsed() > Duration::from_secs(20);
        Ok(snapshot)
    }

    pub fn action(
        &self,
        app: &AppHandle,
        session: &str,
        id: &str,
        action: &str,
        input: Option<&str>,
    ) -> Result<(), String> {
        if !self.check_owner(session, id)? {
            return if action == "close" {
                Ok(())
            } else {
                Err("browser window closed".into())
            };
        }
        let window = app.get_webview_window(id).ok_or("browser window closed")?;
        let result = match action {
            "navigate" => {
                let url = Url::parse(input.unwrap_or_default().trim())
                    .map_err(|_| "browser requires an HTTP(S) address")?;
                if !permitted_url(&url, app.config().build.dev_url.as_ref()) {
                    return Err("browser address is not permitted".into());
                }
                window.navigate(url)
            }
            "back" => window.eval("history.back()"),
            "forward" => window.eval("history.forward()"),
            "reload" => window.reload(),
            "focus" => window.set_focus(),
            "close" => window.close(),
            _ => return Err("unknown browser action".into()),
        };
        result.map_err(|_| "browser operation unavailable".into())
    }

    pub fn close_all(&self, app: &AppHandle) {
        let ids: Vec<_> = self
            .browsers
            .lock()
            .map(|browsers| browsers.keys().cloned().collect())
            .unwrap_or_default();
        for id in ids {
            if let Some(window) = app.get_webview_window(&id) {
                let _ = window.close();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_browser_cannot_navigate_to_privileged_or_non_web_addresses() {
        let dev: Url = "http://localhost:1421".parse().unwrap();
        for input in [
            "file:///tmp/test",
            "javascript:alert(1)",
            "data:text/html,test",
            "tauri://localhost",
            "https://tauri.localhost/index.html",
            "https://child.tauri.localhost",
            "http://localhost:1421/page",
            "http://127.0.0.1:1421",
            "http://[::1]:1421",
            "https://user:pass@example.com",
        ] {
            assert!(
                !permitted_url(&input.parse().unwrap(), Some(&dev)),
                "{input}"
            );
        }
        for input in [
            "https://example.com",
            "http://127.0.0.1:3000/preview",
            "https://example.com/a?b=1#c",
        ] {
            assert!(
                permitted_url(&input.parse().unwrap(), Some(&dev)),
                "{input}"
            );
        }
        assert!(!permitted_url(
            &format!("https://example.com/{}", "a".repeat(4096))
                .parse()
                .unwrap(),
            None
        ));
        assert!(privileged_window("main"));
        assert!(!privileged_window("user-browser-123"));
    }

    #[test]
    fn browser_handles_are_session_owned_and_closed_handles_are_idempotent() {
        let manager = UserBrowserManager::default();
        manager.browsers.lock().unwrap().insert(
            "handle".into(),
            Browser {
                session: "owner".into(),
                loading_since: Instant::now() - Duration::from_secs(30),
                snapshot: BrowserSnapshot {
                    url: "https://example.com".into(),
                    loading: true,
                    slow: false,
                    blocked_navigation: false,
                    closed: false,
                },
            },
        );
        assert!(manager.read("other", "handle").is_err());
        assert!(manager.check_owner("other", "handle").is_err());
        assert!(manager.read("owner", "handle").unwrap().slow);
        manager.browsers.lock().unwrap().remove("handle");
        assert!(manager.read("owner", "handle").unwrap().closed);
        assert!(!manager.check_owner("owner", "handle").unwrap());
    }
}
