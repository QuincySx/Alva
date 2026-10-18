# 横向支撑 · Foreground command host
> Bounded explicit native-process input/output and actual cleanup, shared by optional command-based capabilities.

## 地位
Native implementation above platform/system/process, without SDK/plugin/product policy or configuration discovery.
## 逻辑
Caller approves/reports the exact process before Run. Input writer and both bounded readers run concurrently; all finish before returning. Streams drain before natural leader observation/reap so a fast process cannot lose its tail. EOF alone is not treated as exit. Cancellation/limits close the owned group and wait for actual cleanup.
## 约束
No shell inference, ambient credentials, background execution or replay. stdin ≤256KiB, each selected stream ≤2MiB. Over-budget output is an explicit incomplete prefix plus error, never a silently successful truncation. Exit code is a business result; I/O/cleanup failures are distinct. Caller owns durable facts, redaction, references, timeouts and authorization. This is process ownership, not an OS filesystem/network sandbox.
## 业务域清单
| 文件 | 职责 |
|---|---|
| command.go | Bounded full-duplex capture and actual foreground lifecycle |
| command_test.go | Real Go subprocess fast-exit tail, full-duplex, explicit env, overflow and EOF/cancel regressions |
