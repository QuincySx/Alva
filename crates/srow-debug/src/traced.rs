/// Wraps a GPUI event handler (3 params: event, window, cx) with tracing.
/// In release builds, this compiles to just the handler — zero overhead.
///
/// Usage:
///   div().on_click(srow_debug::traced!("send_btn:click", |event, window, cx| { ... }))
///   div().on_click(srow_debug::traced!("send_btn:click", move |_, _, cx| { ... }))
#[cfg(debug_assertions)]
#[macro_export]
macro_rules! traced {
    ($name:expr, $handler:expr) => {{
        let __traced_name: &'static str = $name;
        let __traced_handler = $handler;
        move |event, window, cx| {
            ::tracing::info!(target: "gpui_event", name = __traced_name, "event");
            (__traced_handler)(event, window, cx)
        }
    }};
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
#[cfg(debug_assertions)]
#[macro_export]
macro_rules! traced_listener {
    ($name:expr, $handler:expr) => {{
        let __traced_name: &'static str = $name;
        let __traced_handler = $handler;
        move |this, event, window, cx| {
            ::tracing::info!(target: "gpui_event", name = __traced_name, "event");
            (__traced_handler)(this, event, window, cx)
        }
    }};
}

#[cfg(not(debug_assertions))]
#[macro_export]
macro_rules! traced_listener {
    ($name:expr, $handler:expr) => {
        $handler
    };
}
