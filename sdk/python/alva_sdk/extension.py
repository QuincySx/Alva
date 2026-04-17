"""Extension base class and typed event payload wrappers."""

from dataclasses import dataclass
from typing import Any, Dict, List, Optional


@dataclass
class ToolCall:
    """A tool call the agent is about to run (or just finished).

    Handlers decorated with ``@before_tool_call`` or
    ``@after_tool_call`` receive an instance of this class wrapping
    the raw AEP ``toolCall`` payload.
    """

    id: str
    name: str
    args: Dict[str, Any]

    @classmethod
    def from_wire(cls, data: Optional[Dict[str, Any]]) -> "ToolCall":
        data = data or {}
        return cls(
            id=data.get("id", ""),
            name=data.get("name", ""),
            args=data.get("arguments") or {},
        )


class Extension:
    """Base class for all alva-sdk plugins.

    Subclass, set ``name`` / ``version`` / ``description`` as class
    attributes, and decorate async methods with event decorators
    (``@before_tool_call``, etc.) to register them. The runtime
    discovers the decorated methods during ``initialize`` and routes
    events to them.

    Inside a handler, ``self.host`` exposes host APIs (``log``,
    ``notify``, ``emit_metric``).
    """

    name: str = "unnamed-extension"
    version: str = "0.0.0"
    description: str = ""
    requested_capabilities: List[str] = []

    # Populated by the runtime during the handshake. Type-annotated
    # here so IDEs can see it, but initialised lazily.
    host: "Any" = None  # alva_sdk.runtime.HostProxy

    # ---- Action helpers (return from event handlers) ----

    def continue_(self) -> Dict[str, Any]:
        """Proceed normally — do not modify the triggering operation."""
        return {"action": "continue"}

    def block(self, reason: str) -> Dict[str, Any]:
        """Block the triggering operation with a human-readable reason.

        Currently only legal for the ``before_tool_call`` event.
        """
        return {"action": "block", "reason": reason}

    def modify_args(self, new_args: Dict[str, Any]) -> Dict[str, Any]:
        """Rewrite the tool call's arguments before it runs.

        .. note::
            Host-side support is not yet wired up — the host logs a
            warning and treats this as ``continue`` for now.
        """
        return {"action": "modify", "modified_arguments": new_args}

    def replace_result(self, result: Any) -> Dict[str, Any]:
        """Skip tool execution and use ``result`` directly.

        .. note::
            Same host-side caveat as :meth:`modify_args`.
        """
        return {"action": "replace_result", "result": result}
