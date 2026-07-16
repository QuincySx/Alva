# `run_script` QuickJS contract

`run_script({ "script": "..." })` evaluates one JavaScript source string in a fresh QuickJS
runtime inside the WASIp1 worker. It is global strict script code, not a module. The return value is
JSON text with `result` (the JavaScript string coercion of the final expression) and `output` (lines
captured by `print`). A JavaScript or binding failure is returned to the model as an error tool
result, without terminating the agent.

## Runtime boundary

- ECMAScript built-ins are available. Node APIs, npm, CommonJS `require`, ES module `import`,
  network APIs, subprocesses, and ambient host state are absent.
- Each call gets a fresh runtime, a 2-second QuickJS interrupt deadline, a 32 MiB QuickJS heap
  limit, and a 512 KiB QuickJS stack limit. The runtime uses rquickjs's size-tracking Rust
  allocator because WASI libc cannot report allocation sizes accurately enough for heap limits.
- The host independently applies the enclosing worker's `RunLimits` (default: 30 seconds and
  256 MiB WebAssembly linear memory) as a last-resort epoch/memory trap.
- Every path goes through the guest `WasiFs` adapter. Relative paths start at the agent workspace;
  absolute paths remain subject to the job's WASI preopens. Bindings do not add path authority.
- Text functions require UTF-8. Binary file APIs are intentionally out of scope for Ticket 06.

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

All functions are synchronous because QuickJS executes on the worker's single guest thread. There
is no loader or module resolution hook to extend this surface.
