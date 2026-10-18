# Alva AEP Python author SDK

Optional plugin-side client for the maintained Go Alva host. The Alva implementation remains Go;
Python is required only when explicitly running a Python plugin or its interoperability fixture.
The host loads an explicitly approved bundle/interpreter; it does not scan global plugin directories.

## Local use

Use this checked-in source explicitly, or install from this local directory in your own environment:

```sh
python3 -m pip install /absolute/path/to/Alva/integration/protocol/aep/python
```

No published package or automatic installation is assumed. The Go acceptance fixture supplies an
explicit PYTHONPATH, disables site/user imports and bytecode writes, and executes only its local fixture.

## Author surface

Import Plugin, Message, ToolCall, ToolResult, run and decorators from alva_sdk.
The seven events cover agent start/end, user input, model start/end and tool before/after;
@tool declares an executable plugin capability. Handlers may return explicit message, argument,
response or result mutations. The real Go loader test checks those wire actions against actual
pre-intent arguments, model context and committed tool results, rather than a Python-only mock.

Host helpers use expiring event handles and explicit manifest/host capability grants.
Log/notification/metric calls record host facts; they do not prove UI notification delivery or
external telemetry export. Memory/approval helpers are not supported by the current Go host.
The subprocess has native host access after separate approval; it is not an OS sandbox.

See [the Go AEP contract](../AGENTS.md), [loader ownership](../../../../extension/loader/AGENTS.md)
and [the local example](./examples/shell_guard.py). Author SDK source/packaging is carried from the
Rust archive, preserving the original MIT declaration in pyproject.toml. No Rust code is retained.
