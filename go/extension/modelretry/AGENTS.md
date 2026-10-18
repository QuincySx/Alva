# L4 · Explicit model retry
> Optional bounded private buffering before publishing one complete response.

## 地位
Opt-in Plugin/Middleware above SDK; no default harness installation or external dependency.

## 逻辑
Config explicitly supplies 1..8 attempts, 1 byte..8 MiB encoded buffer, 1..65536 chunks, a positive total timeout of at most five minutes and a nonnil Retryable classifier. Every sequential next uses a fresh request copy, the same captured model and its own normal kernel admission. Failed attempts stay private; successful chunks must pass shared wire protocol/finish validation before one selected stream is published with index, reasoning, usage and metadata intact. Each attempt receives a fresh bounded buffer; no prior attempt body is retained for replay.

## 约束
Cancellation/deadline, typed SessionError and this component's buffer/protocol failures cannot be retried even if the classifier permits them. Total encoded JSON bytes and chunk count both apply, including empty text/usage; raw fields are checked before encoding/retention, with maximum nesting64 and65536 visited values per chunk. The byte limit is not a physical heap ceiling. Deadline covers all attempts/publication and requests cancellation; synchronous next/emit really join even if they ignore cancellation. Publication errors never retry. There is no provider fallback, execution replay, persistence, backoff or built-in classifier; callers explicitly accept possible additional model billing/transport side effects. Trusted surrounding wrappers may produce their own effects/output, so this component does not claim to undo those or validate opaque internal transports.

## 业务域清单
| 文件 | 职责 |
|---|---|
| plugin.go | Explicit config/Plugin, sequential retry, bounded private stream validation and actual synchronous join |
| plugin_test.go | Real SDK partial-failure isolation, classifier exclusions, byte/chunk/protocol bounds, metadata copies, admission and deadline join regressions |
