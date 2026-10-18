# INPUT: Explicit alva_sdk Plugin and AEP event/tool decorators
# OUTPUT: Example shell guard and remote echo capability
# POS: Opt-in author example; never discovered or executed by ordinary Go startup
"""Explicit Alva AEP plugin example that blocks destructive shell arguments.

Supply this file in a local bundle with an alva.toml declaring name, version,
runtime="python" and entry="main.py". Configure the Go host with an explicit
interpreter and separate host approval. No global directory or automatic
startup discovery is used by the Go product.
"""

from alva_sdk import Plugin, ToolCall, before_tool_call, run


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
            # Log through the host so it shows up in alva's tracing.
            await self.host.log(
                f"blocked dangerous command: {command}", level="warn"
            )
            return self.block(f"rm -rf is forbidden: {command}")

        return self.continue_()


if __name__ == "__main__":
    run(ShellGuard())
