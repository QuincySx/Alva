#!/usr/bin/env node
// INPUT:  JSON config, stdin control messages, Claude Agent SDK module resolution
// OUTPUT: JSON-line bridge events and sanitized SDK-load failures
// POS:    Node boundary that resolves and drives the external Claude Agent SDK.
// bridge/index.mjs — Embedded in alva-engine-adapter-claude, written to cache at runtime.
//
// Protocol:
//   stdin  (Rust → Bridge): JSON-line control messages
//   stdout (Bridge → Rust): JSON-line events
//
// Rust process ←→ this script ←→ Claude Agent SDK ←→ Claude Code subprocess

import { createInterface } from "readline";
import { execFileSync } from "child_process";
import { createRequire } from "module";
import path from "path";
import { pathToFileURL } from "url";

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
const SDK_PACKAGE = "@anthropic-ai/claude-agent-sdk";
const NPM_ROOT_TIMEOUT_MS = 3_000;

async function importResolved(requireFrom, label, attempts) {
  let resolved;
  try {
    resolved = requireFrom.resolve(SDK_PACKAGE);
  } catch {
    attempts.push(`${label}: not found`);
    return null;
  }

  attempts.push(`${label}: resolved`);
  try {
    return await import(pathToFileURL(resolved).href);
  } catch {
    throw sdkLoadFailure(label, "import failed");
  }
}

async function importFromGlobalRoot(globalRoot, attempts) {
  const label = "npm global root";
  let resolved;
  try {
    // `npm root -g` already returns the node_modules directory. Resolve the
    // package by its absolute child path; using a require rooted *inside*
    // globalRoot would incorrectly search globalRoot/node_modules.
    const resolver = createRequire(path.join(globalRoot, "__alva_sdk_resolver__.cjs"));
    const packagePath = path.join(globalRoot, SDK_PACKAGE);
    resolved = resolver.resolve(packagePath);
  } catch {
    attempts.push(`${label}: not found`);
    return null;
  }
  attempts.push(`${label}: resolved`);
  try {
    return await import(pathToFileURL(resolved).href);
  } catch {
    throw sdkLoadFailure(label, "import failed");
  }
}

function requireFromDirectory(directory) {
  return createRequire(path.join(directory, "package.json"));
}

function sdkLoadFailure(label, reason) {
  const error = new Error(`Claude Agent SDK load failed at ${label}: ${reason}.`);
  error.alvaSafeMessage = true;
  return error;
}

function isWindowsDrivePath(specifier) {
  return /^[a-zA-Z]:[\\/]/.test(specifier);
}

function isUrlSpecifier(specifier) {
  return !isWindowsDrivePath(specifier)
    && /^[a-zA-Z][a-zA-Z+.-]*:/.test(specifier);
}

function isFilesystemSpecifier(specifier) {
  return path.isAbsolute(specifier)
    || isWindowsDrivePath(specifier)
    || specifier.startsWith("./")
    || specifier.startsWith("../");
}

async function importExplicitSdk(specifier, attempts) {
  const label = "explicit SDK override";
  attempts.push(`${label}: configured`);
  if (isUrlSpecifier(specifier)) {
    try {
      return await import(specifier);
    } catch {
      throw sdkLoadFailure(label, "import failed");
    }
  }

  if (isFilesystemSpecifier(specifier)) {
    try {
      const base = config.cwd ? path.resolve(config.cwd) : process.cwd();
      const resolvedPath = isWindowsDrivePath(specifier)
        ? specifier
        : path.resolve(base, specifier);
      return await import(pathToFileURL(resolvedPath).href);
    } catch {
      throw sdkLoadFailure(label, "import failed");
    }
  }

  const base = config.cwd ? path.resolve(config.cwd) : process.cwd();
  let resolved;
  try {
    resolved = requireFromDirectory(base).resolve(specifier);
  } catch {
    throw sdkLoadFailure(label, "resolve failed");
  }
  try {
    return await import(pathToFileURL(resolved).href);
  } catch {
    throw sdkLoadFailure(label, "import failed");
  }
}

