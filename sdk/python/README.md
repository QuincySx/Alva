# alva-sdk

Python SDK for writing plugins for the [alva-agent](https://github.com/QuincySx/alva-agent) framework.

Talks AEP (Alva Extension Protocol) with the alva host over stdio.
Plugin authors write Python; alva loads the subprocess at startup.

## Install

```bash
pip install alva-sdk
```

## Minimal example

```python
from alva_sdk import Extension, ToolCall, before_tool_call, run


class ShellGuard(Extension):
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
- `@after_tool_call` — runs after a tool returns; observational
- `@on_agent_start` — runs when the agent loop begins
- `@on_agent_end` — runs when the agent loop ends; receives optional error
- `@on_user_message` — runs on new user input

## Host API (call back into alva)

Inside any handler, `self.host` gives you:

- `await self.host.log(message, level="info", **fields)` — log through alva's tracing
- `await self.host.notify(message, level="info")` — user-visible notification
- `await self.host.emit_metric(name, value, labels=None)` — numeric metric

More host APIs (state access, memory read/write) are coming in later
phases — see the [AEP spec](../../crates/alva-app-extension-loader/docs/aep.md)
for the full planned surface.

## License

MIT
