#!/usr/bin/env python3
"""
Echo Agent — minimal ACP-compatible agent for integration testing.

Protocol:
  1. Read bootstrap payload (first line of stdin, JSON)
  2. Read subsequent lines as ACP outbound messages
  3. For "prompt" messages: emit task_start -> session_update (echo) -> task_complete
  4. For "shutdown" messages: exit 0
  5. For "cancel" messages: emit task_complete with cancelled reason
"""

import json
import sys


def main():
    # 1. Read bootstrap payload (first line)
    bootstrap_line = sys.stdin.readline().strip()
    if not bootstrap_line:
        sys.exit(1)

    # Parse bootstrap just to validate it's valid JSON
    try:
        _bootstrap = json.loads(bootstrap_line)
    except json.JSONDecodeError:
        print(json.dumps({
            "acp_event_type": "error_data",
            "data": {"code": "INVALID_BOOTSTRAP", "message": "Invalid bootstrap JSON", "recoverable": False}
        }), flush=True)
        sys.exit(1)

    # 2. Process incoming messages
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue

        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue

        msg_type = msg.get("type", "")

        if msg_type == "prompt":
            content = msg.get("content", "")

            # Emit task_start
            print(json.dumps({
                "acp_event_type": "task_start",
                "data": {
                    "task_id": "echo-task-1",
                    "description": "Echo agent task"
                }
            }), flush=True)

            # Emit session_update with echoed content
            print(json.dumps({
                "acp_event_type": "session_update",
                "session_id": "echo-session-1",
                "content": [{
                    "type": "text",
                    "text": content,
                    "is_delta": False
                }]
            }), flush=True)

            # Emit task_complete
            print(json.dumps({
                "acp_event_type": "task_complete",
                "data": {
                    "task_id": "echo-task-1",
                    "finish_reason": "complete",
                    "summary": "Echoed user prompt"
                }
            }), flush=True)

        elif msg_type == "cancel":
            print(json.dumps({
                "acp_event_type": "task_complete",
                "data": {
                    "task_id": "echo-task-1",
                    "finish_reason": "cancelled",
                    "summary": "Task cancelled by user"
                }
            }), flush=True)

        elif msg_type == "shutdown":
            sys.exit(0)

        elif msg_type == "pong":
            pass  # Ignore pong

        else:
            print(f"echo_agent: unknown message type: {msg_type}", file=sys.stderr, flush=True)


if __name__ == "__main__":
    main()
