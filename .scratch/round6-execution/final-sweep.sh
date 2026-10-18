#!/bin/bash
cd /Volumes/DevVault/Dev/Projects/QuincyWork/alva-agent
LOG=.scratch/round6-execution/final-sweep.log
: > "$LOG"
run() { echo "=== $1 ===" >> "$LOG"; shift; "$@" >> "$LOG" 2>&1; echo "--- exit=$? ---" >> "$LOG"; }
run "kernel-core lib (R72)" cargo test --offline -p alva-kernel-core --lib
run "agent-core lib (R62)" cargo test --offline -p alva-agent-core --lib
run "app-core full (R71/R72/R73)" cargo test --offline -p alva-app-core
run "hub-core lib (R63/R64/R80/R90)" cargo test --offline -p alva-hub-core --lib
run "engine-runtime (R83 harness)" cargo test --offline -p alva-engine-runtime --features test-support
run "codex adapter (R81)" cargo test --offline -p alva-engine-adapter-codex
run "pi adapter (R82)" cargo test --offline -p alva-engine-adapter-pi
run "alva adapter (R74)" cargo test --offline -p alva-engine-adapter-alva
run "claude adapter (R74)" cargo test --offline -p alva-engine-adapter-claude
run "hub-server bin (R65)" cargo test --offline -p alva-hub-server --bin alva-hub-server
run "hub-desktop lib (R66)" cargo test --offline -p alva-hub-desktop --lib
echo "=== ALL DONE ===" >> "$LOG"
