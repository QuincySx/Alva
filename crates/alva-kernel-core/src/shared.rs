// INPUT:  std::any, std::collections::HashMap, thiserror
// OUTPUT: MiddlewareError, Extensions, MiddlewarePriority
// POS:    Shared types used by both middleware and state — extracted from the old middleware module.

use std::any::{Any, TypeId};
use std::collections::HashMap;

use alva_kernel_abi::AgentError;

// ---------------------------------------------------------------------------
// MiddlewareError
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum MiddlewareError {
    #[error("blocked: {reason}")]
    Blocked { reason: String },
    #[error("middleware error: {0}")]
    Other(String),
    #[error(transparent)]
    Agent(#[from] AgentError),
}

impl MiddlewareError {
    pub fn into_agent_error(self) -> AgentError {
        match self {
            MiddlewareError::Blocked { reason } => AgentError::Other(format!("blocked: {reason}")),
            MiddlewareError::Other(message) => {
                AgentError::Other(format!("middleware error: {message}"))
            }
            MiddlewareError::Agent(error) => error,
        }
    }
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
    pub const HOOKS: i32 = 1500;
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

    // -- Extensions: remove / contains / Default -------------------------

    #[test]
    fn extensions_contains_reflects_insert_and_remove() {
        // contains is the cheap "is key set?" check that consumers
        // use before get<T>; pinned so a refactor doesn't break the
        // shortcut.
        #[derive(Debug, PartialEq)]
        struct Flag(bool);

        let mut ext = Extensions::new();
        assert!(!ext.contains::<Flag>());
        ext.insert(Flag(true));
        assert!(ext.contains::<Flag>());
        let _ = ext.remove::<Flag>();
        assert!(!ext.contains::<Flag>(), "remove must clear contains");
    }

    #[test]
    fn extensions_remove_returns_inserted_value() {
        // Pin: remove<T>() is `Option<T>` (not `Option<Box<dyn Any>>`),
        // so callers get the typed value back. Without the downcast
        // path working, this would silently return None.
        #[derive(Debug, PartialEq)]
        struct Payload(String);

        let mut ext = Extensions::new();
        ext.insert(Payload("hi".into()));
        let removed = ext.remove::<Payload>();
        assert_eq!(removed, Some(Payload("hi".into())));
        // Subsequent remove must return None — entry is gone.
        assert!(ext.remove::<Payload>().is_none());
    }

    #[test]
    fn extensions_default_is_empty() {
        let ext: Extensions = Default::default();
        assert!(!ext.contains::<u32>());
    }

    // -- MiddlewareError Display + into_agent_error ----------------------

    #[test]
    fn middleware_error_blocked_display_has_blocked_prefix() {
        let e = MiddlewareError::Blocked { reason: "no auth".into() };
        assert_eq!(format!("{e}"), "blocked: no auth");
    }

    #[test]
    fn middleware_error_other_display_has_prefix() {
        let e = MiddlewareError::Other("boom".into());
        assert_eq!(format!("{e}"), "middleware error: boom");
    }

    #[test]
    fn middleware_error_from_agent_error_passes_through_display() {
        // #[error(transparent)] on Agent variant — Display must match
        // the inner AgentError without any wrapping prefix.
        let inner = AgentError::Cancelled;
        let me: MiddlewareError = inner.into();
        assert_eq!(format!("{me}"), "Cancelled");
    }

    #[test]
    fn into_agent_error_blocked_wraps_reason_with_blocked_prefix() {
        // Pin: blocked errors surface to users with "blocked: ..."
        // prefix so they can diagnose middleware rejections.
        let e = MiddlewareError::Blocked { reason: "no auth".into() };
        let agent_err = e.into_agent_error();
        // AgentError::Other carries the message verbatim (its Display
        // has no prefix — pinned in L115).
        assert_eq!(format!("{agent_err}"), "blocked: no auth");
    }

    #[test]
    fn into_agent_error_other_wraps_with_middleware_prefix() {
        let e = MiddlewareError::Other("oops".into());
        let agent_err = e.into_agent_error();
        assert_eq!(format!("{agent_err}"), "middleware error: oops");
    }

    #[test]
    fn into_agent_error_agent_passes_through_unchanged() {
        // Pin: when the inner error is already AgentError, no extra
        // wrapping. Pulling it out twice should preserve identity.
        let inner = AgentError::Cancelled;
        let me = MiddlewareError::Agent(inner);
        let back = me.into_agent_error();
        // Match on variant identity (AgentError has no PartialEq).
        assert!(matches!(back, AgentError::Cancelled));
    }

    // -- MiddlewarePriority constants -------------------------------------

    #[test]
    fn middleware_priority_tiers_are_strictly_increasing() {
        // Pin the documented tier order: SECURITY < HOOKS < GUARDRAIL <
        // CONTEXT (= DEFAULT) < ROUTING < OBSERVATION < RETRY. Reorder
        // → middleware execution order changes silently (e.g. observation
        // running before security).
        assert!(MiddlewarePriority::SECURITY < MiddlewarePriority::HOOKS);
        assert!(MiddlewarePriority::HOOKS < MiddlewarePriority::GUARDRAIL);
        assert!(MiddlewarePriority::GUARDRAIL < MiddlewarePriority::CONTEXT);
        assert!(MiddlewarePriority::CONTEXT < MiddlewarePriority::ROUTING);
        assert!(MiddlewarePriority::ROUTING < MiddlewarePriority::OBSERVATION);
        assert!(MiddlewarePriority::OBSERVATION < MiddlewarePriority::RETRY);
    }

    #[test]
    fn middleware_priority_default_aliases_context() {
        // Pin: DEFAULT is the same tier as CONTEXT (3000). The doc
        // says so; if a future split breaks this alias, existing
        // DEFAULT callers shift tier silently.
        assert_eq!(MiddlewarePriority::DEFAULT, MiddlewarePriority::CONTEXT);
    }

    #[test]
    fn middleware_priority_tiers_have_thousand_wide_gaps() {
        // Pin: each tier has 999 slots — `MiddlewarePriority::SECURITY + 1`
        // (etc.) is the documented sub-ordering pattern. A future
        // refactor packing tiers tighter would break callers that use
        // additive offsets up to 999.
        for (a, b) in [
            (MiddlewarePriority::SECURITY, MiddlewarePriority::GUARDRAIL),
            (MiddlewarePriority::GUARDRAIL, MiddlewarePriority::CONTEXT),
            (MiddlewarePriority::CONTEXT, MiddlewarePriority::ROUTING),
            (MiddlewarePriority::ROUTING, MiddlewarePriority::OBSERVATION),
            (MiddlewarePriority::OBSERVATION, MiddlewarePriority::RETRY),
        ] {
            assert!(
                b - a >= 1000,
                "tier gap shrunk: {a} → {b} = {} (want ≥ 1000)",
                b - a
            );
        }
    }
}
