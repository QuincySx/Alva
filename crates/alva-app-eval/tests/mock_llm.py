"""Scenario-based Mock OpenAI LLM for eval test suite.

Route scenario via system prompt: "SCENARIO:<name>"
Supports all test cases in TEST_CASES.md.
"""

import json, time, sys
from http.server import HTTPServer, BaseHTTPRequestHandler

PORT = 8999


def sse(data):
    return f"data: {json.dumps(data)}\n\n".encode()


def chunk(content=None, tool_calls=None, finish_reason=None, usage=None):
    delta = {}
    if content is not None:
        delta["role"] = "assistant"
        delta["content"] = content
    if tool_calls is not None:
        delta["tool_calls"] = tool_calls
    c = {
        "id": f"cmpl-{int(time.time() * 1000)}",
        "object": "chat.completion.chunk",
        "created": int(time.time()),
        "model": "mock-model",
        "choices": [{"index": 0, "delta": delta, "finish_reason": finish_reason}],
    }
    if usage:
        c["usage"] = usage
    return c


def done():
    return b"data: [DONE]\n\n"


def text_response(wfile, text, in_tok=100, out_tok=20):
    for w in text.split():
        wfile.write(sse(chunk(content=w + " ")))
    wfile.write(sse(chunk(finish_reason="stop",
                          usage={"prompt_tokens": in_tok, "completion_tokens": out_tok,
                                 "total_tokens": in_tok + out_tok})))
    wfile.write(done())
    wfile.flush()


def tool_response(wfile, call_id, name, args, in_tok=100, out_tok=20):
    wfile.write(sse(chunk(tool_calls=[
        {"index": 0, "id": call_id, "type": "function",
         "function": {"name": name, "arguments": json.dumps(args)}}
    ])))
    wfile.write(sse(chunk(finish_reason="tool_calls",
                          usage={"prompt_tokens": in_tok, "completion_tokens": out_tok,
                                 "total_tokens": in_tok + out_tok})))
    wfile.write(done())
    wfile.flush()


def multi_tool_response(wfile, calls, in_tok=200, out_tok=40):
    for i, (cid, name, args) in enumerate(calls):
        wfile.write(sse(chunk(tool_calls=[
            {"index": i, "id": cid, "type": "function",
             "function": {"name": name, "arguments": json.dumps(args)}}
        ])))
    wfile.write(sse(chunk(finish_reason="tool_calls",
                          usage={"prompt_tokens": in_tok, "completion_tokens": out_tok,
                                 "total_tokens": in_tok + out_tok})))
    wfile.write(done())
    wfile.flush()


def count_tool_turns(messages):
    return sum(1 for m in messages if m.get("role") == "assistant" and m.get("tool_calls"))


def get_scenario(messages):
    for m in messages:
        if m.get("role") == "system":
            for line in m.get("content", "").split("\n"):
                if "SCENARIO:" in line:
                    return line.split("SCENARIO:")[1].strip().split()[0].lower()
    return "a1"


