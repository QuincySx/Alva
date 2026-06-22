"""AEP runtime — async JSON-RPC loop over stdin/stdout.

Authors call :func:`run` with an :class:`Plugin` instance; the
runtime drives the full protocol:

- read lines from stdin, parse JSON-RPC
- handle ``initialize`` / ``initialized`` / ``shutdown``
- dispatch ``extension/*`` events to decorated methods
- route plugin-originated host calls (``self.host.log`` etc.) to the
  host and route responses back to the awaiting coroutine

The runtime is single-process, single-task; there is no explicit
threading. Handler methods may be sync or async — the runtime awaits
coroutines and passes through regular return values.
"""

import asyncio
import inspect
import json
import sys
import traceback
from typing import Any, Dict, Optional

from .extension import Message, Plugin, ToolCall, ToolResult, _to_wire

PROTOCOL_VERSION = "0.1.0"


class HostProxy:
    """Plugin-side helper for calling host APIs.

    Attached to the extension as ``self.host`` during the handshake.
    Every method is async — ``await`` it from within a handler.
    """

    def __init__(self, send_call):
        self._send_call = send_call
        self._state_handle: Optional[str] = None

    def _set_state_handle(self, handle: Optional[str]) -> None:
        self._state_handle = handle

    def _state_params(self, **extra: Any) -> Dict[str, Any]:
        if not self._state_handle:
            raise RuntimeError("state handle is not available for this event")
        params: Dict[str, Any] = {"handle": self._state_handle}
        params.update(extra)
        return params

    async def log(self, message: str, level: str = "info", **fields: Any) -> None:
        """Write a line through the alva host logger.

        ``level`` is one of ``"trace"``, ``"debug"``, ``"info"``,
        ``"warn"``, ``"error"``. Additional keyword arguments are
        attached as structured fields on the log event.
        """
        params: Dict[str, Any] = {"level": level, "message": message}
        if fields:
            params["fields"] = fields
        await self._send_call("host/log", params)

    async def notify(self, message: str, level: str = "info") -> None:
        """Show a user-visible notification."""
        await self._send_call(
            "host/notify", {"level": level, "message": message}
        )

    async def emit_metric(
        self,
        name: str,
        value: float,
        labels: Optional[Dict[str, Any]] = None,
    ) -> None:
        """Report a numeric metric."""
        params: Dict[str, Any] = {"name": name, "value": value}
        if labels:
            params["labels"] = labels
        await self._send_call("host/emit_metric", params)

    async def state_get_messages(self, limit: Optional[int] = None, offset: int = 0) -> Any:
        """Read messages for the currently handled event."""
        params: Dict[str, Any] = {}
        if limit is not None:
            params["limit"] = limit
        if offset:
            params["offset"] = offset
        result = await self._send_call(
            "host/state.get_messages", self._state_params(**params)
        )
        return [Message.from_wire(message) for message in result.get("messages", [])]

    async def state_get_metadata(self) -> Any:
        """Read metadata for the currently handled event."""
        result = await self._send_call(
            "host/state.get_metadata", self._state_params()
        )
        return result.get("metadata", {})

    async def state_count_tokens(self) -> int:
        """Estimate token count for the currently handled event."""
        result = await self._send_call(
            "host/state.count_tokens", self._state_params()
        )
        return int(result.get("tokens", 0))


def _discover_handlers(plugin: Plugin) -> Dict[str, Any]:
    """Walk the plugin and collect methods tagged by event decorators."""
    handlers: Dict[str, Any] = {}
    for attr_name in dir(plugin):
        if attr_name.startswith("_"):
            continue
        try:
            attr = getattr(plugin, attr_name, None)
        except AttributeError:
            continue
        if attr is None:
            continue
        event_name = getattr(attr, "__aep_event__", None)
        if event_name:
            handlers[event_name] = attr
    return handlers


def _discover_tools(plugin: Plugin) -> Dict[str, Any]:
    """Walk the plugin and collect methods tagged by ``@tool``."""
    tools: Dict[str, Any] = {}
    for attr_name in dir(plugin):
        if attr_name.startswith("_"):
            continue
        try:
            attr = getattr(plugin, attr_name, None)
        except AttributeError:
            continue
        if attr is None:
            continue
        tool_def = getattr(attr, "__aep_tool__", None)
        if tool_def:
            tools[tool_def["name"]] = attr
    return tools


def run(plugin: Plugin) -> None:
    """Run the plugin until the host closes stdin or sends ``shutdown``.

    This is the one thing every plugin file calls from its
    ``if __name__ == "__main__":`` block.
    """
    try:
        asyncio.run(_main(plugin))
    except KeyboardInterrupt:
        pass


