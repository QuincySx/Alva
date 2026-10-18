# L7 · Read-only Devtools MCP
> Explicit authenticated Hub HTTP inspection exposed over owned stdio MCP.

## 地位
Product diagnostic bridge, outside SDK and model/Agent execution; uses official Go MCP SDK pinned v1.8.0 without exporting its DTOs into SDK.

## 逻辑
Explicit canonical numeric loopback HTTP address + separately supplied token. Construction/listing is cold. One read-only tool selects bounded session discovery, product history metadata, typed Inspector index or explicit 2KiB record text; actual Hub service performs owner/branch/typed-child authorization. History projects to previews and references before returning.

## 约束
No mutation, raw settings/credentials, arbitrary HTTP/host path, redirects, ambient proxies, model calls, agent restore, SSE polling or replay. Four concurrent reads, 5s/request, 1MiB input/HTTP response and 32KiB MCP result. Input exact JSON fields/Unicode, native child IDs never accepted. Official MCP handles framing, negotiated versions and request cancellation; stdio EOF/owner context closes the session and joins handlers. Token never appears in arguments, output, logs or stored configuration; CLI takes only explicit read-only fd3..10. Inspector content remains untrusted evidence.

## 业务域清单
| 文件 | 职责 |
|---|---|
| service.go | Canonical Hub client, strictly read-only route mapping, bounded metadata/provenance projection and real MCP serving |
| service_test.go | Actual Hub HTTP + official client/server stdio, record ownership, no execution/mutation and cancellation regression |