function npmGlobalRoot(attempts) {
  const windows = process.platform === "win32";
  const command = windows ? "cmd.exe" : "npm";
  const args = windows ? ["/d", "/s", "/c", "npm root -g"] : ["root", "-g"];
  try {
    const root = execFileSync(command, args, {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
      timeout: NPM_ROOT_TIMEOUT_MS,
    }).trim();
    if (root) return root;
  } catch {
    // A missing/timed-out npm is equivalent to no global installation.
  }
  attempts.push("npm global root: unavailable");
  return null;
}

// Resolve in explicit-to-ambient order. The bridge itself lives in a cache
// directory, so a bare import would otherwise search the wrong node_modules.
async function loadSdk() {
  const attempts = [];
  // `sdk_package_path` deliberately means an import specifier/path, NOT the
  // Claude Code executable. A caller can use a file URL, absolute entry file,
  // or an ESM-resolvable package specifier.
  if (config.sdk_package_path !== null && config.sdk_package_path !== undefined) {
    // Explicit configuration is fail-closed. Falling through after it fails
    // could silently load a different ambient SDK version.
    return importExplicitSdk(config.sdk_package_path, attempts);
  }

  const cwd = config.cwd ? path.resolve(config.cwd) : null;
  if (cwd) {
    const sdk = await importResolved(requireFromDirectory(cwd), "request cwd node_modules", attempts);
    if (sdk) return sdk;
  }

  const processCwd = process.cwd();
  if (!cwd || processCwd !== cwd) {
    const sdk = await importResolved(requireFromDirectory(processCwd), "process cwd node_modules", attempts);
    if (sdk) return sdk;
  }

  const globalRoot = npmGlobalRoot(attempts);
  if (globalRoot) {
    const sdk = await importFromGlobalRoot(globalRoot, attempts);
    if (sdk) return sdk;
  }

  const error = new Error(
    `Cannot find ${SDK_PACKAGE}. Tried ${attempts.join("; ")}. ` +
    `Install it via: npm install -g ${SDK_PACKAGE}`
  );
  error.alvaSafeMessage = true;
  throw error;
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

  // --- Model & API ---
  if (config.model) options.model = config.model;
  if (config.effort) options.effort = config.effort;
  if (config.max_budget_usd != null) options.maxBudgetUsd = config.max_budget_usd;
  if (config.max_turns != null) options.maxTurns = config.max_turns;
  if (config.system_prompt) options.systemPrompt = config.system_prompt;
  if (config.resume_session) options.resume = config.resume_session;

  // --- Authentication ---
  if (config.api_key) options.env.ANTHROPIC_API_KEY = config.api_key;
  if (config.api_base_url) options.env.ANTHROPIC_BASE_URL = config.api_base_url;

  // --- Execution ---
  if (config.sandbox && config.sandbox.enabled) {
    options.sandbox = { enabled: true };
  }
  if (config.persist_session != null) options.persistSession = config.persist_session;

  // --- Settings sources ---
  if (config.setting_sources && config.setting_sources.length > 0) {
    options.settingSources = config.setting_sources;
  }

  // --- MCP servers ---
  if (config.mcp_servers && Object.keys(config.mcp_servers).length > 0) {
    options.mcpServers = config.mcp_servers;
  }

  // --- Agents ---
  if (config.agents && Object.keys(config.agents).length > 0) {
    options.agents = config.agents;
  }

  // --- Permission callback (only in default mode) ---
  if (config.permission_mode === "default" || !config.permission_mode) {
    options.canUseTool = canUseTool;
  }

  const result = query({ prompt: config.prompt, options });

  for await (const message of result) {
    emit("sdk_message", { message });
  }

  emit("done");
}

main().catch((error) => {
  // The host only receives stable category text. Never reflect Node paths,
  // import URLs, or error strings which can expose machine/user information.
  emit("error", {
    message: error?.alvaSafeMessage
      ? error.message
      : "Claude Agent SDK bridge failed",
  });
  process.exit(1);
});