async def _main(plugin: Plugin) -> None:
    handlers = _discover_handlers(plugin)
    tools = _discover_tools(plugin)
    subscriptions = sorted(handlers.keys())

    # Pending plugin-originated RPCs keyed by our request id.
    pending: Dict[str, "asyncio.Future[Any]"] = {}
    next_id = [0]

    def next_req_id() -> str:
        next_id[0] += 1
        return f"p-{next_id[0]}"

    # We protect stdout with a lock so concurrent tasks (host call
    # responses + plugin-originated calls) cannot interleave bytes.
    stdout_lock = asyncio.Lock()

    async def send_obj(obj: Any) -> None:
        line = json.dumps(obj)
        async with stdout_lock:
            sys.stdout.write(line + "\n")
            sys.stdout.flush()

    async def send_call(method: str, params: Any) -> Any:
        req_id = next_req_id()
        fut: "asyncio.Future[Any]" = asyncio.get_event_loop().create_future()
        pending[req_id] = fut
        await send_obj(
            {
                "jsonrpc": "2.0",
                "id": req_id,
                "method": method,
                "params": params,
            }
        )
        try:
            return await asyncio.wait_for(fut, timeout=5.0)
        except asyncio.TimeoutError:
            pending.pop(req_id, None)
            raise

    # Attach the host proxy so handlers can call `self.host.log(...)`.
    plugin.host = HostProxy(send_call)

    # Wire up an async stdin reader on top of the OS pipe.
    loop = asyncio.get_event_loop()
    reader = asyncio.StreamReader()
    await loop.connect_read_pipe(
        lambda: asyncio.StreamReaderProtocol(reader), sys.stdin
    )

    shutdown_event = asyncio.Event()
    background_tasks: set = set()

    async def process_incoming(msg: Dict[str, Any]) -> None:
        method = msg.get("method")
        req_id = msg.get("id")

        if method == "initialize":
            await _handle_initialize(plugin, subscriptions, tools, req_id, send_obj)
        elif method == "initialized":
            on_init = getattr(plugin, "on_init", None)
            if callable(on_init):
                try:
                    maybe = on_init()
                    if inspect.isawaitable(maybe):
                        await maybe
                except Exception:
                    traceback.print_exc(file=sys.stderr)
        elif method == "shutdown":
            await send_obj({"jsonrpc": "2.0", "id": req_id, "result": {}})
            shutdown_event.set()
        elif isinstance(method, str) and method.startswith("extension/"):
            event_name = method[len("extension/") :]
            await _dispatch_event(handlers, event_name, msg, send_obj)
        elif method == "tools/list":
            await _handle_tools_list(tools, req_id, send_obj)
        elif method == "tools/call":
            await _handle_tools_call(tools, msg, send_obj)
        else:
            if req_id is not None:
                await send_obj(
                    {
                        "jsonrpc": "2.0",
                        "id": req_id,
                        "error": {
                            "code": -32601,
                            "message": f"method not found: {method}",
                        },
                    }
                )

    # Main loop: race stdin reads against the shutdown event so a
    # handler returning from `shutdown` breaks us out promptly. Each
    # incoming message that needs actual work is dispatched to a
    # background task — this is REQUIRED, not an optimisation:
    # handlers call back into the host via `self.host.log` etc., and
    # those reverse calls need the main loop to keep reading so the
    # response can flow back. Running handlers inline deadlocks.
    while not shutdown_event.is_set():
        read_task = asyncio.create_task(reader.readline())
        stop_task = asyncio.create_task(shutdown_event.wait())
        done, _ = await asyncio.wait(
            [read_task, stop_task],
            return_when=asyncio.FIRST_COMPLETED,
        )

        if stop_task in done and read_task not in done:
            read_task.cancel()
            break
        stop_task.cancel()

        if read_task not in done:
            break

        line_bytes = read_task.result()
        if not line_bytes:
            break  # EOF — host closed stdin
        line = line_bytes.decode("utf-8").strip()
        if not line:
            continue

        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue

        # Fast path: a response to one of our plugin-originated calls.
        # Resolve the awaiting future inline; don't spawn a task.
        method = msg.get("method")
        req_id = msg.get("id")
        if method is None and req_id in pending:
            fut = pending.pop(req_id)
            if "error" in msg and msg["error"] is not None:
                fut.set_exception(RuntimeError(str(msg["error"])))
            else:
                fut.set_result(msg.get("result"))
            continue

        # Slow path: dispatch on a background task so the main loop
        # can immediately return to reading stdin.
        task = asyncio.create_task(process_incoming(msg))
        background_tasks.add(task)
        task.add_done_callback(background_tasks.discard)

    # Drain any in-flight handlers before exit so we do not truncate
    # a response mid-write.
    if background_tasks:
        await asyncio.gather(*background_tasks, return_exceptions=True)


async def _handle_initialize(
    plugin: Plugin,
    subscriptions: list,
    tools: Dict[str, Any],
    req_id: Any,
    send_obj,
) -> None:
    await send_obj(
        {
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {
                "protocolVersion": PROTOCOL_VERSION,
                "plugin": {
                    "name": getattr(plugin, "name", "unnamed"),
                    "version": getattr(plugin, "version", "0.0.0"),
                    "description": getattr(plugin, "description", ""),
                },
                "tools": [_tool_def(handler) for handler in tools.values()],
                "eventSubscriptions": subscriptions,
                "requestedCapabilities": list(
                    getattr(plugin, "requested_capabilities", []) or []
                ),
            },
        }
    )


