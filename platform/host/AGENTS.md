# 横向支撑 · Host 执行所有权
> UI-independent native root-run ownership and bounded observation.

## 地位
Native capability consumes the harness Runner; no UI, provider discovery or product-state authority.
## 逻辑
Controller retains one Runner across sequential input batches. Start creates controller-owned cancellation; Subscribe only observes; Stop/Close explicitly cancel. Snapshot Text is the current streamed draft or latest actual no-tool-call committed plain response after AfterModel edits; reasoning/tool prefaces are not final answers. Committed Message/ContentBlock replay is deep-cloned. Snapshot/replay are bounded ephemeral presentation, not durable native or Hub records.
ApprovalBroker.PendingSnapshot is a bounded trusted-host-only metadata snapshot for recursive child approvals. Products must filter using their actual active root and verified typed ancestry; display/respond must retain actual child session IDs, never relabel or reuse parent grants. Snapshot callbacks never run under broker locks.
QuestionBroker is a separate real human-clarification rendezvous, never an approval channel. Its bounded deep-cloned pending snapshot is trusted-host-only; product filtering must use the same claimed-root/typed-ancestry discipline, and RespondQuestion retains actual native session/question identity. Canceled/expired questions reject late answers; no ambient stdin reader is spawned.
## 约束
Admission rejects nil/typed-nil Runner before any goroutine starts. Single active root, 16 retained runs, max 4096 replay events and 1 MiB replay bytes/run, 512 KiB/event, 1 MiB assistant output/run, 8 observers/run and 8 queued events/observer. Slow consumers close and must refresh; subscriptions never block model/tool execution. Event/text overflow fails explicitly. Start's caller context governs admission only. Actual Runner result determines terminal status; late Stop cannot relabel success or independent failure. Pure cancellation is cancelled; cancellation joined with settlement/flush failure is failed with retained error evidence. Close waits for actual run exit. No UI framework lifecycle owns session facts; these bounds do not bound the separate retained SDK session history.
## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Controller | controller.go | Root execution, explicit cancellation, bounded replay and immutable snapshots |
| Approval broker | approval.go | Bounded session/run/call-bound HITL rendezvous; no automatic approval |
| Pending admission | pending.go | Active root run checks for opt-in PendingService interjections |
| Pending regressions | pending_test.go | Real model/tool/model injection, exactly-once facts and late/foreign admission denial |
| Approval regressions | approval_test.go | Explicit decisions, foreign scope, expiry, Close and pending limits |
| Human questions | questions.go / questions_test.go | Bounded real clarification/selection wait, ownership/late-answer rejection and Close, strictly independent from approvals |
| Lifecycle regressions | controller_test.go | Detached/slow observers, nil/typed-nil Runner and cancelled admission, actual Stop/Close and state isolation |
| Foreground commands | command/ | Explicit bounded stdin/stdout/stderr, tail-preserving capture and actual process reap |
| Owned browser host | browser/ | Explicit Chrome launch, private fresh profiles, prior risk callback and real process/CDP cleanup |
