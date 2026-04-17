"""Example alva-sdk plugin: blocks destructive shell commands.

Drop into ``~/.config/alva/extensions/shell-guard/main.py`` next to
this ``alva.toml``::

    name = "shell-guard"
    version = "0.1.0"
    runtime = "python"
    entry = "main.py"

Then start alva — the plugin loads automatically and blocks any
tool call whose arguments contain ``rm -rf``.
"""

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
            # Log through the host so it shows up in alva's tracing.
            await self.host.log(
                f"blocked dangerous command: {command}", level="warn"
            )
            return self.block(f"rm -rf is forbidden: {command}")

        return self.continue_()


if __name__ == "__main__":
    run(ShellGuard())
