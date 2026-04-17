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

    The method receives a :class:`alva_sdk.ToolCall`. Return values
    are currently observational only.
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
