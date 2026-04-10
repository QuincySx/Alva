// ---------------------------------------------------------------------------
// Inspector — run detail viewer with structured turn timeline
// ---------------------------------------------------------------------------

let currentRunId = null;

// ---------------------------------------------------------------------------
// Sidebar: run list
// ---------------------------------------------------------------------------

async function loadRuns() {
  const container = document.getElementById('runs-container');
  try {
    const res = await fetch('/api/runs');
    const runs = await res.json();
    container.innerHTML = '';
    if (runs.length === 0) {
      container.innerHTML = '<div style="color:var(--text-dim);font-size:12px">No completed runs yet. Go to Playground to start one.</div>';
      return;
    }
    runs.forEach(r => {
      const el = document.createElement('div');
      el.className = 'card';
      el.style.cursor = 'pointer';
      el.style.marginBottom = '6px';
      el.innerHTML = `
        <div class="card-header">${escHtml(r.model_id)}</div>
        <div style="font-size:11px;color:var(--text-dim)">
          ${r.turns} turns, ${r.total_tokens} tok, ${(r.duration_ms / 1000).toFixed(1)}s
        </div>`;
      el.onclick = () => loadRecord(r.run_id);
      container.appendChild(el);
    });
  } catch (e) {
    container.innerHTML = `<div style="color:var(--red);font-size:12px">${e.message}</div>`;
  }
}

// ---------------------------------------------------------------------------
// Main: load and render a run record
// ---------------------------------------------------------------------------

async function loadRecord(runId) {
  currentRunId = runId;
  const content = document.getElementById('inspector-content');
  content.innerHTML = '<div style="color:var(--text-dim);margin:auto">Loading record...</div>';

  try {
    const res = await fetch(`/api/records/${runId}`);
    if (!res.ok) {
      content.innerHTML = '<div style="color:var(--red);margin:auto">Record not found</div>';
      return;
    }
    const record = await res.json();
    renderRecord(record, content);
  } catch (e) {
    content.innerHTML = `<div style="color:var(--red);margin:auto">${e.message}</div>`;
  }
}

// ---------------------------------------------------------------------------
// Render: full record
// ---------------------------------------------------------------------------

function renderRecord(record, container) {
  container.innerHTML = '';

  // ── Run Overview ──
  renderOverview(record, container);

  // ── Turn timeline ──
  let prevInputTokens = 0;
  record.turns.forEach((turn, idx) => {
    renderTurn(turn, prevInputTokens, container);
    prevInputTokens = turn.llm_call.input_tokens;
  });

  // ── Summary ──
  renderSummary(record, container);

  // ── Tracing Logs ──
  loadLogs(currentRunId, container);

  document.getElementById('status').textContent = 'Inspecting run';
  document.getElementById('status').style.color = 'var(--blue)';
}

// ---------------------------------------------------------------------------
// Render: overview card
// ---------------------------------------------------------------------------

function renderOverview(record, container) {
  const c = record.config_snapshot;
  const totalTokens = record.total_input_tokens + record.total_output_tokens;
  const totalTools = record.turns.reduce((sum, t) => sum + t.tool_calls.length, 0);

  const el = document.createElement('div');
  el.className = 'card start';
  el.innerHTML = `
    <div class="card-header" style="font-size:14px">Run Overview</div>
    <div style="display:grid;grid-template-columns:1fr 1fr;gap:4px 16px;font-size:12px;margin-top:8px">
      <div>Model: <strong style="color:var(--text)">${escHtml(c.model_id)}</strong></div>
      <div>Total: <strong style="color:var(--text)">${totalTokens.toLocaleString()}</strong> tokens</div>
      <div>Duration: <strong style="color:var(--text)">${(record.total_duration_ms / 1000).toFixed(1)}s</strong></div>
      <div>Turns: <strong style="color:var(--text)">${record.turns.length}</strong></div>
      <div>Tools registered: <strong style="color:var(--text)">${c.tool_names.length}</strong></div>
      <div>Tool calls: <strong style="color:var(--text)">${totalTools}</strong></div>
      <div>Max iterations: <strong style="color:var(--text)">${c.max_iterations}</strong></div>
      <div>Tokens: <strong style="color:var(--text)">${record.total_input_tokens.toLocaleString()}</strong> in / <strong style="color:var(--text)">${record.total_output_tokens.toLocaleString()}</strong> out</div>
    </div>

    <details style="margin-top:10px">
      <summary style="cursor:pointer;font-size:12px;color:var(--blue)">System Prompt</summary>
      <pre>${escHtml(c.system_prompt || '(empty)')}</pre>
    </details>

    ${c.tool_definitions && c.tool_definitions.length > 0 ? `
    <details style="margin-top:6px">
      <summary style="cursor:pointer;font-size:12px;color:var(--blue)">Tool Definitions (${c.tool_definitions.length})</summary>
      <div style="margin-top:6px">
        ${c.tool_definitions.map(td => `
          <details style="margin-left:8px;margin-bottom:4px">
            <summary style="cursor:pointer;font-size:12px">
              <strong>${escHtml(td.name)}</strong>
              <span style="color:var(--text-dim);font-size:11px;margin-left:6px">${escHtml(truncate(td.description, 60))}</span>
            </summary>
            <pre style="margin-left:16px">${escHtml(formatJson(td.parameters))}</pre>
          </details>
        `).join('')}
      </div>
    </details>` : ''}
  `;
  container.appendChild(el);
}

