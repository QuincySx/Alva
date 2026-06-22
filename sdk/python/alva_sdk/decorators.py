"""Event handler decorators.

Each decorator tags an async method with an AEP event name. During
``initialize``, the runtime walks the extension class, collects every
method with a ``__aep_event__`` attribute, and uses that list as
both the subscription list sent to the host and the internal
dispatch table.

Bound methods transparently proxy attribute access to the underlying
function, so reading ``bound_method.__aep_event__`` returns what the
decorator stamped on ``bound_method.__func__``.
"""


def before_tool_call(fn):
    """Register the method as handler for ``before_tool_call`` events.

    The method receives a :class:`alva_sdk.ToolCall`. Return
    ``self.block(reason)`` to prevent the tool from running, or
    ``self.continue_()`` (or ``None``) to let it proceed.
    """
    fn.__aep_event__ = "before_tool_call"
    return fn


def after_tool_call(fn):
    """Register the method as handler for ``after_tool_call`` events.

    The method receives ``(ToolCall, ToolResult)``. Return
    ``self.modify_result(result)`` to replace the completed tool output.
    """
    fn.__aep_event__ = "after_tool_call"
    return fn


def on_agent_start(fn):
    """Register the method as handler for ``on_agent_start`` events.

    The method takes no arguments (besides ``self``).
    """
    fn.__aep_event__ = "on_agent_start"
    return fn


def on_agent_end(fn):
    """Register the method as handler for ``on_agent_end`` events.

    The method receives an ``Optional[str]`` error description.
    """
    fn.__aep_event__ = "on_agent_end"
    return fn


def on_user_message(fn):
    """Register the method as handler for ``on_user_message`` events.

    The method receives the user's text as a single ``str`` argument.
    """
    fn.__aep_event__ = "on_user_message"
    return fn


def on_llm_call_start(fn):
    """Register the method as handler for ``on_llm_call_start`` events.

    The method receives the LLM-bound messages list. Return
    ``self.modify_messages(messages)`` to replace it.
    """
    fn.__aep_event__ = "on_llm_call_start"
    return fn


def on_llm_call_end(fn):
    """Register the method as handler for ``on_llm_call_end`` events.

    The method receives the LLM response message. Return
    ``self.modify_response(message)`` to replace it.
    """
    fn.__aep_event__ = "on_llm_call_end"
    return fn


def tool(name=None, description="", input_schema=None):
    """Register a method as an LLM-callable tool.

    The decorated method is called for ``tools/call`` with keyword
    arguments from the request's ``arguments`` object. It may be sync or
    async. Return a string for a text result, a :class:`alva_sdk.ToolResult`,
    or a ToolOutput-shaped dict with ``content`` and ``isError`` /
    ``is_error``.
    """

    def decorate(fn):
        fn.__aep_tool__ = {
            "name": name or fn.__name__,
            "description": description or (fn.__doc__ or "").strip(),
            "inputSchema": input_schema or {"type": "object"},
        }
        return fn

    return decorate
