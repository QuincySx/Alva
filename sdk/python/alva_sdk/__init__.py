"""alva-sdk — write alva agent plugins in Python.

Plugin authors subclass :class:`Extension`, decorate async methods
with event decorators like :func:`before_tool_call`, and start the
runtime from their ``if __name__ == "__main__":`` block with
:func:`run`. The SDK takes care of the AEP (Alva Extension Protocol)
handshake, the JSON-RPC stdio loop, and dispatching events to the
right handler.

Minimal example::

    from alva_sdk import Extension, ToolCall, before_tool_call, run

    class ShellGuard(Extension):
        name = "shell-guard"
        version = "0.1.0"

        @before_tool_call
        async def guard(self, call: ToolCall):
            if "rm -rf" in call.args.get("command", ""):
                return self.block("rm -rf forbidden")
            return self.continue_()

    if __name__ == "__main__":
        run(ShellGuard())
"""

from .extension import Extension, ToolCall
from .decorators import (
    before_tool_call,
    after_tool_call,
    on_agent_start,
    on_agent_end,
    on_user_message,
)
from .runtime import run

__version__ = "0.1.0"

__all__ = [
    "Extension",
    "ToolCall",
    "before_tool_call",
    "after_tool_call",
    "on_agent_start",
    "on_agent_end",
    "on_user_message",
    "run",
]
