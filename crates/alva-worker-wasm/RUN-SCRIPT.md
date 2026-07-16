# `run_script` QuickJS contract

`run_script({ "script": "..." })` evaluates one JavaScript source string in a fresh QuickJS
runtime inside the WASIp1 worker. It is global strict script code, not a module. The return value is
JSON text with `result` (the JavaScript string coercion of the final expression) and `output` (lines
captured by `print`). A JavaScript or binding failure is returned to the model as an error tool
result, without terminating the agent.

## Runtime boundary

- ECMAScript built-ins are available. Node APIs, npm, CommonJS `require`, ES module `import`,
  subprocesses, and ambient host state are absent. The only network surface is the synchronous
  `fetch` binding below; its traffic is executed and authorized by the native host.
- Each call gets a fresh runtime, a 10-second QuickJS interrupt deadline, a 32 MiB QuickJS heap
  limit, and a 512 KiB QuickJS stack limit. The runtime uses rquickjs's size-tracking Rust
  allocator because WASI libc cannot report allocation sizes accurately enough for heap limits.
- The host independently applies the enclosing worker's `RunLimits` (default: 30 seconds and
  256 MiB WebAssembly linear memory) as a last-resort epoch/memory trap.
- Every path goes through the guest `WasiFs` adapter. Relative paths start at the agent workspace;
  absolute paths remain subject to the job's WASI preopens. Bindings do not add path authority.
- Text functions require UTF-8. Binary file APIs are intentionally out of scope for Ticket 06.
- `fetch` authority is job-local and fail-closed: no `--allow-domain` values means every URL is
  rejected. This grant is independent from the host-only LLM provider channel.

## File and output bindings

| Function | Result and semantics |
|---|---|
| `readFile(path)` | Read a UTF-8 file and return a string. |
| `writeFile(path, content)` | Create/replace a file, creating parent directories; return UTF-8 byte count. |
| `appendFile(path, content)` | Create/append a file, creating parent directories; return appended byte count. |
| `exists(path)` | Return whether the path is visible through WASI. |
| `remove(path)` | Remove one file or a directory tree. |
| `rename(from, to)` | Rename a path, creating destination parents. Both paths need authority. |
| `mkdir(path)` | Recursively create a directory. |
| `readDir(path = ".")` | Return sorted `{name, isDir, size}` entries, non-recursively. |
| `stat(path)` | Return `{isFile, isDir, size, readonly}`. |
| `glob(pattern)` | Return sorted workspace-relative paths matching a glob pattern. |
| `copyFile(from, to)` | Copy one file, creating destination parents; return copied byte count. |
| `readJson(path)` | Read UTF-8 text and apply `JSON.parse`. |
| `writeJson(path, value, space = 2)` | Apply `JSON.stringify` and write the result. |
| `print(...values)` | Append one line to the tool result's `output` array; objects use `JSON.stringify`. |
| `fetch(url, init = {})` | Synchronously return `{status, ok, headers, body}`. `method`, object/tuple-array `headers`, and UTF-8 string `body` are supported. Policy/network/UTF-8 failures throw a catchable JS exception. |

All functions are synchronous because QuickJS executes on the worker's single guest thread. There
is no loader or module resolution hook to extend this surface. `fetch` deliberately does not return
a Promise: the WASIp1 guest has no event loop and its host import is blocking.

## Domain matching and redirects

- A plain entry such as `example.com` matches that host exactly, case-insensitively. It does not
  match `www.example.com`.
- `*.example.com` matches one or more subdomain labels, but not the apex `example.com`.
- Ports do not participate in matching. A rule grants the host on any explicit/default port.
- IPv4 and IPv6 literals are allowed only as exact entries; IP wildcards are invalid.
- Entries are ASCII (IDNA names must use punycode) and cannot contain a scheme, path, credentials,
  query, fragment, or port.
- Only `http` and `https` URLs are accepted. Embedded URL credentials are rejected.
- The host disables the HTTP client's automatic redirects. It resolves `Location` itself, checks
  the target against the same allowlist before every hop, and stops after 10 redirects. Therefore
  an allowed origin cannot redirect through to an unlisted host.

The host caps request bodies at 1 MiB and decoded response bodies at 4 MiB; both JSON ABI messages
are separately bounded before guest memory allocation. The 10-second script default is unchanged
by Ticket 07. A blocking network call can consume much of that budget, so callers with deliberate
slow endpoints should tune the existing script/worker limits rather than relying on a larger
global default.