// ---------------------------------------------------------------------------
// Render: single turn
// ---------------------------------------------------------------------------

function renderTurn(turn, prevInputTokens, container) {
  const lc = turn.llm_call;
  const turnTokens = lc.input_tokens + lc.output_tokens;
  const tokenDelta = prevInputTokens > 0 ? lc.input_tokens - prevInputTokens : 0;

  const turnEl = document.createElement('div');
  turnEl.className = 'card';
  turnEl.style.borderLeftColor = 'var(--purple)';
  turnEl.style.padding = '0';

  // ── Turn header ──
  let html = `
    <div style="display:flex;justify-content:space-between;align-items:center;padding:10px 14px;border-bottom:1px solid var(--border)">
      <div style="font-weight:600;font-size:14px">Turn ${turn.turn_number}</div>
      <div style="display:flex;gap:12px;font-size:11px;color:var(--text-dim);font-family:var(--mono)">
        <span>${(turn.duration_ms / 1000).toFixed(1)}s</span>
        <span>${turnTokens.toLocaleString()} tok</span>
      </div>
    </div>
    <div style="padding:10px 14px">
  `;

  // ── LLM Request ──
  html += `
    <div style="background:var(--bg);border-radius:6px;padding:8px 10px;margin-bottom:2px">
      <div style="display:flex;justify-content:space-between;font-size:12px">
        <span style="color:var(--blue);font-weight:600">LLM Request</span>
        <span style="color:var(--text-dim);font-family:var(--mono)">${lc.messages_sent_count} msgs, ${lc.input_tokens.toLocaleString()} input tok</span>
      </div>
      ${tokenDelta > 0 ? `<div style="font-size:11px;color:var(--orange);margin-top:2px">+${tokenDelta.toLocaleString()} tokens from previous turn (history + tool results)</div>` : ''}
      <details style="margin-top:6px">
        <summary style="cursor:pointer;font-size:11px;color:var(--blue)">View messages</summary>
        <pre style="max-height:300px">${escHtml(formatJson(lc.messages_sent))}</pre>
      </details>
    </div>
  `;

  // ── Arrow ──
  html += `<div style="text-align:center;color:var(--text-dim);font-size:14px;line-height:1">↓</div>`;

  // ── LLM Response ──
  const resp = lc.response;
  const stopColor = lc.stop_reason === 'tool_use' ? 'var(--orange)' : lc.stop_reason === 'end_turn' ? 'var(--green)' : 'var(--red)';

  html += `
    <div style="background:var(--bg);border-radius:6px;padding:8px 10px;margin-bottom:2px">
      <div style="display:flex;justify-content:space-between;font-size:12px">
        <span style="color:var(--green);font-weight:600">LLM Response</span>
        <span style="color:var(--text-dim);font-family:var(--mono)">${lc.output_tokens.toLocaleString()} out tok, ${(lc.duration_ms / 1000).toFixed(1)}s</span>
      </div>
      <div style="margin-top:4px">
        <span class="badge" style="background:${stopColor}22;color:${stopColor};font-size:10px;padding:1px 6px;border-radius:8px">${lc.stop_reason}</span>
      </div>
  `;

  if (resp && resp.content) {
    for (const block of resp.content) {
      if ((block.type === 'text' || block.Text) && (block.text || block.Text?.text)) {
        const text = block.text || block.Text?.text || '';
        html += `
          <details style="margin-top:6px" ${lc.stop_reason === 'end_turn' ? 'open' : ''}>
            <summary style="cursor:pointer;font-size:11px;color:var(--blue)">Response text (${text.length} chars)</summary>
            <pre style="max-height:300px">${escHtml(truncate(text, 3000))}</pre>
          </details>`;
      }
      if (block.type === 'tool_use' || block.ToolUse) {
        const name = block.name || block.ToolUse?.name || '?';
        const input = block.input || block.ToolUse?.input || {};
        html += `
          <div style="margin-top:6px;font-size:12px;font-family:var(--mono)">
            <span style="color:var(--orange)">→ ${escHtml(name)}</span>(${escHtml(truncate(JSON.stringify(input), 100))})
          </div>`;
      }
    }
  } else {
    html += `<div style="color:var(--red);font-size:12px;margin-top:4px">(empty response)</div>`;
  }
  html += `</div>`;

  // ── Tool Calls ──
  for (const tc of turn.tool_calls) {
    html += `<div style="text-align:center;color:var(--text-dim);font-size:14px;line-height:1">↓</div>`;

    const isErr = tc.is_error;
    const borderColor = isErr ? 'var(--red)' : 'var(--orange)';
    const statusBadge = isErr
      ? '<span class="badge badge-err">ERROR</span>'
      : '<span class="badge badge-ok">OK</span>';

    html += `
      <div style="background:var(--bg);border-radius:6px;padding:8px 10px;border-left:2px solid ${borderColor}">
        <div style="display:flex;justify-content:space-between;align-items:center;font-size:12px">
          <div>
            <span class="badge badge-tool">TOOL</span>
            <strong style="margin-left:4px">${escHtml(tc.tool_call.name)}</strong>
            ${statusBadge}
          </div>
          <span style="color:var(--text-dim);font-family:var(--mono)">${tc.duration_ms}ms</span>
        </div>
        <details style="margin-top:6px">
          <summary style="cursor:pointer;font-size:11px;color:var(--blue)">Input</summary>
          <pre>${escHtml(formatJson(tc.tool_call.arguments))}</pre>
        </details>
        <details style="margin-top:4px">
          <summary style="cursor:pointer;font-size:11px;color:var(--blue)">Output (${tc.result ? formatToolOutput(tc.result).length : 0} chars)</summary>
          <pre style="max-height:200px">${escHtml(truncate(tc.result ? formatToolOutput(tc.result) : '(no output)', 2000))}</pre>
        </details>
      </div>`;
  }

  html += `</div>`; // close padding div
  turnEl.innerHTML = html;
  container.appendChild(turnEl);
}

