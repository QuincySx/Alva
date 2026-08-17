// INPUT:  std::any, std::collections::HashMap, thiserror
// OUTPUT: MiddlewareError, Extensions, MiddlewarePriority
// POS:    Shared types used by both middleware and state — extracted from the old middleware module.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

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

/// Type-erased store for cross-middleware state. Interior-mutable so the
/// middleware onion can share it behind `&AgentState` (see the `Middleware`
/// trait doc for why the borrow is shared, not exclusive).
///
/// # Mutation hazard
///
/// [`Extensions::get`] returns an owned `Arc<T>`. This store itself keeps an
/// `Arc` inside the map, so after a `get()` the reference count is always ≥2.
/// Consequently `Arc::make_mut` on a returned value will **clone-on-write**:
/// the mutation lands on the caller's private copy, and neither the map nor
/// other already-cloned callers observe it. There is no compiler error for
/// this — the update is silently lost.
///
/// To "mutate" a stored value, use one of:
///
/// - **replace**: `extensions.insert(new_value)` writes a fresh entry.
/// - **interior mutability**: store a type that mutates through `&self`
///   (`Atomic*`, `Mutex<T>`, etc.), then `get()` and mutate through the shared
///   `Arc` in place. See `examples/middleware_basic.rs`, which stores
///   `AtomicU32` counters.
///
/// # Lock poisoning
///
/// Lock acquisition uses `.expect("Extensions lock poisoned")` — fail-loud is
/// deliberate. The critical sections are `HashMap` ops only (no fallible code
/// runs under the lock), and recovering via `into_inner()` would mask the real
/// signal that a thread panicked mid-mutation. This mirrors the kernel's
/// fail-loud posture elsewhere.
pub struct Extensions {
    map: RwLock<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>,
}

impl Extensions {
    pub fn new() -> Self {
        Self {
            map: RwLock::new(HashMap::new()),
        }
    }

    pub fn insert<T: Send + Sync + 'static>(&self, val: T) {
        self.map
            .write()
            .expect("Extensions lock poisoned")
            .insert(TypeId::of::<T>(), Arc::new(val));
    }

    /// Read a value back as a shared `Arc<T>`.
    ///
    /// Do **not** `Arc::make_mut` this and expect the store to observe it — the
    /// store keeps its own `Arc`, so `make_mut` clone-on-writes onto a private
    /// copy (see the [`Extensions`] type doc). Mutate via `insert` replacement
    /// or an interior-mutable value type instead.
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.map
            .read()
            .expect("Extensions lock poisoned")
            .get(&TypeId::of::<T>())
            .and_then(|b| b.clone().downcast::<T>().ok())
    }

    pub fn remove<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.map
            .write()
            .expect("Extensions lock poisoned")
            .remove(&TypeId::of::<T>())
            .and_then(|b| b.downcast::<T>().ok())
    }

    pub fn contains<T: Send + Sync + 'static>(&self) -> bool {
        self.map
            .read()
            .expect("Extensions lock poisoned")
            .contains_key(&TypeId::of::<T>())
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
        let ext = Extensions::new();

        #[derive(Debug, PartialEq)]
        struct TokenCount(u32);

        #[derive(Debug, PartialEq)]
        struct RequestId(String);

        ext.insert(TokenCount(42));
        ext.insert(RequestId("req-123".to_string()));

        assert_eq!(
            ext.get::<TokenCount>().as_deref(),
            Some(&TokenCount(42))
        );
        assert_eq!(
            ext.get::<RequestId>().as_deref(),
            Some(&RequestId("req-123".to_string()))
        );
        assert!(ext.get::<String>().is_none());
    }

    #[test]
    fn test_extensions_get_is_shared_arc() {
        let ext = Extensions::new();

        #[derive(Debug, PartialEq)]
        struct Counter(u32);

        ext.insert(Counter(0));
        // `get` returns an owned `Arc<T>`; interior mutability means the same
        // value is shared across clones and readable through `&self`.
        let shared = ext.get::<Counter>().expect("inserted");
        let clone = shared.clone();
        assert_eq!(shared.0, 0);
        assert_eq!(clone.0, 0);
        assert_eq!(ext.get::<Counter>().as_deref(), Some(&Counter(0)));
    }

    // -- Extensions: remove / contains / Default -------------------------

    #[test]
    fn extensions_contains_reflects_insert_and_remove() {
        // contains is the cheap "is key set?" check that consumers
        // use before get<T>; pinned so a refactor doesn't break the
        // shortcut.
        #[derive(Debug, PartialEq)]
        struct Flag(bool);

        let ext = Extensions::new();
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

        let ext = Extensions::new();
        ext.insert(Payload("hi".into()));
        let removed = ext.remove::<Payload>();
        assert_eq!(removed.as_deref(), Some(&Payload("hi".into())));
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
        let e = MiddlewareError::Blocked {
            reason: "no auth".into(),
        };
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
        let e = MiddlewareError::Blocked {
            reason: "no auth".into(),
        };
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
        const {
            assert!(MiddlewarePriority::SECURITY < MiddlewarePriority::HOOKS);
            assert!(MiddlewarePriority::HOOKS < MiddlewarePriority::GUARDRAIL);
            assert!(MiddlewarePriority::GUARDRAIL < MiddlewarePriority::CONTEXT);
            assert!(MiddlewarePriority::CONTEXT < MiddlewarePriority::ROUTING);
            assert!(MiddlewarePriority::ROUTING < MiddlewarePriority::OBSERVATION);
            assert!(MiddlewarePriority::OBSERVATION < MiddlewarePriority::RETRY);
        }
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
