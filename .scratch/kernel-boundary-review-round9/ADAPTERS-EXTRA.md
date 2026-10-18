# Round 9 adapter spec-axis review

Scope: R81 (`8c9b68eb`), R82 (`79bab61b`), R83 (`a5504407`), R74 (`9f163921`). Read per-crate src/AGENTS for Codex/Pi/Claude/Alva/runtime. No production changes. No network calls or real paid engine execution by this reviewer. Parent owns cargo execution and final output recording.

## A1 — fatal transport failure can retain a successful result (P2)

Locations: Codex adapter.rs:234–239,268–271; Pi adapter.rs:186–191,208–211.

After the newly recognized native terminal, adapters keep reading to EOF. A following stdout decoding failure sets `error_forwarded=true` and emits fatal Error, but final result selection consults `mapper.failed()` rather than that adapter-owned failure flag. Thus the last answer remains in Completed.result. Cancel after native terminal uses the same flag and has the same omission.

Probe: feed healthy session/message/native-terminal JSONL, then a non-UTF8 line (0xff). Real adapter execute paths are used. Healthy controls and both malformed-tail cases run before the final aggregate assertion. Native terminal acceptance itself is correct; this is composition with adapter-owned failure after that boundary.

Acceptance must choose a consistent boundary: either native completion commits success and trailing transport bytes are irrelevant, or continued reading can fail the run and must clear the final result. The test allows either. It only forbids fatal Error plus Completed(Some(answer)). Include a controlled cancel-after-terminal case if keeping the EOF-driven lifecycle; preserve one Completed and existing EOF/fatal behavior.

## A2 — Pi provisional assistant errors abort before native retry decision (P1/P2)

Locations: Pi mapping.rs:132–139,155–162 and adapter.rs:181–182.

Both assistantMessageEvent.error and message_end(stopReason=error) immediately become nonrecoverable RuntimeEvent::Error. Adapter stops on the first one. The subsequent agent_end(willRetry=true), auto_retry_start, successful retry answer and final agent_end(false) are never consumed. R82 adds native terminal recognition but does not implement its required retry semantics; the only mapper retry test feeds auto_retry_start in isolation and omits the preceding real error frames.

Wire-order evidence is local installed Pi **0.85.1**, package root:
`/Users/smallraw/.local/share/mise/installs/npm-earendil-works-pi-coding-agent/latest/node_modules/@earendil-works/pi-coding-agent/`.

- `dist/core/agent-session.js:360–440`: original message events forwarded at 386; agent_end gets willRetry from `_willRetryAfterAgentEnd`; message_end stores `_lastAssistantMessage`.
- `dist/core/agent-session.js:772–805`: `_runAgentPrompt` awaits original agent run before `_handlePostAgentRun`; line 793 prepares retry of the last error.
- `dist/core/agent-session.js:2286–2306`: `_prepareRetry` emits auto_retry_start after the original failing run.
- `dist/modes/print-mode.js:85–96`: JSON mode writes every session event to stdout.
- `dist/modes/json-event.js`: message_update projection retains assistantMessageEvent.error; other event types are unchanged.

Independent fixture mirrors that order: session → message_update(error=503) → message_end(error) → agent_end(true) → auto_retry_start → successful message_end → auto_retry_end(true) → agent_end(false). Expected: retry remains recoverable, final answer survives, one Completed. Also test non-retry final failure and EOF while waiting on a retry decision so buffering provisional errors cannot disguise an outage.

## Other dispositions / limits

- R83 now rejects fatal in Normal without requiring text from every valid success. Known Codex/Pi samples assert answer and token usage. No additional defect found there.
- R74 Claude Full normal fixture traverses the actual embedded bridge with controlled SDK messages and asserts result text. Five Full scenarios and Alva killed sample are concrete execute-boundary tests.
- Alva killed sample deliberately selects a tool-call continuation; its own comments acknowledge text-only natural completion misses cancel. This reviewer has not independently reproduced that path; not reported as an additional confirmed finding here.
- Pi real test is environment dependent and not opt-in: adapter.rs:555–562 defaults to a machine-specific provider/model and :569 only checks binary presence. Treat failures as a validation limitation; do not mislabel them R82 regressions or rerun paid calls for this review.

## Reproduction

`cargo run --offline --manifest-path .scratch/kernel-boundary-review-round9/evidence/adapters_extra/Cargo.toml`

Parent executed the probe with `--target-dir target`: compiled successfully, exit 101.
Both healthy controls: fatal=false, has_result=true. Both invalid-UTF8 tails: fatal=true,
has_result=true. Pi retry fixture: recoverable_retry_lost=true, only 503 fatal + Completed(None).
Aggregate assertion observed `(2, true)` rather than `(0, false)`. See evidence/RESULTS.md.
