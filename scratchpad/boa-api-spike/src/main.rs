// INPUT:  boa_engine 0.21.1, futures executor, std time/future primitives
// OUTPUT: Executable evidence for native bindings, script-only modules, and budgeted cancellation
// POS:    Disposable JS-engine selection spike; not part of the Alva workspace or production worker.

use std::future::{poll_fn, Future};
use std::pin::pin;
use std::task::Poll;
use std::time::{Duration, Instant};

use boa_engine::{js_string, Context, JsResult, JsValue, NativeFunction, Script, Source};
use futures::executor::block_on;

fn double(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::new(args[0].to_number(context)? * 2.0))
}

fn main() {
    let mut context = Context::default();
    context
        .register_global_builtin_callable(
            js_string!("double"),
            1,
            NativeFunction::from_fn_ptr(double),
        )
        .expect("register native function");
    let value = context
        .eval(Source::from_bytes("double(21)"))
        .expect("call native function");
    println!("native binding result={}", value.display());
    let require_type = context
        .eval(Source::from_bytes("typeof require"))
        .expect("check CommonJS absence");
    let import_error = context
        .eval(Source::from_bytes("import value from 'missing';"))
        .expect_err("static import must be invalid in script mode");
    println!(
        "module boundary require={}, static_import_error={}",
        require_type.display(),
        import_error
    );

    let mut context = Context::default();
    let script = Script::parse(Source::from_bytes("for (;;) {}"), None, &mut context)
        .expect("parse infinite loop");
    let started = Instant::now();
    let deadline = Duration::from_millis(20);
    let outcome = {
        let evaluation = script.evaluate_async_with_budget(&mut context, 64);
        let mut evaluation = pin!(evaluation);
        block_on(poll_fn(|cx| {
            if started.elapsed() >= deadline {
                return Poll::Ready("cancelled");
            }
            match evaluation.as_mut().poll(cx) {
                Poll::Ready(_) => Poll::Ready("unexpectedly completed"),
                Poll::Pending => {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
            }
        }))
    };
    println!(
        "infinite loop outcome={outcome}, elapsed_ms={}",
        started.elapsed().as_millis()
    );
    drop(context);

    let mut next_context = Context::default();
    let value = next_context
        .eval(Source::from_bytes(
            "JSON.stringify([40 + 2, 'agent continues'])",
        ))
        .expect("run after cancelled script");
    println!("next fresh context result={}", value.display());
}