def _tool_def(handler: Any) -> Dict[str, Any]:
    meta = getattr(handler, "__aep_tool__", {})
    return {
        "name": meta.get("name", getattr(handler, "__name__", "tool")),
        "description": meta.get("description", ""),
        "inputSchema": meta.get("inputSchema", {"type": "object"}),
    }


async def _handle_tools_list(
    tools: Dict[str, Any],
    req_id: Any,
    send_obj,
) -> None:
    await send_obj(
        {
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {"tools": [_tool_def(handler) for handler in tools.values()]},
        }
    )


async def _handle_tools_call(
    tools: Dict[str, Any],
    msg: Dict[str, Any],
    send_obj,
) -> None:
    req_id = msg.get("id")
    params = msg.get("params") or {}
    name = params.get("name")
    arguments = params.get("arguments") or {}
    handler = tools.get(name)
    if handler is None:
        await send_obj(
            {
                "jsonrpc": "2.0",
                "id": req_id,
                "error": {
                    "code": -32602,
                    "message": f"tool not found: {name}",
                },
            }
        )
        return

    try:
        maybe = handler(**arguments)
        result = await maybe if inspect.isawaitable(maybe) else maybe
    except Exception as e:
        traceback.print_exc(file=sys.stderr)
        await send_obj(
            {
                "jsonrpc": "2.0",
                "id": req_id,
                "result": _tool_output(str(e), is_error=True),
            }
        )
        return

    await send_obj(
        {
            "jsonrpc": "2.0",
            "id": req_id,
            "result": _tool_output(result),
        }
    )


def _tool_output(value: Any, is_error: bool = False) -> Dict[str, Any]:
    value = _to_wire(value)
    if isinstance(value, dict) and "content" in value:
        normalized = dict(value)
        if "isError" not in normalized:
            normalized["isError"] = bool(normalized.pop("is_error", is_error))
        return normalized
    if not isinstance(value, str):
        value = json.dumps(value)
    return {
        "content": [{"type": "text", "text": value}],
        "isError": is_error,
    }


async def _dispatch_event(
    handlers: Dict[str, Any],
    event_name: str,
    msg: Dict[str, Any],
    send_obj,
) -> None:
    req_id = msg.get("id")
    handler = handlers.get(event_name)
    if handler is None:
        # We claimed to subscribe but have no handler? Be defensive.
        await send_obj(
            {
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {"action": "continue"},
            }
        )
        return

    params = msg.get("params") or {}
    args = _unpack_event_args(event_name, params)
    state_handle = params.get("stateHandle")

    try:
        host = getattr(getattr(handler, "__self__", None), "host", None)
        if host is not None and hasattr(host, "_set_state_handle"):
            host._set_state_handle(state_handle)
        maybe = handler(*args)
        if inspect.isawaitable(maybe):
            result = await maybe
        else:
            result = maybe
    except Exception as e:
        traceback.print_exc(file=sys.stderr)
        await send_obj(
            {
                "jsonrpc": "2.0",
                "id": req_id,
                "error": {
                    "code": -32603,
                    "message": f"handler raised: {e}",
                },
            }
        )
        return
    finally:
        host = getattr(getattr(handler, "__self__", None), "host", None)
        if host is not None and hasattr(host, "_set_state_handle"):
            host._set_state_handle(None)

    if result is None:
        result = {"action": "continue"}

    await send_obj({"jsonrpc": "2.0", "id": req_id, "result": result})


def _unpack_event_args(
    event_name: str, params: Dict[str, Any]
) -> tuple:
    """Turn a raw AEP params dict into positional arguments for a
    decorated handler method.

    Handlers always take ``self`` implicitly. On top of that:

    - ``before_tool_call`` → :class:`ToolCall`
    - ``after_tool_call`` → :class:`ToolCall`, :class:`ToolResult`
    - ``on_llm_call_start`` → ``list`` of :class:`Message`
    - ``on_llm_call_end`` → :class:`Message`
    - ``on_user_message`` → ``str`` (the message text)
    - ``on_agent_start`` → nothing
    - ``on_agent_end`` → ``Optional[str]`` (the error, if any)
    - anything else → the raw params dict
    """
    if event_name == "before_tool_call":
        return (ToolCall.from_wire(params.get("toolCall")),)
    if event_name == "after_tool_call":
        return (
            ToolCall.from_wire(params.get("toolCall")),
            ToolResult.from_wire(params.get("result") or {}),
        )
    if event_name == "on_llm_call_start":
        return ([Message.from_wire(message) for message in params.get("messages") or []],)
    if event_name == "on_llm_call_end":
        return (Message.from_wire(params.get("response") or {}),)
    if event_name == "on_user_message":
        msg = params.get("message") or {}
        return (msg.get("text", ""),)
    if event_name == "on_agent_start":
        return ()
    if event_name == "on_agent_end":
        return (params.get("error"),)
    return (params,)
