// INPUT:  tracing
// OUTPUT: pub macro traced, pub macro traced_listener
// POS:    Macros that wrap GPUI event handlers with tracing instrumentation, compiling to zero overhead in release builds.
/// Wraps a GPUI event handler (3 params: event, window, cx) with tracing.
/// In release builds, this compiles to just the handler — zero overhead.
///
/// Usage:
///   div().on_click(alva_app_debug::traced!("send_btn:click", move |_, _, cx| { ... }))
///   div().on_click(alva_app_debug::traced!("send_btn:click", |event, window, cx| { ... }))
#[cfg(debug_assertions)]
#[macro_export]
macro_rules! traced {
    ($name:expr, move |$p1:tt, $p2:tt, $p3:tt| $body:block) => {
        move |$p1, $p2, $p3| {
            ::tracing::info!(target: "gpui_event", name = $name, "event");
            $body
        }
    };
    ($name:expr, |$p1:tt, $p2:tt, $p3:tt| $body:block) => {
        |$p1, $p2, $p3| {
            ::tracing::info!(target: "gpui_event", name = $name, "event");
            $body
        }
    };
}

#[cfg(not(debug_assertions))]
#[macro_export]
macro_rules! traced {
    ($name:expr, $handler:expr) => {
        $handler
    };
}

/// Wraps a cx.listener()-style handler (4 params: this, event, window, cx) with tracing.
/// In release builds, this compiles to just the handler — zero overhead.
///
/// Usage:
///   .on_click(cx.listener(alva_app_debug::traced_listener!("send_btn:click", |this, event, window, cx| { ... })))
///   .on_click(cx.listener(alva_app_debug::traced_listener!("send_btn:click", |this, _: &ClickEvent, _, cx| { ... })))
#[cfg(debug_assertions)]
#[macro_export]
macro_rules! traced_listener {
    // p2 has type annotation: |p1, p2: Type, p3, p4|
    ($name:expr, |$p1:tt, $p2:tt : $p2ty:ty, $p3:tt, $p4:tt| $body:block) => {
        |$p1, $p2 : $p2ty, $p3, $p4| {
            ::tracing::info!(target: "gpui_event", name = $name, "event");
            $body
        }
    };
    // p2 has type annotation with move: move |p1, p2: Type, p3, p4|
    ($name:expr, move |$p1:tt, $p2:tt : $p2ty:ty, $p3:tt, $p4:tt| $body:block) => {
        move |$p1, $p2 : $p2ty, $p3, $p4| {
            ::tracing::info!(target: "gpui_event", name = $name, "event");
            $body
        }
    };
    // 4 simple params without move
    ($name:expr, |$p1:tt, $p2:tt, $p3:tt, $p4:tt| $body:block) => {
        |$p1, $p2, $p3, $p4| {
            ::tracing::info!(target: "gpui_event", name = $name, "event");
            $body
        }
    };
    // 4 simple params with move
    ($name:expr, move |$p1:tt, $p2:tt, $p3:tt, $p4:tt| $body:block) => {
        move |$p1, $p2, $p3, $p4| {
            ::tracing::info!(target: "gpui_event", name = $name, "event");
            $body
        }
    };
}

#[cfg(not(debug_assertions))]
#[macro_export]
macro_rules! traced_listener {
    ($name:expr, $handler:expr) => {
        $handler
    };
}

#[cfg(test)]
mod tests {
    //! Tests for `traced!` + `traced_listener!` macro expansion.
    //!
    //! Both macros have two cfg-gated branches:
    //! - debug_assertions: wraps the handler body with
    //!   `tracing::info!(target:"gpui_event", name=$name)` before
    //!   executing the body
    //! - release (no debug_assertions): identity passthrough — zero
    //!   overhead is a CRITICAL silent contract
    //!
    //! `cargo test` runs with debug_assertions=true so we can only
    //! pin the debug branch here. The release branch's contract is
    //! "the handler is returned unchanged" which is a compile-time
    //! property: changing `$handler` to `{ $handler }` or wrapping it
    //! would silently add overhead but still compile. Out of scope
    //! for this loop.
    //!
    //! Each match arm of each macro must:
    //! 1. expand without compile error
    //! 2. produce a closure of the correct arity (3 for `traced!`,
    //!    4 for `traced_listener!`)
    //! 3. invoke the user-provided body when called
    //! 4. propagate the body's return value out of the closure
    //!
    //! Tracing output verification (the actual `tracing::info!` call)
    //! would require `tracing-test` (new dev-dep, blocked). The
    //! tracing call itself is a no-op when no subscriber is installed,
    //! so the closure still invokes correctly.
    use std::sync::atomic::{AtomicI32, Ordering};
    use std::sync::Arc;

    // -- traced! (3 param): 2 arms ---------------------------------------

