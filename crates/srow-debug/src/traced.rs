/// Wraps a GPUI event handler (3 params: event, window, cx) with tracing.
/// In release builds, this compiles to just the handler — zero overhead.
///
/// Usage:
///   div().on_click(srow_debug::traced!("send_btn:click", move |_, _, cx| { ... }))
///   div().on_click(srow_debug::traced!("send_btn:click", |event, window, cx| { ... }))
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
///   .on_click(cx.listener(srow_debug::traced_listener!("send_btn:click", |this, event, window, cx| { ... })))
///   .on_click(cx.listener(srow_debug::traced_listener!("send_btn:click", |this, _: &ClickEvent, _, cx| { ... })))
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
