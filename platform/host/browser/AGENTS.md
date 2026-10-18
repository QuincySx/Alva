# 横向支撑 · Owned browser host
> Explicit Chrome process, private temporary profile and actual cleanup ownership.

## 地位
Native process/CDP assembly; observer and prior-risk callback are supplied by its owner, not invented storage or approval policy.
## 逻辑
New validates and copies an absolute approved executable/cwd/environment without startup. Start reserves a unique instance and new private directory, requires prior durable-risk admission, then starts platform/system/process with a fresh profile/home, no inherited credentials, no extensions/sync and loopback random debugging port. Only that process's exact debugger URL is connected. A short map lock never spans callbacks/startup/cleanup; per-instance startup cancellation/readiness and cleanup admission prevent late start or false stop. Complete bounded process lines or explicit partial evidence are observed with backpressure. Observer failures are fatal even when their private cause contains os.ErrClosed. CleanupComplete independently proves all process/protocol/I/O/profile cleanup, not that journal failures disappeared.
## 约束
No user-browser connection, existing profile, executable discovery, program installation, arbitrary argv, ambient credentials, automatic --no-sandbox or side-effect replay. Headed/headless/proxy are immutable explicit host choices; proxies cannot embed credentials. Same-owner private roots use Unix UID/modes or Windows protected DACLs; other platforms fail closed. Native Windows acceptance remains separate from compilation. Four instances,1MiB/line,4MiB process transcript/instance, startup≤1min. Close requests cancellation even during prior admission, waits for actual native group reap and I/O completion, then removes only its exact newly created temporary directory; timeout retains unfinished resources for retry. Protocol disposal is not proof of process cleanup. Chrome's own sandbox is retained but this host does not promise an OS filesystem/network sandbox or resource quota.
## 业务域清单
| 文件 | 职责 |
|---|---|
| manager.go | Explicit config, prior-start admission, owned process, bounded I/O observer and real cleanup |
| private_unix.go | Same-user0700 nonsymlink temporary-root proof |
| private_windows.go | Protected current-user-only DACL临时根检查；profile/缓存/home变量按Windows大小写规则隔离 |
| private_other.go | Explicit fail-closed unsupported ACL proof on other platforms |
| manager_test.go | Actual private Chrome all-verb/CDP/image/source tests, prior-admission failure, cancellation/observer error and exact cleanup |
