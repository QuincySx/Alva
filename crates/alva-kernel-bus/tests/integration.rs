//! Integration test — verifies the full bus round-trip across simulated layers.

use alva_kernel_bus::{Bus, BusEvent, BusHandle, BusWriter, StateCell};
use std::sync::Arc;

// -- Shared event definitions --

#[derive(Clone, Debug, PartialEq)]
struct ToolExecuted {
    tool_name: String,
    session_id: String,
}
impl BusEvent for ToolExecuted {}

#[derive(Clone, Debug, PartialEq)]
struct TokenUsageUpdated {
    used: usize,
    total: usize,
}
impl BusEvent for TokenUsageUpdated {}

// -- Shared capability trait --

trait TokenAccounting: Send + Sync {
    fn current_usage(&self) -> (usize, usize);
}

struct MockTokenAccounting;
impl TokenAccounting for MockTokenAccounting {
    fn current_usage(&self) -> (usize, usize) {
        (3000, 4096)
    }
}

// -- Simulated layers --

fn init_engine_layer(bus: &BusWriter) {
    bus.provide::<dyn TokenAccounting>(Arc::new(MockTokenAccounting));
}

fn init_tool_layer(bus: &BusHandle) {
    bus.emit(ToolExecuted {
        tool_name: "shell".into(),
        session_id: "sess-1".into(),
    });
}

async fn context_layer_subscribe(bus: &BusHandle) -> ToolExecuted {
    let mut rx = bus.subscribe::<ToolExecuted>();
    rx.recv().await.unwrap()
}

#[tokio::test]
async fn full_bus_round_trip() {
    let bus = Bus::new();

    let engine_writer = bus.writer();
    let tool_handle = bus.handle();
    let context_handle = bus.handle();

    init_engine_layer(&engine_writer);

    let context_task = {
        let h = context_handle.clone();
        tokio::spawn(async move { context_layer_subscribe(&h).await })
    };

    tokio::task::yield_now().await;

    init_tool_layer(&tool_handle);

    let received = context_task.await.unwrap();
    assert_eq!(received.tool_name, "shell");
    assert_eq!(received.session_id, "sess-1");

    let accounting = context_handle.require::<dyn TokenAccounting>();
    assert_eq!(accounting.current_usage(), (3000, 4096));
}

#[tokio::test]
async fn state_cell_cross_handle() {
    let bus = Bus::new();
    let w = bus.writer();
    let h2 = bus.handle();

    let cell = StateCell::new(0u32);
    w.provide(Arc::new(cell.clone()));

    let cell_ref = h2.require::<StateCell<u32>>();
    let mut rx = cell_ref.watch();

    cell.set(42);
    assert_eq!(cell_ref.get(), 42);
    assert_eq!(rx.recv().await.unwrap(), 42);
}

#[test]
fn concurrent_provide_and_get() {
    use std::thread;

    let bus = Bus::new();
    let writers: Vec<_> = (0..10).map(|_| bus.writer()).collect();

    let threads: Vec<_> = writers
        .into_iter()
        .enumerate()
        .map(|(i, w)| {
            thread::spawn(move || {
                #[derive(Debug)]
                struct Val(usize);
                w.provide(Arc::new(Val(i)));
            })
        })
        .collect();

    for t in threads {
        t.join().unwrap();
    }
}
