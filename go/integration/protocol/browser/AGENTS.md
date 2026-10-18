# 横向支撑 · Browser CDP
> Explicit typed operations over a pinned Chrome DevTools connection, without process discovery or product policy.

## 地位
Protocol adapter over chromedp v0.21.0/cdproto v0.157.8. No SDK, storage, user-profile or authorization dependency.
## 逻辑
Connect accepts the exact owned loopback browser WebSocket only and disables URL discovery. The retained first-Run context owns each target receiver; per-operation cancellation never kills receivers after a successful action. Navigation opens a new tracked tab and waits for load; input uses real CDP mouse/key/text dispatch. Snapshot returns actual text/HTML or explicitly heuristic cloned-DOM readability. Screenshot captures actual requested viewport/full-page/element scope and PNG/JPEG encoding with verified dimensions.
## 约束
No executable search, browser/profile discovery, account import, installation, arbitrary JavaScript interface, files, media understanding or storage. Page URLs are explicit HTTP/HTTPS/about:blank, without embedded credentials; no claim this is an OS or network sandbox. At most16 tracked tabs, selector2KiB, typed text64KiB, snapshot4MiB UTF8 without successful truncation, screenshot20MiB/32Mi pixels/16384 dimension. Owner must close both protocol and process; protocol Close initiates cancellation before waiting for an in-flight operation lock. Third-party transport itself accepts up to256MiB per CDP message; application limits are not a claim to bound Chrome DOM/network memory, untracked page-created targets or OS resource usage. Full hostile-page transport/resource hardening remains explicit future work.
## 业务域清单
| 文件 | 职责 |
|---|---|
| client.go | Exact endpoint, retained target contexts, new-tab navigation, actual interaction and metadata |
| content.go | Bounded source extraction and real screenshot scope/format/dimension evidence |
| client_test.go | No-browser/no-network invalid capability and endpoint regressions |