class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))))
        messages = body.get("messages", [])
        scenario = get_scenario(messages)
        turns = count_tool_turns(messages)

        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.end_headers()

        # ── A1: pure text ──
        if scenario == "a1":
            text_response(self.wfile, "Hello! I am a helpful assistant.", 50, 15)

        # ── A2: single tool call ──
        elif scenario == "a2":
            if turns == 0:
                tool_response(self.wfile, "a2_c1", "list_files", {"path": "."}, 80, 20)
            else:
                text_response(self.wfile, "Found project files in the directory.", 200, 25)

        # ── A3: multi-turn tool chain ──
        elif scenario == "a3":
            if turns == 0:
                tool_response(self.wfile, "a3_c1", "list_files", {"path": "."}, 100, 20)
            elif turns == 1:
                tool_response(self.wfile, "a3_c2", "read_file", {"path": "Cargo.toml"}, 400, 30)
            else:
                text_response(self.wfile, "The project is a Rust workspace with multiple crates.", 800, 40)

        # ── A4: multi-tool in one turn ──
        elif scenario == "a4":
            if turns == 0:
                multi_tool_response(self.wfile, [
                    ("a4_c1", "list_files", {"path": "."}),
                    ("a4_c2", "list_files", {"path": "crates/"}),
                ], 150, 35)
            else:
                text_response(self.wfile, "Listed both directories successfully.", 400, 30)

        # ── A5: hit max_iterations ──
        elif scenario == "a5":
            tool_response(self.wfile, f"a5_{turns}", "list_files", {"path": "."}, 100 + turns * 30, 20)

        # ── T2: tool reads nonexistent file (valid params, tool returns error) ──
        elif scenario == "t2":
            if turns == 0:
                tool_response(self.wfile, "t2_c1", "read_file", {"path": "/nonexistent/file/that/does/not/exist.txt"}, 80, 20)
            else:
                text_response(self.wfile, "The file does not exist.", 200, 15)

        # ── T3: tool large result ──
        elif scenario == "t3":
            if turns == 0:
                tool_response(self.wfile, "t3_c1", "grep_search", {"pattern": "pub fn", "path": "."}, 100, 25)
            else:
                text_response(self.wfile, "Found many public functions in the codebase.", 2000, 30)

        # ── T4: execute_shell ──
        elif scenario == "t4":
            if turns == 0:
                tool_response(self.wfile, "t4_c1", "execute_shell", {"command": "echo hello_from_shell"}, 80, 20)
            else:
                text_response(self.wfile, "Shell command executed successfully.", 200, 15)

        # ── T5: file_edit (use workspace-relative paths) ──
        elif scenario == "t5":
            ws = next((m.get("content","") for m in messages if m.get("role")=="system"), "")
            # Extract workspace from WORKSPACE: marker if present
            if turns == 0:
                tool_response(self.wfile, "t5_c1", "create_file", {"path": ".alva/eval-test.txt", "content": "test content 123"}, 80, 25)
            elif turns == 1:
                tool_response(self.wfile, "t5_c2", "file_edit", {
                    "path": ".alva/eval-test.txt",
                    "old_string": "test content 123",
                    "new_string": "modified content 456"
                }, 200, 30)
            else:
                text_response(self.wfile, "File created and edited successfully.", 300, 20)

        # ── T6: create_file (workspace-relative) ──
        elif scenario == "t6":
            if turns == 0:
                tool_response(self.wfile, "t6_c1", "create_file", {"path": ".alva/eval-create-test.txt", "content": "new file here"}, 80, 20)
            else:
                text_response(self.wfile, "File created.", 150, 10)

        # ── M1: loop detection warn ──
        elif scenario == "m1":
            if turns < 3:
                tool_response(self.wfile, f"m1_{turns}", "list_files", {"path": "."}, 100, 20)
            else:
                text_response(self.wfile, "Done listing.", 300, 15)

        # ── M2: loop detection hard limit ──
        elif scenario == "m2":
            tool_response(self.wfile, f"m2_{turns}", "list_files", {"path": "."}, 100, 20)

        # ── S1: security auto-approve (shell) ──
        elif scenario == "s1":
            if turns == 0:
                tool_response(self.wfile, "s1_c1", "execute_shell", {"command": "echo security_test"}, 80, 20)
            else:
                text_response(self.wfile, "Command approved and executed.", 200, 15)

        # ── E1: LLM HTTP error ──
        elif scenario == "e1":
            self.wfile.write(b"")  # close without SSE
            # Actually we need to send a proper error. Override status:
            # Can't change status after send_response, so just send empty SSE
            self.wfile.flush()
            return

        # ── E2: empty SSE body ──
        elif scenario == "e2":
            self.wfile.write(done())
            self.wfile.flush()

        # ── E3: tool error (valid params, file doesn't exist) ──
        elif scenario == "e3":
            if turns == 0:
                tool_response(self.wfile, "e3_c1", "read_file", {"path": "/no/such/path/xyz.txt"}, 80, 20)
            else:
                text_response(self.wfile, "Tool failed as expected.", 200, 15)

        # ── E4: error then recovery (first call fails, second succeeds) ──
        elif scenario == "e4":
            if turns == 0:
                tool_response(self.wfile, "e4_c1", "read_file", {"path": "/no/such/file.txt"}, 80, 20)
            elif turns == 1:
                tool_response(self.wfile, "e4_c2", "list_files", {"path": "."}, 200, 20)
            else:
                text_response(self.wfile, "Recovered from error and listed files.", 400, 25)

        # ── D8: concurrent run support (just normal responses) ──
        elif scenario == "d8a" or scenario == "d8b":
            if turns == 0:
                tool_response(self.wfile, f"{scenario}_c1", "list_files", {"path": "."}, 80, 20)
            else:
                text_response(self.wfile, f"Response from {scenario}.", 200, 15)

        # ── M3: dangling tool call (call a tool not in the registered list) ──
        elif scenario == "m3":
            if turns == 0:
                tool_response(self.wfile, "m3_c1", "nonexistent_tool_xyz", {"arg": "val"}, 80, 20)
            else:
                text_response(self.wfile, "Dangling tool call handled.", 150, 15)

        # ── M5: compaction (many turns to fill context) ──
        elif scenario == "m5":
            if turns < 8:
                tool_response(self.wfile, f"m5_{turns}", "list_files", {"path": "."}, 500 + turns * 200, 20)
            else:
                text_response(self.wfile, "Done after many turns.", 2000, 30)

        # ── M7: checkpoint (write tool triggers checkpoint) ──
        elif scenario == "m7":
            if turns == 0:
                tool_response(self.wfile, "m7_c1", "create_file", {"path": ".alva/checkpoint-test.txt", "content": "checkpoint data"}, 80, 25)
            else:
                text_response(self.wfile, "File written, checkpoint should have fired.", 200, 20)

        # ── SA1: sub-agent spawn ──
        elif scenario == "sa1":
            if turns == 0:
                # Parent: spawn a sub-agent via the "agent" tool
                tool_response(self.wfile, "sa1_c1", "agent", {
                    "task": "List the files in the project root",
                    "role": "explorer",
                    "inherit_tools": True
                }, 100, 30)
            else:
                # Parent: after sub-agent result, summarize
                text_response(self.wfile, "Sub-agent explored the project successfully.", 500, 40)

        else:
            # Fallback: any request without SCENARIO (e.g., child agents)
            # returns a simple text response so child agents complete quickly
            text_response(self.wfile, "I have completed the assigned task. Here are the results.", 100, 30)

    def log_message(self, fmt, *args):
        print(f"[mock] {args[0]}", file=sys.stderr)


if __name__ == "__main__":
    print(f"Mock LLM on http://127.0.0.1:{PORT}")
    HTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
