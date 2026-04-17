# AEP — Alva Extension Protocol (v0.1 draft)

> Wire protocol for dynamically-loaded plugins running as subprocesses.
> Authors write plugins in Python or JavaScript; the host loads them
> at runtime and exposes them through the existing `Extension` system.

## 0. Design Principles

1. **Extend MCP, do not replace it.** All `tools/*` methods reuse
   MCP semantics so SDKs can share code paths.
2. **JSON-RPC 2.0 over stdio**, newline-delimited. One message per
   line. Simpler to implement than LSP-style Content-Length framing.
3. **Plugin authors face the SDK, not the protocol.** The wire format
   is a Rust-side / SDK-side contract; plugin code only sees a
   language-native `Extension` base class plus decorators.
4. **Capability handles** — large host-owned objects (`AgentState`)
   never cross the process boundary in full. The plugin receives an
   opaque handle and calls back into the host on demand.
5. **Bidirectional JSON-RPC** — both sides can originate requests
   and notifications. Needed for plugin-to-host reverse calls during
   event dispatch.

---

## 1. Transport & Framing

| Aspect | Choice |
|---|---|
| Transport | Subprocess stdio |
| Host → plugin | `stdin` |
| Plugin → host | `stdout` |
| Plugin logs / errors | `stderr` (collected, forwarded to host log) |
| Framing | `\n`-delimited JSON |
| Encoding | UTF-8 |
| Direction | Full duplex, both sides may originate requests |

Request/response ids are strings of the form `"h-<seq>"` (host
originated) or `"p-<seq>"` (plugin originated) to avoid collisions
on the shared channel.

---

## 2. Lifecycle

```
┌───────────────┐                         ┌─────────────────┐
│     Host      │                         │     Plugin      │
│ (Rust,        │                         │ (Python/JS,     │
│  AEP loader)  │                         │  alva-sdk)      │
└───────────────┘                         └─────────────────┘
        │                                         │
        │ 1. spawn process                        │
        │─────────────────────────────────────────▶│
        │                                         │
        │ 2. initialize(req)                      │
        │─────────────────────────────────────────▶│
        │                                         │
        │ 3. initialize(result: manifest)         │
        │◀─────────────────────────────────────────│
        │                                         │
        │ 4. initialized(notification)            │
        │─────────────────────────────────────────▶│
        │                                         │
        │        ↔ normal RPC (bidirectional) ↔   │
        │                                         │
        │ 5. shutdown(req)                        │
        │─────────────────────────────────────────▶│
        │                                         │
        │ 6. shutdown(result: {})                 │
        │◀─────────────────────────────────────────│
        │                                         │
        │ 7. process exits                        │
        │ ◀─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─│
```

### 2.1 `initialize` — host → plugin

```json
{
  "jsonrpc": "2.0",
  "id": "h-1",
  "method": "initialize",
  "params": {
    "protocolVersion": "0.1.0",
    "hostInfo": { "name": "alva", "version": "0.x.y" },
    "hostCapabilities": {
      "stateAccess": ["messages", "metadata", "tool_calls"],
      "events": [
        "before_tool_call", "after_tool_call",
        "on_llm_call_start", "on_llm_call_end",
        "on_user_message", "on_agent_start", "on_agent_end"
      ],
      "hostApi": [
        "log", "notify", "request_approval", "emit_metric",
        "get_state", "memory.read", "memory.write"
      ]
    }
  }
}
```

### 2.2 `initialize` response — plugin → host

```json
{
  "jsonrpc": "2.0",
  "id": "h-1",
  "result": {
    "protocolVersion": "0.1.0",
    "plugin": {
      "name": "my-memory",
      "version": "0.1.0",
      "description": "A simple key-value memory extension",
      "author": "@someone"
    },
    "tools": [
      {
        "name": "remember",
        "description": "Remember a fact",
        "inputSchema": { "type": "object", "properties": { "key": {"type":"string"}, "value": {"type":"string"} } }
      }
    ],
    "eventSubscriptions": ["before_tool_call", "on_llm_call_start"],
    "requestedCapabilities": ["host:log", "host:get_state"]
  }
}
```