// ---------------------------------------------------------------------------
// Render: summary
// ---------------------------------------------------------------------------

function renderSummary(record, container) {
  const totalTokens = record.total_input_tokens + record.total_output_tokens;
  const totalTools = record.turns.reduce((sum, t) => sum + t.tool_calls.length, 0);

  // Token breakdown per turn
  let tokenBreakdown = '';
  if (record.turns.length > 1) {
    tokenBreakdown = `
      <div style="margin-top:8px;font-size:11px">
        <div style="color:var(--text-dim);margin-bottom:4px">Token usage per turn:</div>
        ${record.turns.map(t => {
          const pct = totalTokens > 0 ? ((t.llm_call.input_tokens + t.llm_call.output_tokens) / totalTokens * 100).toFixed(0) : 0;
          return `<div style="display:flex;align-items:center;gap:6px;margin-bottom:2px">
            <span style="width:50px;color:var(--text-dim)">Turn ${t.turn_number}</span>
            <div style="flex:1;height:6px;background:var(--bg3);border-radius:3px;overflow:hidden">
              <div style="height:100%;width:${pct}%;background:var(--blue);border-radius:3px"></div>
            </div>
            <span style="width:80px;text-align:right;font-family:var(--mono)">${(t.llm_call.input_tokens + t.llm_call.output_tokens).toLocaleString()}</span>
          </div>`;
        }).join('')}
      </div>`;
  }

  const el = document.createElement('div');
  el.className = 'card end-ok';
  el.innerHTML = `
    <div class="card-header" style="font-size:14px">Summary</div>
    <div style="display:grid;grid-template-columns:1fr 1fr 1fr;gap:8px;margin-top:8px;text-align:center">
      <div style="background:var(--bg);padding:8px;border-radius:6px">
        <div style="font-size:20px;font-weight:700;color:var(--text)">${record.turns.length}</div>
        <div style="font-size:11px;color:var(--text-dim)">Turns</div>
      </div>
      <div style="background:var(--bg);padding:8px;border-radius:6px">
        <div style="font-size:20px;font-weight:700;color:var(--text)">${totalTokens.toLocaleString()}</div>
        <div style="font-size:11px;color:var(--text-dim)">Tokens</div>
      </div>
      <div style="background:var(--bg);padding:8px;border-radius:6px">
        <div style="font-size:20px;font-weight:700;color:var(--text)">${(record.total_duration_ms / 1000).toFixed(1)}s</div>
        <div style="font-size:11px;color:var(--text-dim)">Duration</div>
      </div>
    </div>
    ${tokenBreakdown}
  `;
  container.appendChild(el);
}

