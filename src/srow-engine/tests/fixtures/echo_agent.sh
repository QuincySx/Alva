#!/bin/bash
# Echo Agent — minimal ACP-compatible agent for integration testing.
#
# Protocol:
#   1. Read bootstrap payload (first line of stdin, JSON)
#   2. Read subsequent lines as ACP outbound messages
#   3. For "prompt" messages: emit task_start -> session_update (echo) -> task_complete
#   4. For "shutdown" messages: exit 0
#   5. For "cancel" messages: emit task_complete with cancelled reason

# Read bootstrap payload (discard it — echo agent doesn't need model config)
read -r BOOTSTRAP_LINE

# Process incoming messages
while IFS= read -r LINE; do
    # Skip empty lines
    [ -z "$LINE" ] && continue

    # Extract message type
    MSG_TYPE=$(echo "$LINE" | python3 -c "import sys,json; print(json.load(sys.stdin).get('type',''))" 2>/dev/null)

    case "$MSG_TYPE" in
        prompt)
            # Extract prompt content
            CONTENT=$(echo "$LINE" | python3 -c "import sys,json; print(json.load(sys.stdin).get('content',''))" 2>/dev/null)

            # Emit task_start
            echo '{"acp_event_type":"task_start","data":{"task_id":"echo-task-1","description":"Echo agent task"}}'

            # Emit session_update with echoed content
            # Escape the content for JSON
            ESCAPED_CONTENT=$(echo "$CONTENT" | python3 -c "import sys,json; print(json.dumps(sys.stdin.read().strip()))" 2>/dev/null)
            echo "{\"acp_event_type\":\"session_update\",\"session_id\":\"echo-session-1\",\"content\":[{\"type\":\"text\",\"text\":${ESCAPED_CONTENT},\"is_delta\":false}]}"

            # Emit task_complete
            echo '{"acp_event_type":"task_complete","data":{"task_id":"echo-task-1","finish_reason":"complete","summary":"Echoed user prompt"}}'
            ;;

        cancel)
            echo '{"acp_event_type":"task_complete","data":{"task_id":"echo-task-1","finish_reason":"cancelled","summary":"Task cancelled by user"}}'
            ;;

        shutdown)
            exit 0
            ;;

        pong)
            # Ignore pong messages
            ;;

        *)
            # Unknown message type — log to stderr
            echo "echo_agent: unknown message type: $MSG_TYPE" >&2
            ;;
    esac
done
