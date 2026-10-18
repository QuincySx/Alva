#!/bin/bash
cd /Volumes/DevVault/Dev/Projects/QuincyWork/alva-agent
LOG=.scratch/round6-execution/cargo-verify.log
: > "$LOG"
run() {
  echo "=== $1 ===" >> "$LOG"
  shift
  "$@" >> "$LOG" 2>&1
  echo "--- exit=$? ---" >> "$LOG"
}
run "R62 agent-core lib" cargo test --offline -p alva-agent-core --lib
run "R63/R64 hub-core lib" cargo test --offline -p alva-hub-core --lib
run "R65 hub-server bin (HTTP)" cargo test --offline -p alva-hub-server --bin alva-hub-server
run "R66 hub-desktop lib" cargo test --offline -p alva-hub-desktop --lib
run "R71 app-core r48" cargo test --offline -p alva-app-core --test r48_swap_child_identity
run "R72-WIP app-core r11" cargo test --offline -p alva-app-core --test r11_tool_lock_matrix
echo "=== ALL DONE ===" >> "$LOG"
