// INPUT: Current Hub desktop/server and Claude adapter Rust source files.
// OUTPUT: Executable failure-classification result and source-order witnesses.
// POS: Read-only review evidence; does not execute an app or modify production source.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import assert from 'node:assert/strict';

const read = p => fs.readFileSync(p, 'utf8');
const desktop = read('crates/alva-hub-desktop/src/main.rs');
const server = read('crates/alva-hub-server/src/main.rs');
const adapter = read('crates/alva-engine-adapter-claude/src/adapter.rs');
const run = read('crates/alva-hub-desktop/src/run.rs');
const failure = read('crates/alva-hub-desktop/src/failure.rs')
  .split('#[cfg(test)]')[0]
  .replace('use serde::Serialize;', '')
  .replace(', Serialize', '')
  .replace(/#\[serde\([^\n]*\)\]\n/g, '');
const message = adapter.match(/message: "([^"]*cancel grace; killed)"/)[1];
const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'alva-stop-review-'));
const executable = path.join(temp, 'classification');
const source = `${failure}\nfn main() { let message = ${JSON.stringify(message)}; let class = classify_failure(message); println!("ACTUAL CLASSIFIER: {:?}, triggers_fallback={}", class, triggers_fallback(class)); assert!(triggers_fallback(class)); }`;
const compiled = spawnSync('rustc', ['--edition=2021', '-', '-o', executable], {input: source, encoding:'utf8'});
assert.equal(compiled.status, 0, compiled.stderr);
const result = spawnSync(executable, [], {encoding:'utf8'});
assert.equal(result.status, 0, result.stderr);
process.stdout.write(result.stdout);

const execution = desktop.slice(desktop.indexOf('async fn execute_session_run('), desktop.indexOf('struct PermissionDecisionRequest'));
assert.match(execution, /\.await;[\s\S]*if !session_key\.is_empty\(\) \{[\s\S]*active_hubs\.lock\(\)\.unwrap\(\)\.remove\(&session_key\)/);
console.log('SOURCE WITNESS: desktop removes live hub after any run result, including failed claim.');
const hibernate = desktop.slice(desktop.indexOf('async fn hibernate_worker('), desktop.indexOf('fn restore_worker('));
assert.match(hibernate, /if let Err\(error\) = stop_attempt_inner[\s\S]*tracing::debug![\s\S]*\.remove\(&worker.worker_session_id\)/);
console.log('SOURCE WITNESS: hibernate swallows cancel failure and removes the still-live hub.');
const stop = server.slice(server.indexOf('async fn stop_attempt('), server.indexOf('fn effective_model_routes('));
assert.ok(stop.indexOf('permission_router.clear_session') < stop.indexOf('hub.cancel('));
assert.match(run, /let armed = terminal_error.is_some\(\) \|\| \(open_tools.is_empty\(\) && !permission_pending\)/);
console.log('SOURCE WITNESS: failed HTTP cancel loses approval route; pending permission disables stall timer.');
console.log('Scope: classifier executed from current Rust source; other witnesses are source-order checks, not end-to-end tests.');
