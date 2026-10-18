# INPUT: Explicit checked-in alva_sdk import and AEP v0.1 native host callbacks
# OUTPUT: Hermetic real Python Plugin that mutates messages/arguments/results and exposes a tool
# POS: Cross-language protocol acceptance fixture without third-party packages or ambient settings
from alva_sdk import (
    Plugin, Message, ToolResult, before_tool_call, after_tool_call,
    on_llm_call_start, on_llm_call_end, tool, run,
)


class Fixture(Plugin):
    name = "fixture"
    version = "1"
    requested_capabilities = ["host:log", "host:state.get_metadata", "host:state.get_messages"]

    @on_llm_call_start
    async def start(self, messages):
        metadata = await self.host.state_get_metadata()
        if metadata.get("session_id") != "native":
            raise RuntimeError("wrong native authority")
        history = await self.host.state_get_messages(limit=2)
        if not history or not any(message.text for message in history):
            raise RuntimeError("message protocol text disappeared")
        return self.modify_messages([Message.system("sdk-system")] + messages)

    @before_tool_call
    async def before(self, call):
        if call.name == "target":
            await self.host.log("real Python pre-intent callback")
            return self.modify_args({"path": "sdk-effective"})
        return self.continue_()

    @after_tool_call
    async def after(self, call, result):
        if call.name == "target":
            if result.text != "actual target executed":
                raise RuntimeError("tool result protocol text disappeared")
            return self.modify_result(ToolResult.text("SDK transformed result", is_error=True))
        return self.continue_()

    @on_llm_call_end
    async def end(self, response):
        if response.text == "done":
            return self.modify_response(Message.assistant("sdk-done"))
        return self.continue_()

    @tool(name="echo", description="Explicit fixture echo", input_schema={"type": "object"})
    async def echo(self, text=""):
        return "Python SDK echo: " + text


if __name__ == "__main__":
    run(Fixture())
