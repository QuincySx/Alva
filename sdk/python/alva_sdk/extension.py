"""Plugin base class and typed event payload wrappers."""

import time
import uuid
from dataclasses import dataclass
from typing import Any, Dict, List, Optional


class WireObject:
    """Small dict-compatible wrapper around an AEP wire object."""

    def __init__(self, data: Optional[Dict[str, Any]] = None):
        self._data: Dict[str, Any] = dict(data or {})

    def __getitem__(self, key: str) -> Any:
        return self._data[key]

    def get(self, key: str, default: Any = None) -> Any:
        return self._data.get(key, default)

    def to_wire(self) -> Dict[str, Any]:
        return dict(self._data)


class Message(WireObject):
    """A chat message passed to LLM hook handlers."""

    @classmethod
    def from_wire(cls, data: Optional[Dict[str, Any]]) -> "Message":
        return cls(data)

    @classmethod
    def text_message(cls, role: str, text: str) -> "Message":
        return cls(
            {
                "id": str(uuid.uuid4()),
                "role": role,
                "content": [{"type": "text", "text": text}],
                "timestamp": int(time.time() * 1000),
            }
        )

    @classmethod
    def system(cls, text: str) -> "Message":
        return cls.text_message("system", text)

    @classmethod
    def assistant(cls, text: str) -> "Message":
        return cls.text_message("assistant", text)

    @classmethod
    def user(cls, text: str) -> "Message":
        return cls.text_message("user", text)

    @property
    def role(self) -> str:
        return str(self._data.get("role", ""))

    @property
    def text(self) -> str:
        parts = self._data.get("content") or []
        return "".join(
            str(part.get("text", ""))
            for part in parts
            if isinstance(part, dict) and part.get("type") == "text"
        )


class ToolResult(WireObject):
    """A tool result passed to or returned from tool hook handlers."""

    @classmethod
    def from_wire(cls, data: Optional[Dict[str, Any]]) -> "ToolResult":
        return cls(data)

    @classmethod
    def from_text(cls, text: str, is_error: bool = False) -> "ToolResult":
        return cls({"content": [{"type": "text", "text": text}], "is_error": is_error})

    def _text_content(self) -> str:
        parts = self._data.get("content") or []
        return "".join(
            str(part.get("text", ""))
            for part in parts
            if isinstance(part, dict) and part.get("type") == "text"
        )

    @property
    def is_error(self) -> bool:
        return bool(self._data.get("is_error", self._data.get("isError", False)))


class _ToolResultText:
    """Descriptor that supports both ToolResult.text(...) and result.text."""

    def __get__(self, obj: Optional[ToolResult], owner: type) -> Any:
        if obj is None:
            return owner.from_text
        return obj._text_content()


ToolResult.text = _ToolResultText()  # type: ignore[attr-defined]


def _to_wire(value: Any) -> Any:
    if hasattr(value, "to_wire"):
        return value.to_wire()
    if isinstance(value, list):
        return [_to_wire(item) for item in value]
    return value


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


class Plugin:
    """Base class for all alva-sdk plugins.

    Subclass, set ``name`` / ``version`` / ``description`` as class
    attributes, and decorate async methods with event decorators
    (``@before_tool_call``, etc.) to register them. The runtime
    discovers the decorated methods during ``initialize`` and routes
    events to them.

    Inside a handler, ``self.host`` exposes host APIs (``log``,
    ``notify``, ``emit_metric``).
    """

    name: str = "unnamed-plugin"
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

        Use this only for AEP events whose legal actions include ``block``.
        """
        return {"action": "block", "reason": reason}

    def modify_args(self, new_args: Dict[str, Any]) -> Dict[str, Any]:
        """Rewrite the tool call's arguments before it runs.
        """
        return {"action": "modify", "modified_arguments": new_args}

    def replace_result(self, result: Any) -> Dict[str, Any]:
        """Skip tool execution and use ``result`` directly.
        """
        return {"action": "replace_result", "result": _to_wire(result)}

    def modify_messages(self, messages: List[Any]) -> Dict[str, Any]:
        """Replace the message list for ``on_llm_call_start``."""
        return {"action": "modify_messages", "messages": _to_wire(messages)}

    def modify_response(self, response: Any) -> Dict[str, Any]:
        """Replace the response message for ``on_llm_call_end``."""
        return {"action": "modify_response", "response": _to_wire(response)}

    def modify_result(self, result: Any) -> Dict[str, Any]:
        """Replace the completed tool result for ``after_tool_call``."""
        return {"action": "modify_result", "result": _to_wire(result)}
