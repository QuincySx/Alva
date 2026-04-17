"""AEP runtime — async JSON-RPC loop over stdin/stdout.

Authors call :func:`run` with an :class:`Extension` instance; the
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

from .extension import Extension, ToolCall

PROTOCOL_VERSION = "0.1.0"


class HostProxy:
    """Plugin-side helper for calling host APIs.

    Attached to the extension as ``self.host`` during the handshake.
    Every method is async — ``await`` it from within a handler.
    """

    def __init__(self, send_call):
        self._send_call = send_call

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


def _discover_handlers(ext: Extension) -> Dict[str, Any]:
    """Walk the extension and collect methods tagged by event decorators."""
    handlers: Dict[str, Any] = {}
    for attr_name in dir(ext):
        if attr_name.startswith("_"):
            continue
        try:
            attr = getattr(ext, attr_name, None)
        except AttributeError:
            continue
        if attr is None:
            continue
        event_name = getattr(attr, "__aep_event__", None)
        if event_name:
            handlers[event_name] = attr
    return handlers


def run(extension: Extension) -> None:
    """Run the plugin until the host closes stdin or sends ``shutdown``.

    This is the one thing every plugin file calls from its
    ``if __name__ == "__main__":`` block.
    """
    try:
        asyncio.run(_main(extension))
    except KeyboardInterrupt:
        pass


async def _main(extension: Extension) -> None:
    handlers = _discover_handlers(extension)
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
    extension.host = HostProxy(send_call)

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
            await _handle_initialize(
                extension, subscriptions, req_id, send_obj
            )
        elif method == "initialized":
            on_init = getattr(extension, "on_init", None)
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
    extension: Extension,
    subscriptions: list,
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
                    "name": getattr(extension, "name", "unnamed"),
                    "version": getattr(extension, "version", "0.0.0"),
                    "description": getattr(extension, "description", ""),
                },
                "tools": [],  # tool bridging is a later phase
                "eventSubscriptions": subscriptions,
                "requestedCapabilities": list(
                    getattr(extension, "requested_capabilities", []) or []
                ),
            },
        }
    )


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

    try:
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

    if result is None:
        result = {"action": "continue"}

    await send_obj({"jsonrpc": "2.0", "id": req_id, "result": result})


def _unpack_event_args(
    event_name: str, params: Dict[str, Any]
) -> tuple:
    """Turn a raw AEP params dict into positional arguments for a
    decorated handler method.

    Handlers always take ``self`` implicitly. On top of that:

    - ``before_tool_call`` / ``after_tool_call`` → :class:`ToolCall`
    - ``on_user_message`` → ``str`` (the message text)
    - ``on_agent_start`` → nothing
    - ``on_agent_end`` → ``Optional[str]`` (the error, if any)
    - anything else → the raw params dict
    """
    if event_name in ("before_tool_call", "after_tool_call"):
        return (ToolCall.from_wire(params.get("toolCall")),)
    if event_name == "on_user_message":
        msg = params.get("message") or {}
        return (msg.get("text", ""),)
    if event_name == "on_agent_start":
        return ()
    if event_name == "on_agent_end":
        return (params.get("error"),)
    return (params,)
