# L7 · Hub CLI
> Independent headless command/REPL over the same bootstrap/product authority as Hub desktop/server.

## 地位
Process entrypoint, separate from direct alva CLI. No second journal, runtime, old-data import or ambient account discovery.
## 逻辑
Explicit data-dir/provider/approved executable snapshots. Cold sessions/history/handoff do not create a session or run an engine. New prompts explicitly create or resume one durable Hub session; manual engine switching uses Store.Switch and the same retained branches/handoff. REPL commands go to stderr, stdout receives only the actual successful final answer after execution owner cleanup. A native answer is read from its exact committed qualified record, avoiding the small product-display preview limit.
## 约束
API key only via separate explicit regular/pipe Unix fd3..10 or portable --api-key-file; the latter reads an existing current-owner private regular file without symlink/hardlink/discovery or creation. Windows rejects Unix fd input rather than treating a CRT slot as a HANDLE. Sources are mutually exclusive; secret bodies never enter argv/environment. Stdin belongs to REPL/one-call HITL. Default permission deny; ask requires REPL and actual request-ID confirmation. No auto-allow or silent external bypass. Prompt≤32KiB, command line≤64KiB, commands≤1024/1MiB, final answer≤8MiB, per-Turn timeout1s..30min and owned cancellation/Close. Input is explicitly owned; non-Windows scanner uses settled Close cancellation while Windows file/pipe polls without closing the borrowed handle mid-read; no detached reader; disk/terminal OS I/O is not claimed hard realtime. Terminal control characters are escaped. Startup/EOF/observation do not execute or replay pending turns or automation. A process exit closes its own application background resources.
## 业务域清单
| 文件 | 职责 |
|---|---|
| main.go | Explicit grammar/configuration, private application owner, cold commands, resume/engine selection and REPL |
| turn.go | Actual Runtime turn admission, scoped HITL/timeout/cancel, bounded final-answer reconstruction and stdout errors |
| input_windows.go / input_other.go | Windows文件/pipe不readahead的真正取消分发，其他平台保留既有owned scanner，不靠借用handle Close伪造中止 |
| input.go | Owned bounded command lines and terminal-safe output using the shared OS text primitive, no unjoined reader goroutine |
| main_test.go | Real local SDK/provider resume, cold reads/REPL/switch, invalid configuration, permissions, cancellation and output ownership |

Windows *os.File输入改用lineinput的regular-file/PeekNamedPipe逐行传输，不再依靠Close中断同步Scanner。非Windows和finite owned reader保持原scanner/Close owner。行/累计预算、Turn/审批政策不合并；console在读取前明确拒绝。
