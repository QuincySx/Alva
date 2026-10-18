# Round 10 adapter spec review

Scope: HEAD 764864da, R94 36d44ec9, R95 c5e37703. Read adapter/runtime AGENTS and originating cards. No production changes or real model requests. Cargo execution delegated to parent for shared target scheduling.

## Confirmed static finding: Pi overflow compaction recovery is prematurely fatal

P1/P2; R95 follow-up. `crates/alva-engine-adapter-pi/src/mapping.rs:118–126` treats every `agent_end(willRetry=false)` with a pending assistant error as definitive failure. Adapter `adapter.rs:181–182` then stops and drops its kill-on-drop process.

Pi 0.85.1 has a separate legitimate recovery path not represented by this willRetry flag: context overflow errors deliberately return false from `_isRetryableError`; after agent_end, `_handlePostAgentRun` calls `_checkCompaction`, which compacts and requests continuation. Therefore `error → agent_end(false) → compaction_end(willRetry=true) → successful answer` is a recoverable real wire sequence, not noise after a committed terminal. The adapter kills it before compaction can complete. Ordinary 503 auto-retry fixtures cannot cover this case.

Local installed primary source (package.json version 0.85.1):
`/Users/smallraw/.local/share/mise/installs/npm-earendil-works-pi-coding-agent/latest/node_modules/@earendil-works/pi-coding-agent/`:

- `dist/core/agent-session.js:386,429–440`: forward agent_end with willRetry computed solely by the retry classifier.
- `dist/core/agent-session.js:2248–2253`: explicitly exclude context overflow from retry classifier because compaction handles it.
- `dist/core/agent-session.js:772–810`: await agent run, then post-run retry/compaction and agent.continue loop.
- `dist/core/agent-session.js:1658–1695`: overflow with stopReason=error selects compact-and-retry.
- `dist/core/agent-session.js:1850–1863`: emits compaction_end(willRetry=true), returns continuation.
- `dist/core/agent-session.js:347–352,784`: agent_settled is emitted after the complete post-run loop.
- `dist/modes/print-mode.js:85–96`: stdout receives session events through toJsonEvent.

Independent probe: `evidence/adapters/main.rs` executes the real PiAdapter against local scripts, with healthy control and this overflow recovery sequence. Parent will run and archive output; no dynamic result claimed here yet.

Acceptance: healthy ordinary completion, 503 retry success/exhaustion, overflow compaction recovery, failed/aborted compaction, and EOF while recovery is in progress. Use the native whole-request settlement boundary or equivalent validated state, rather than treating auto-retry=false as proof no other recovery remains. Preserve one Completed and bounded cancellation.

## R94 disposition / other limits

Current code does **not** break immediately upon native terminal; it continues to EOF. Consequently there is no evidence here of a newly introduced normal-success terminal break killing session persistence. It ignores trailing decoding errors/cancel as the accepted R94 policy. Do not file an early-kill issue based on a nonexistent immediate break.

The new R94 late-cancel tests place answer after terminal, so they demonstrate the chosen adapter policy rather than authentic native wire order. The concrete acceptance behavior is still covered by the independent round9 malformed-tail probe; this test limitation alone is not a new product bug.

R95's ordinary retry and explicit AutoRetryEnd(false) branches appear consistent within those paths. Its expanded fatal boundary is incomplete specifically for compaction recovery above.
