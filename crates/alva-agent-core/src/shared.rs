// INPUT:  std::any, std::collections::HashMap, thiserror
// OUTPUT: MiddlewareError, Extensions, MiddlewarePriority
// POS:    Shared types used by both middleware and state — extracted from the old middleware module.

use std::any::{Any, TypeId};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// MiddlewareError
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, thiserror::Error)]
pub enum MiddlewareError {
    #[error("blocked: {reason}")]
    Blocked { reason: String },
    #[error("middleware error: {0}")]
    Other(String),
}

// ---------------------------------------------------------------------------
// Extensions — type-safe key-value store for inter-middleware communication
// ---------------------------------------------------------------------------

pub struct Extensions {
    map: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl Extensions {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn insert<T: Send + Sync + 'static>(&mut self, val: T) {
        self.map.insert(TypeId::of::<T>(), Box::new(val));
    }

    pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.map
            .get(&TypeId::of::<T>())
            .and_then(|b| b.downcast_ref())
    }

    pub fn get_mut<T: Send + Sync + 'static>(&mut self) -> Option<&mut T> {
        self.map
            .get_mut(&TypeId::of::<T>())
            .and_then(|b| b.downcast_mut())
    }

    pub fn remove<T: Send + Sync + 'static>(&mut self) -> Option<T> {
        self.map
            .remove(&TypeId::of::<T>())
            .and_then(|b| b.downcast().ok().map(|b| *b))
    }

    pub fn contains<T: Send + Sync + 'static>(&self) -> bool {
        self.map.contains_key(&TypeId::of::<T>())
    }
}

impl Default for Extensions {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// MiddlewarePriority — standard tiers with 1000-wide gaps
// ---------------------------------------------------------------------------

/// Standard priority tiers for middleware ordering.
///
/// Each tier has 999 slots for sub-ordering within the tier.
/// Use `MiddlewarePriority::SECURITY + 1`, `+ 2`, etc. for multiple
/// middlewares within the same tier.
///
/// ```text
/// 1000  SECURITY    — auth, permission, sandbox
/// 2000  GUARDRAIL   — safety checks, PII filtering
/// 3000  CONTEXT     — context management plugins
/// 4000  ROUTING     — model selection, A/B testing
/// 5000  OBSERVATION  — logging, metrics, tracing
/// 6000  RETRY       — error handling, retry, fallback
/// ```
pub struct MiddlewarePriority;

impl MiddlewarePriority {
    pub const SECURITY: i32 = 1000;
    pub const GUARDRAIL: i32 = 2000;
    pub const CONTEXT: i32 = 3000;
    pub const DEFAULT: i32 = 3000;
    pub const ROUTING: i32 = 4000;
    pub const OBSERVATION: i32 = 5000;
    pub const RETRY: i32 = 6000;
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extensions_insert_get() {
        let mut ext = Extensions::new();

        #[derive(Debug, PartialEq)]
        struct TokenCount(u32);

        #[derive(Debug, PartialEq)]
        struct RequestId(String);

        ext.insert(TokenCount(42));
        ext.insert(RequestId("req-123".to_string()));

        assert_eq!(ext.get::<TokenCount>(), Some(&TokenCount(42)));
        assert_eq!(
            ext.get::<RequestId>(),
            Some(&RequestId("req-123".to_string()))
        );
        assert_eq!(ext.get::<String>(), None);
    }

    #[test]
    fn test_extensions_get_mut() {
        let mut ext = Extensions::new();

        #[derive(Debug, PartialEq)]
        struct Counter(u32);

        ext.insert(Counter(0));
        if let Some(c) = ext.get_mut::<Counter>() {
            c.0 += 10;
        }
        assert_eq!(ext.get::<Counter>(), Some(&Counter(10)));
    }
}
