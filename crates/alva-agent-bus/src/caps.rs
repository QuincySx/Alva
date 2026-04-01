use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

/// Thread-safe capability registry keyed by type.
///
/// Stores `Arc<T>` values indexed by `TypeId`, allowing different layers
/// to register and discover shared capabilities without direct dependencies.
#[derive(Clone)]
pub struct Caps {
    inner: Arc<RwLock<HashMap<TypeId, Box<dyn Any + Send + Sync>>>>,
}

impl Caps {
    /// Create an empty capability registry.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a capability. Overwrites any previous value of the same type.
    pub fn provide<T: Send + Sync + 'static>(&self, value: Arc<T>) {
        let mut map = self.inner.write();
        map.insert(TypeId::of::<T>(), Box::new(value));
    }

    /// Look up a capability by type. Returns `None` if not registered.
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        let map = self.inner.read();
        map.get(&TypeId::of::<T>())
            .and_then(|boxed| boxed.downcast_ref::<Arc<T>>())
            .cloned()
    }

    /// Look up a capability by type, panicking if it is missing.
    pub fn require<T: Send + Sync + 'static>(&self) -> Arc<T> {
        self.get::<T>().unwrap_or_else(|| {
            panic!(
                "Caps: required capability `{}` not found",
                std::any::type_name::<T>()
            )
        })
    }

    /// Check whether a capability of the given type is registered.
    pub fn has<T: Send + Sync + 'static>(&self) -> bool {
        let map = self.inner.read();
        map.contains_key(&TypeId::of::<T>())
    }
}

impl Default for Caps {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DatabasePool;
    struct HttpClient;

    #[test]
    fn provide_and_get() {
        let caps = Caps::new();
        let pool = Arc::new(DatabasePool);
        caps.provide(pool);
        assert!(caps.get::<DatabasePool>().is_some());
    }

    #[test]
    fn get_missing_returns_none() {
        let caps = Caps::new();
        assert!(caps.get::<DatabasePool>().is_none());
    }

    #[test]
    #[should_panic(expected = "required capability")]
    fn require_panics_on_missing() {
        let caps = Caps::new();
        let _: Arc<DatabasePool> = caps.require::<DatabasePool>();
    }

    #[test]
    fn has_returns_correct_value() {
        let caps = Caps::new();
        assert!(!caps.has::<DatabasePool>());
        caps.provide(Arc::new(DatabasePool));
        assert!(caps.has::<DatabasePool>());
    }

    #[test]
    fn overwrite_replaces_value() {
        let caps = Caps::new();
        caps.provide(Arc::new(42_u32));
        caps.provide(Arc::new(99_u32));
        let val = caps.require::<u32>();
        assert_eq!(*val, 99);
    }

    #[test]
    fn concrete_types_are_isolated() {
        let caps = Caps::new();
        caps.provide(Arc::new(42_u32));
        caps.provide(Arc::new("hello"));
        assert_eq!(*caps.require::<u32>(), 42);
        assert_eq!(*caps.require::<&str>(), "hello");
    }

    #[test]
    fn clone_shares_state() {
        let caps = Caps::new();
        let caps2 = caps.clone();
        caps.provide(Arc::new(HttpClient));
        assert!(caps2.get::<HttpClient>().is_some());
    }
}
