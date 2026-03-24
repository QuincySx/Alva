#!/usr/bin/env node
// bridge/index.mjs — Embedded in alva-engine-adapter-claude, written to cache at runtime.
//
// Protocol:
//   stdin  (Rust → Bridge): JSON-line control messages
//   stdout (Bridge → Rust): JSON-line events
//
// Rust process ←→ this script ←→ Claude Agent SDK ←→ Claude Code subprocess

import { createInterface } from "readline";

// Config is passed as the first CLI argument (JSON string).
const config = JSON.parse(process.argv[2] || "{}");

// --- stdout emitter ---
function emit(type, data = {}) {
  process.stdout.write(JSON.stringify({ type, ...data }) + "\n");
}

// --- stdin control message handler ---
const pendingPermissions = new Map();
let abortController = new AbortController();

const rl = createInterface({ input: process.stdin, crlfDelay: Infinity });
rl.on("line", (line) => {
  let msg;
  try { msg = JSON.parse(line); } catch { return; }

  if (msg.type === "permission_response" && msg.request_id) {
    const resolve = pendingPermissions.get(msg.request_id);
    if (resolve) {
      pendingPermissions.delete(msg.request_id);
      const decision = msg.decision || {};
      if (decision.behavior === "allow") {
        resolve({ behavior: "allow", updatedInput: decision.updated_input });
      } else {
        resolve({ behavior: "deny", message: decision.message || "Denied" });
      }
    }
  } else if (msg.type === "cancel") {
    abortController.abort();
  } else if (msg.type === "shutdown") {
    abortController.abort();
    setTimeout(() => process.exit(0), 1000);
  }
});

// --- canUseTool callback (bridges permissions to Rust) ---
async function canUseTool(toolName, toolInput, { signal }) {
  const requestId = crypto.randomUUID();
  emit("permission_request", {
    request_id: requestId,
    tool_name: toolName,
    tool_input: toolInput,
  });
  return new Promise((resolve) => {
    pendingPermissions.set(requestId, resolve);
    const onAbort = () => {
      pendingPermissions.delete(requestId);
      resolve({ behavior: "deny", message: "Aborted" });
    };
    if (signal) {
      signal.addEventListener("abort", onAbort, { once: true });
    }
    // Timeout: auto-deny after 60s if no response
    setTimeout(() => {
      if (pendingPermissions.has(requestId)) {
        pendingPermissions.delete(requestId);
        resolve({ behavior: "deny", message: "Permission timeout" });
      }
    }, 60_000);
  });
}

// --- Dynamic import of SDK ---
async function loadSdk() {
  try {
    return await import("@anthropic-ai/claude-agent-sdk");
  } catch {
    // If the SDK is not in node_modules, try the explicit path
    if (config.sdk_package_path) {
      return await import(config.sdk_package_path);
    }
    throw new Error(
      "Cannot find @anthropic-ai/claude-agent-sdk. " +
      "Install it via: npm install -g @anthropic-ai/claude-agent-sdk"
    );
  }
}

// --- Main ---
async function main() {
  const { query } = await loadSdk();

  const options = {
    cwd: config.cwd || process.cwd(),
    abortController,
    permissionMode: config.permission_mode || "default",
    allowedTools: config.allowed_tools || [],
    disallowedTools: config.disallowed_tools || [],
    includePartialMessages: config.streaming !== false,
    env: { ...process.env, ...(config.env || {}) },
  };

  if (config.model) options.model = config.model;
  if (config.max_budget_usd != null) options.maxBudgetUsd = config.max_budget_usd;
  if (config.system_prompt) options.systemPrompt = config.system_prompt;
  if (config.api_key) options.env.ANTHROPIC_API_KEY = config.api_key;
  if (config.sdk_executable_path) options.pathToClaudeCodeExecutable = config.sdk_executable_path;

  // MCP servers
  if (config.mcp_servers && Object.keys(config.mcp_servers).length > 0) {
    options.mcpServers = config.mcp_servers;
  }

  // Permission callback (only in default mode)
  if (config.permission_mode === "default" || !config.permission_mode) {
    options.canUseTool = canUseTool;
  }

  const result = query({ prompt: config.prompt, options });

  for await (const message of result) {
    emit("sdk_message", { message });
  }

  emit("done");
}

main().catch((err) => {
  emit("error", { message: err?.message || String(err) });
  process.exit(1);
});
