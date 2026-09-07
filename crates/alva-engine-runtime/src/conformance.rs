// INPUT:  crate::{EngineRuntime, RuntimeEvent, RuntimeRequest, RuntimeError, StreamContract, validate_event_stream}
// OUTPUT: conformance harness — Scenario + assert_conforms
// POS:    Test-support helper that collects an engine's execute() stream and
//         checks it against the multi-engine stream contract plus scenario
//         invariants. Gated behind the `test-support` feature so it never
//         ships in production crates.
//
// ticket 16: the contract tests historically sat in mapping.rs (mapper-level)
// while the violations (missing Completed, fatal-then-more-events) happen in
// the adapter's execute() select-loops. This harness runs the whole execute()
// boundary, which is where regressions actually land.

use futures_util::StreamExt;

use crate::{
    validate_event_stream, EngineRuntime, RuntimeError, RuntimeEvent, RuntimeRequest,
    StreamContract,
};

/// A canned end-to-end scenario for one `execute()` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scenario {
    /// A normal run must satisfy the hard contract and close with Completed.
    Normal,
    /// The engine must surface a spawn failure as exactly
    /// `Error { recoverable: false }` + `Completed { result: None }`.
    SpawnFailure,
    /// Killed mid-run: the stream must terminate with a non-success pair.
    KilledMidRun,
    /// EOF without a result: a terminal error + Completed{None}, never a success.
    EofWithoutResult,
    /// A fatal Error must not be followed by further non-Completed events.
    OutputAfterFatal,
}

/// Collect the full `execute()` stream for one request.
///
/// Requires `futures-util` on the caller side for `StreamExt::next`; the
/// harness itself only needs the bare `Stream` from `futures-core`.
pub async fn collect_stream(
    engine: &dyn EngineRuntime,
    request: RuntimeRequest,
) -> Result<Vec<RuntimeEvent>, RuntimeError> {
    let stream = engine.execute(request)?;
    let mut events = Vec::new();
    futures_util::pin_mut!(stream);
    while let Some(event) = stream.next().await {
        events.push(event);
    }
    Ok(events)
}

/// Fetch soft warnings for a collected sequence without failing the run.
pub fn soft_warnings_for(events: &[RuntimeEvent]) -> Vec<crate::SoftStreamWarning> {
    StreamContract::check_soft(events).unwrap_or_default()
}

/// Assert one scenario against an engine's `execute()`.
pub async fn assert_conforms(
    engine: &dyn EngineRuntime,
    request: RuntimeRequest,
    scenario: Scenario,
) {
    let events = collect_stream(engine, request)
        .await
        .expect("execute must return a stream (spawn failures become events)");
    assert_conforms_events(&events, scenario);
}

/// The event-sequence half of [`assert_conforms`], factored out so adapter
/// test modules can feed pre-collected sequences without a live engine.
pub fn assert_conforms_events(events: &[RuntimeEvent], scenario: Scenario) {
    if let Err(err) = validate_event_stream(events) {
        panic!("scenario {scenario:?} violated the stream contract: {err}\n{events:#?}");
    }
    match scenario {
        Scenario::Normal => {
            assert!(
                matches!(events.last(), Some(RuntimeEvent::Completed { .. })),
                "Normal must end with Completed: {events:#?}"
            );
        }
        Scenario::SpawnFailure => {
            assert_eq!(
                events.len(),
                2,
                "SpawnFailure must be exactly Error + Completed: {events:#?}"
            );
            assert!(matches!(
                events[0],
                RuntimeEvent::Error {
                    recoverable: false,
                    ..
                }
            ));
            assert!(matches!(
                events[1],
                RuntimeEvent::Completed { result: None, .. }
            ));
        }
        Scenario::KilledMidRun => {
            assert!(
                matches!(
                    events.last(),
                    Some(RuntimeEvent::Completed { result: None, .. })
                ),
                "KilledMidRun must end with Completed{{result:None}}: {events:#?}"
            );
            // Adapting tools closed is Hub's synthesis job (ticket 12); the
            // adapter's contract duty ends at a terminal non-success pair.
        }
        Scenario::EofWithoutResult => {
            assert!(
                matches!(
                    events.last(),
                    Some(RuntimeEvent::Completed { result: None, .. })
                ),
                "EofWithoutResult must end with Completed{{result:None}}: {events:#?}"
            );
        }
        Scenario::OutputAfterFatal => {
            if let Some(index) = events.iter().position(|event| {
                matches!(
                    event,
                    RuntimeEvent::Error {
                        recoverable: false,
                        ..
                    }
                )
            }) {
                for after in &events[index + 1..] {
                    assert!(
                        matches!(after, RuntimeEvent::Completed { .. }),
                        "no event may follow a fatal Error except Completed: {events:#?}"
                    );
                }
            }
        }
    }
}
