use super::types::Settings;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Session-level settings cache
#[derive(Debug, Clone)]
pub struct SettingsCache {
    settings: Arc<RwLock<Option<Settings>>>,
}

impl SettingsCache {
    pub fn new() -> Self {
        Self {
            settings: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn get(&self) -> Option<Settings> {
        self.settings.read().await.clone()
    }

    pub async fn set(&self, settings: Settings) {
        *self.settings.write().await = Some(settings);
    }

    pub async fn invalidate(&self) {
        *self.settings.write().await = None;
    }

    pub async fn get_or_load<F, Fut>(&self, loader: F) -> Settings
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Settings>,
    {
        if let Some(cached) = self.get().await {
            return cached;
        }
        let settings = loader().await;
        self.set(settings.clone()).await;
        settings
    }
}
