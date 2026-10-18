#!/bin/sh
# INPUT: ignored fake CLI arguments
# OUTPUT: valid successful Codex JSONL sequence
# POS: local review fixture with no network or user data access
printf '%s\n' '{"type":"thread.started","thread_id":"review-codex"}' '{"type":"turn.started"}' '{"type":"item.completed","item":{"type":"agent_message","id":"m1","text":"expected successful answer"}}' '{"type":"turn.completed","usage":{"input_tokens":1,"output_tokens":2}}'