    #[test]
    fn traced_bare_3param_closure_invokes_body_and_sees_args() {
        // bare `|p1, p2, p3|` arm — params are bare identifiers (the
        // macro uses `tt` fragments, which match a single token; the
        // closure caller supplies typed values that infer through).
        let sink = AtomicI32::new(0);
        let handler = crate::traced!("test:bare3", |a, b, c| {
            sink.fetch_add(a + b + c, Ordering::SeqCst);
        });
        handler(1i32, 2, 3);
        handler(10, 20, 30);
        assert_eq!(
            sink.load(Ordering::SeqCst),
            66,
            "body must execute with each call's args"
        );
    }

    #[test]
    fn traced_move_3param_closure_invokes_body_with_owned_capture() {
        // `move |p1, p2, p3|` arm — closure takes ownership of capture.
        let counter = Arc::new(AtomicI32::new(0));
        let counter_inner = Arc::clone(&counter);
        let handler = crate::traced!("test:move3", move |a, b, c| {
            counter_inner.fetch_add(a * b * c, Ordering::SeqCst);
        });
        handler(2i32, 3, 4);
        assert_eq!(counter.load(Ordering::SeqCst), 24);
    }

    #[test]
    fn traced_body_return_value_propagates_out_of_wrapper_closure() {
        // CRITICAL: the macro emits `tracing::info!(...); $body` —
        // because $body is the last expr in the closure body, its
        // value MUST become the closure's return value. A refactor
        // that did `tracing::info!(...); $body;` (extra `;`) would
        // silently turn every GPUI handler into one returning `()`.
        let handler = crate::traced!("test:retval", |a, b, _c| { a + b });
        let result: i32 = handler(7i32, 8, 0);
        assert_eq!(result, 15, "body's return value must propagate");
    }

    // -- traced_listener! (4 param): 4 arms ------------------------------

    #[test]
    fn traced_listener_bare_4param_closure_invokes_body() {
        // bare `|p1, p2, p3, p4|` arm.
        let sink = AtomicI32::new(0);
        let handler = crate::traced_listener!("test:listener:bare4", |a, b, c, d| {
            sink.fetch_add(a + b + c + d, Ordering::SeqCst);
        });
        handler(1i32, 2, 3, 4);
        assert_eq!(sink.load(Ordering::SeqCst), 10);
    }

    #[test]
    fn traced_listener_move_4param_closure_invokes_body() {
        // `move |p1, p2, p3, p4|` arm.
        let counter = Arc::new(AtomicI32::new(0));
        let counter_inner = Arc::clone(&counter);
        let handler = crate::traced_listener!("test:listener:move4", move |a, b, c, d| {
            counter_inner.fetch_add(a + b + c + d, Ordering::SeqCst);
        });
        handler(5i32, 5, 5, 5);
        assert_eq!(counter.load(Ordering::SeqCst), 20);
    }

    #[test]
    fn traced_listener_with_p2_type_annotation_compiles_and_invokes() {
        // The type-annotated arm: `|$p1:tt, $p2:tt : $p2ty:ty, $p3:tt, $p4:tt|`.
        // This arm exists because GPUI handlers often need explicit
        // event types ONLY on $p2 (e.g., `|this, _: &ClickEvent, _, cx| {...}`).
        // A typo in the arm's pattern (e.g., missing the `:ty`
        // fragment) would break this idiom but only show up at the
        // first call site that uses it — pin here to catch immediately.
        let sink = AtomicI32::new(0);
        let handler = crate::traced_listener!("test:listener:typed", |a, b: &i32, c, d| {
            sink.fetch_add(a + *b + c + d, Ordering::SeqCst);
        });
        let event: i32 = 99;
        handler(1i32, &event, 2, 3);
        assert_eq!(sink.load(Ordering::SeqCst), 105);
    }

    #[test]
    fn traced_listener_with_p2_type_annotation_and_move_compiles_and_invokes() {
        // `move |$p1:tt, $p2:tt : $p2ty:ty, $p3:tt, $p4:tt|` arm —
        // the move + typed combo. Separate arm in the macro because
        // macro_rules cannot make `move` optional in one pattern.
        let counter = Arc::new(AtomicI32::new(0));
        let counter_inner = Arc::clone(&counter);
        let handler =
            crate::traced_listener!("test:listener:typed_move", move |a, b: &i32, c, d| {
                counter_inner.fetch_add(a * (*b) + c + d, Ordering::SeqCst);
            });
        let event: i32 = 5;
        handler(2i32, &event, 3, 4);
        assert_eq!(counter.load(Ordering::SeqCst), 17);
    }

    #[test]
    fn traced_listener_body_return_value_propagates() {
        // Same return-value pin as traced!, on the 4-arg variant.
        let handler = crate::traced_listener!("test:listener:retval", |a, b, c, d| {
            a * 1000 + b * 100 + c * 10 + d
        });
        let result: i32 = handler(1i32, 2, 3, 4);
        assert_eq!(result, 1234);
    }
}
