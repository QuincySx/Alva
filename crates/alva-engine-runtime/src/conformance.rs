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
            // R52: a killed run must surface the kill as a fatal error — a
            // bare Completed{None} without Error is a contract violation, not
            // a conforming sample.
            assert!(
                events.iter().any(|event| matches!(
                    event,
                    RuntimeEvent::Error { recoverable: false, .. }
                )),
                "KilledMidRun must surface a fatal Error before the terminal: {events:#?}"
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
            // R52: same as KilledMidRun — the missing result must be reported
            // as a fatal Error, never a silently bare Completed{None}.
            assert!(
                events.iter().any(|event| matches!(
                    event,
                    RuntimeEvent::Error { recoverable: false, .. }
                )),
                "EofWithoutResult must surface a fatal Error before the terminal: {events:#?}"
            );
        }
        Scenario::OutputAfterFatal => {
            // R52: the scenario is vacuous without a fatal — a sample without
            // one must be rejected, not silently pass.
            assert!(
                events.iter().any(|event| matches!(
                    event,
                    RuntimeEvent::Error { recoverable: false, .. }
                )),
                "OutputAfterFatal sample must contain a fatal Error: {events:#?}"
            );
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

#[cfg(test)]
mod mutation {
    //! R52: red-green proofs that the harness REJECTS deliberately malformed
    //! samples — a scenario that cannot fail would be decoration, not a gate.

    use super::{assert_conforms_events, Scenario};
    use crate::RuntimeEvent;

    fn started() -> RuntimeEvent {
        RuntimeEvent::SessionStarted {
            session_id: "mutation".into(),
            model: None,
            tools: vec![],
        }
    }

    fn completed_none() -> RuntimeEvent {
        RuntimeEvent::Completed {
            session_id: "mutation".into(),
            result: None,
            usage: None,
        }
    }

    #[test]
    #[should_panic(expected = "must surface a fatal Error")]
    fn eof_without_fatal_is_rejected() {
        assert_conforms_events(&[started(), completed_none()], Scenario::EofWithoutResult);
    }

    #[test]
    #[should_panic(expected = "must surface a fatal Error")]
    fn killed_without_fatal_is_rejected() {
        assert_conforms_events(&[started(), completed_none()], Scenario::KilledMidRun);
    }

    #[test]
    #[should_panic(expected = "must end with Completed{result:None}")]
    fn eof_with_a_success_result_is_rejected() {
        let success = RuntimeEvent::Completed {
            session_id: "mutation".into(),
            result: Some("finished".into()),
            usage: None,
        };
        assert_conforms_events(&[started(), success], Scenario::EofWithoutResult);
    }

    #[test]
    #[should_panic(expected = "OutputAfterFatal sample must contain a fatal Error")]
    fn output_after_fatal_without_fatal_is_rejected() {
        let success = RuntimeEvent::Completed {
            session_id: "mutation".into(),
            result: Some("finished".into()),
            usage: None,
        };
        assert_conforms_events(&[started(), success], Scenario::OutputAfterFatal);
    }
}