`requestedCapabilities` carries the plugin's explicit capability
declaration. **In v0.1 this is observation-only** — the host logs a
warning when the plugin calls a host method it did not declare, but
does not block. v0.2 will switch to strict enforcement.

### 2.3 `initialized` — host → plugin notification

No params. Signals the plugin may begin normal operation.

### 2.4 `shutdown` — host → plugin

Plugin has **3 s** to clean up and return `{}`. After that the host
sends SIGTERM; 2 s later, SIGKILL.

---

## 3. Host → Plugin — Event Dispatch

Events are **requests** (not notifications) because the plugin's
return value can influence host behaviour through the
[`ExtensionAction`](#36-extensionaction) enum.

### 3.1 `extension/before_tool_call`

```json
{
  "jsonrpc": "2.0",
  "id": "h-42",
  "method": "extension/before_tool_call",
  "params": {
    "stateHandle": "s-7",
    "toolCall": {
      "id": "call_abc123",
      "name": "shell",
      "arguments": { "command": "rm -rf /" }
    }
  }
}
```

Plugin responds with an `ExtensionAction`:

```json
{
  "jsonrpc": "2.0",
  "id": "h-42",
  "result": { "action": "block", "reason": "Destructive shell command" }
}
```

### 3.2 Event Catalogue (v0.1)

| Method | When | Legal Actions |
|---|---|---|
| `extension/before_tool_call` | Right before a tool runs | `continue` / `block` / `modify` / `replace_result` |
| `extension/after_tool_call` | After a tool returns | `continue` / `modify_result` |
| `extension/on_llm_call_start` | Before the LLM request | `continue` / `modify_messages` / `block` |
| `extension/on_llm_call_end` | After the LLM response | `continue` / `modify_response` |
| `extension/on_user_message` | New user message | `continue` / `modify` |
| `extension/on_agent_start` | Agent loop begins | `continue` / `block` |
| `extension/on_agent_end` | Agent loop ends | `continue` (others rejected as `INVALID_ACTION`) |

Finer-grained hooks (streaming tokens, tool schema resolution, etc.)
are deferred to v0.2.

### 3.3 Stateless Events

Plugins that only want to observe — `on_agent_end`, pure logging —
may return `{"action": "continue"}`. The SDK is free to use a
**notification** instead of a request when the handler has no return
value, avoiding the round trip.

### 3.4 `ExtensionAction`

```jsonc
// continue
{ "action": "continue" }

// block
{ "action": "block", "reason": "..." }

// modify (rewrite the triggering operation's arguments)
{ "action": "modify", "modified_arguments": {...} }

// replace_result (skip execution, return this result)
{ "action": "replace_result", "result": {...} }

// modify_messages (on_llm_call_start only)
{ "action": "modify_messages", "messages": [...] }

// modify_response (on_llm_call_end only)
{ "action": "modify_response", "response": {...} }

// modify_result (after_tool_call only)
{ "action": "modify_result", "result": {...} }
```

---

## 4. Plugin → Host — Reverse Calls (Capability Handles)

When handling an event, the plugin may call host APIs to read state,
log, request approval, etc. State access uses handles passed in the
event params.

### 4.1 `host/log`

```json
{
  "jsonrpc": "2.0",
  "id": "p-3",
  "method": "host/log",
  "params": { "level": "info", "message": "blocked", "fields": {} }
}
```

### 4.2 `host/state.get_messages`

```json
{
  "jsonrpc": "2.0",
  "id": "p-4",
  "method": "host/state.get_messages",
  "params": { "handle": "s-7", "limit": 10, "offset": 0 }
}
```

The `handle` **must** come from the currently-in-flight event's
params and is invalidated as soon as the host processes the response
to that event. Re-use after expiration returns
`error.code = -32000` (`HANDLE_EXPIRED`).

### 4.3 v1 Host API Catalogue

| Method | Purpose | Needs handle? |
|---|---|---|
| `host/log` | Log a line through the host logger | No |
| `host/notify` | Show a user-visible notification | No |
| `host/request_approval` | Block until user approves | No |
| `host/emit_metric` | Report a numeric metric | No |
| `host/state.get_messages` | Read agent message history | ✅ `stateHandle` |
| `host/state.get_metadata` | Read agent metadata | ✅ `stateHandle` |
| `host/state.count_tokens` | Estimate token count | ✅ `stateHandle` |
| `host/memory.read` | Read from current memory backend | No |
| `host/memory.write` | Write to current memory backend | No |

`host/memory.*` operate on whatever memory backend the host is
currently using. If a plugin wants to **be** the memory backend, it
declares `provides: ["memory"]` in its manifest — the host then routes
its own memory reads/writes to that plugin. (Plumbing for this lands
in phase 6.)

---

## 5. Errors, Timeouts, Crashes

### 5.1 Event Timeouts

Default **5 s** per `extension/*` request; overridable per event in
the manifest. On timeout:

- Host proceeds as if the plugin returned `continue`.
- Warning is logged.
- Three consecutive timeouts → plugin is marked `unhealthy` and
  event dispatch to it is paused. The process is **not** killed.

### 5.2 Error Codes

Standard JSON-RPC codes plus AEP-specific:

| Code | Meaning |
|---|---|
| `-32700` | Parse error |
| `-32600` | Invalid request |
| `-32601` | Method not found |
| `-32602` | Invalid params |
| `-32603` | Internal error |
| `-32000` | `HandleExpired` |
| `-32001` | `CapabilityDenied` |
| `-32002` | `InvalidAction` |

### 5.3 Crash Handling

- Host monitors subprocess exit code.
- Non-zero exit → last N lines of stderr captured, extension marked
  `crashed`.
- **v1 does not auto-restart.** Operator runs
  `alva extension restart <name>`.
- A crashed plugin does not affect the host or sibling plugins.

---

## 6. Full Example Flow

User triggers a `shell` tool; `my-memory` has subscribed to
`before_tool_call` and blocks destructive commands.

```
Host                              Plugin (my-memory)
 │                                     │
 │ extension/before_tool_call          │
 │   id=h-42                           │
 │   stateHandle=s-7                   │
 │   toolCall=<rm -rf />               │
 │────────────────────────────────────▶│
 │                                     │
 │ host/log                            │
 │   id=p-3                            │
 │   level=warn                        │
 │   message=blocking rm -rf           │
 │◀────────────────────────────────────│
 │                                     │
 │ {result: {}}  id=p-3                │
 │────────────────────────────────────▶│
 │                                     │
 │ host/state.get_messages             │
 │   id=p-4                            │
 │   handle=s-7                        │
 │◀────────────────────────────────────│
 │                                     │
 │ {result: [...]}  id=p-4             │
 │────────────────────────────────────▶│
 │                                     │
 │ {result: {                          │
 │    action: "block",                 │
 │    reason: "Destructive rm -rf"     │
 │ }}                                  │
 │   id=h-42                           │
 │◀────────────────────────────────────│
 │                                     │
 │ [host: stateHandle s-7 invalidated] │
```

---

## 7. Out of v0.1 Scope

Deferred explicitly — do not implement until planned:

- **Hot reload.** Plugin code change → operator runs
  `alva extension restart <name>`.
- **Capability enforcement.** `requestedCapabilities` observed only.
- **Streaming events.** Token-level hooks, streaming LLM response
  hooks.
- **Plugin as memory backend.** `provides: ["memory"]` routing.
- **Plugin ↔ plugin messaging.**
- **Remote registry.** `alva extension install @author/name`.
- **Capability negotiation.** Downgrade flow when protocol versions
  disagree.
- **Registering arbitrary `dyn Trait` on the bus.** Physically not
  possible across a process boundary and **never** in scope.

---

## 8. Frozen v0.1 Decisions

Recorded so later discussion does not relitigate these:

| Question | Decision |
|---|---|
| Plugin package format | Directory: `~/.alva/extensions/<name>/` with `alva.toml` + `main.{py,js}` |
| Subprocess launch mode | SDK launcher: `python -m alva_sdk <entry>` — plugin code never touches stdio |
| `requestedCapabilities` in v0.1 | Observation mode — log warning only |
| `host/memory.*` in v0.1 | Included. First real E2E demo replaces the memory backend |