// ---------------------------------------------------------------------------
// Render: tracing logs
// ---------------------------------------------------------------------------

async function loadLogs(runId, container) {
  try {
    const res = await fetch(`/api/logs/${runId}`);
    if (!res.ok) return;
    const logs = await res.json();
    if (!logs.length) return;

    const logsCard = document.createElement('div');
    logsCard.className = 'card';
    logsCard.style.borderLeftColor = 'var(--purple)';

    let html = `<details>
      <summary style="cursor:pointer;font-size:13px;font-weight:600">
        Tracing Logs <span style="color:var(--text-dim);font-weight:400">(${logs.length} events)</span>
      </summary>
      <div style="margin-top:8px;max-height:500px;overflow-y:auto">`;

    const levelColors = {
      'ERROR': 'var(--red)',
      'WARN': 'var(--orange)',
      'INFO': 'var(--blue)',
      'DEBUG': 'var(--text-dim)',
      'TRACE': 'var(--text-dim)',
    };

    for (const log of logs) {
      const color = levelColors[log.level] || 'var(--text-dim)';
      const shortTarget = log.target.split('::').slice(-1)[0];

      // Format fields inline, highlight key ones
      const fieldHtml = Object.entries(log.fields)
        .filter(([k]) => k !== 'message')
        .map(([k, v]) => {
          const val = v.replace(/^"|"$/g, '');
          // Highlight important fields
          if (k === 'body_preview' || k === 'remaining_buffer' || k === 'preview') {
            return `<details style="display:inline"><summary style="cursor:pointer;color:var(--blue);font-size:10px">${k} (${val.length} chars)</summary><pre style="margin:2px 0">${escHtml(truncate(val, 500))}</pre></details>`;
          }
          return `<span style="color:var(--text-dim)">${k}</span>=<span>${escHtml(truncate(val, 80))}</span>`;
        })
        .join(' ');

      html += `<div style="font-family:var(--mono);font-size:11px;padding:4px 6px;border-bottom:1px solid var(--border)">
        <span style="color:${color};font-weight:600;display:inline-block;width:40px">${log.level}</span>
        <span style="color:var(--text-dim)">${shortTarget}</span>
        <span style="color:var(--text);margin-left:4px">${escHtml(log.message)}</span>
        ${fieldHtml ? `<div style="margin-left:46px;margin-top:1px">${fieldHtml}</div>` : ''}
      </div>`;
    }

    html += `</div></details>`;
    logsCard.innerHTML = html;
    container.appendChild(logsCard);
  } catch (e) {
    // Log fetch failed — not critical
  }
}

// ---------------------------------------------------------------------------
// Bootstrap
// ---------------------------------------------------------------------------

loadRuns();
const params = new URLSearchParams(window.location.search);
const autoRun = params.get('run');
if (autoRun) loadRecord(autoRun);
