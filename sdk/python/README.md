# alva-sdk

Python SDK for writing plugins for the [alva-agent](https://github.com/QuincySx/alva-agent) framework.

Talks AEP with the alva host over stdio.
Plugin authors write Python; alva loads the subprocess at startup.

## Install

```bash
pip install alva-sdk
```

## Minimal example

```python
from alva_sdk import Plugin, ToolCall, before_tool_call, run, tool


class ShellGuard(Plugin):
    name = "shell-guard"
    version = "0.1.0"
    description = "Blocks destructive shell commands"

    @before_tool_call
    async def guard(self, call: ToolCall):
        if call.name != "shell":
            return self.continue_()
        command = call.args.get("command", "")
        if "rm -rf" in command:
            await self.host.log(f"blocked: {command}", level="warn")
            return self.block(f"rm -rf is forbidden: {command}")
        return self.continue_()

    @tool(
        name="remote_echo",
        description="Echo text from this Python plugin",
        input_schema={
            "type": "object",
            "properties": {"text": {"type": "string"}},
            "required": ["text"],
        },
    )
    async def remote_echo(self, text: str):
        return f"remote:{text}"


if __name__ == "__main__":
    run(ShellGuard())
```

Save as `main.py`, drop it in `~/.config/alva/extensions/shell-guard/`
alongside this `alva.toml`:

```toml
name = "shell-guard"
version = "0.1.0"
runtime = "python"
entry = "main.py"
```

The next time alva starts, your plugin loads and intercepts tool calls.

## Supported event decorators

- `@before_tool_call` — runs before a tool executes; return `self.block(reason)` to stop it
- `@after_tool_call` — runs after a tool returns; receives `(call, result)` and can return `self.modify_result(result)`
- `@on_llm_call_start` — runs before the LLM request; return `self.modify_messages(messages)` to rewrite the request
- `@on_llm_call_end` — runs after the LLM response; return `self.modify_response(message)` to rewrite the response
- `@on_agent_start` — runs when the agent loop begins
- `@on_agent_end` — runs when the agent loop ends; receives optional error
- `@on_user_message` — runs on new user input
- `@tool(...)` — declares an LLM-callable tool served by this plugin; return `str`, `ToolResult`, or a ToolOutput-shaped dict

## Typed payloads

The SDK wraps event payloads while keeping dict-style access available:

- `ToolCall` exposes `call.name`, `call.args`, and `call["..."]` is not needed for normal use
- `Message` exposes `message.role`, `message.text`, `message.get(...)`, `message[...]`, and constructors like `Message.system("...")`
- `ToolResult` exposes `result.text`, `result.is_error`, `result.get(...)`, `result[...]`, and `ToolResult.text("...")`

Action helpers accept either wrappers or raw wire dicts:

```python
from alva_sdk import Message, Plugin, ToolCall, ToolResult, after_tool_call, on_llm_call_start


class RewritePlugin(Plugin):
    @on_llm_call_start
    async def rewrite_messages(self, messages):
        return self.modify_messages([Message.system("extra system context")])

    @after_tool_call
    async def rewrite_tool_result(self, call: ToolCall, result: ToolResult):
        return self.modify_result(ToolResult.text(f"{call.name}: {result.text}"))
```

## Host API (call back into alva)

Inside any handler, `self.host` gives you:

- `await self.host.log(message, level="info", **fields)` — log through alva's tracing
- `await self.host.notify(message, level="info")` — user-visible notification
- `await self.host.emit_metric(name, value, labels=None)` — numeric metric
- `await self.host.state_get_messages(limit=None, offset=0)` — read current event messages
- `await self.host.state_get_metadata()` — read current event metadata
- `await self.host.state_count_tokens()` — estimate current event token count

More host APIs (memory read/write, approval) are coming in later phases —
see the [AEP spec](../../crates/alva-app-extension-loader/docs/aep.md) for
the full planned surface.

## License

MIT
