#!/bin/sh
# INPUT: ignored fake CLI arguments
# OUTPUT: valid successful Pi JSONL sequence
# POS: local review fixture with no network or user data access
printf '%s\n' '{"type":"session","id":"review-pi"}' '{"type":"message_start","message":{"role":"assistant"}}' '{"type":"message_end","message":{"role":"assistant","stopReason":"end","content":[{"type":"text","text":"expected successful answer"}]}}' '{"type":"agent_end","willRetry":false}'
