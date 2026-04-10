let currentRunId = null;

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
          ${r.turns} turns, ${r.total_tokens} tok, ${(r.duration_ms/1000).toFixed(1)}s
        </div>`;
      el.onclick = () => loadRecord(r.run_id);
      container.appendChild(el);
    });
  } catch (e) {
    container.innerHTML = `<div style="color:var(--red);font-size:12px">${e.message}</div>`;
  }
}

async function loadRecord(runId) {
  currentRunId = runId;
  const content = document.getElementById('inspector-content');
  content.innerHTML = '<div style="color:var(--text-dim);margin:auto">Loading record...</div>';

  try {
    const res = await fetch(`/api/records/${runId}`);
    if (!res.ok) { content.innerHTML = '<div style="color:var(--red);margin:auto">Record not found</div>'; return; }
    const record = await res.json();
    renderRecord(record, content);
  } catch (e) {
    content.innerHTML = `<div style="color:var(--red);margin:auto">${e.message}</div>`;
  }
}

function renderRecord(record, container) {
  container.innerHTML = '';

  // Config summary card
  const configCard = document.createElement('div');
  configCard.className = 'card start';
  configCard.innerHTML = `
    <div class="card-header">Configuration</div>
    <div style="font-size:12px;margin-top:4px;color:var(--text-dim)">
      Model: <strong style="color:var(--text)">${escHtml(record.config_snapshot.model_id)}</strong><br>
      Tools: ${record.config_snapshot.tool_names.length} registered<br>
      Max iterations: ${record.config_snapshot.max_iterations}
    </div>
    <details style="margin-top:8px">
      <summary style="cursor:pointer;font-size:12px;color:var(--blue)">System Prompt</summary>
      <pre>${escHtml(record.config_snapshot.system_prompt)}</pre>
    </details>`;
  container.appendChild(configCard);

  // Each turn
  record.turns.forEach(turn => {
    const turnCard = document.createElement('div');
    turnCard.className = 'card message';
    turnCard.style.borderLeftColor = 'var(--purple)';

    let html = `<div class="card-header" style="font-size:14px">Turn ${turn.turn_number} <span style="font-size:11px;color:var(--text-dim);font-weight:400">${turn.duration_ms}ms</span></div>`;

    // LLM Request (expandable)
    html += `<details style="margin-top:8px">
      <summary style="cursor:pointer;font-size:12px;color:var(--blue)">
        LLM Request — ${turn.llm_call.messages_sent_count} messages, ${turn.llm_call.input_tokens} input tokens
      </summary>
      <pre style="max-height:400px">${escHtml(formatJson(turn.llm_call.messages_sent))}</pre>
    </details>`;

    // LLM Response
    const resp = turn.llm_call.response;
    if (resp) {
      const respText = resp.content ? resp.content.map(b => {
        if (b.type === 'text' || b.Text) return b.text || b.Text?.text || '';
        if (b.type === 'tool_use' || b.ToolUse) return `[tool_use: ${b.name || b.ToolUse?.name}]`;
        return JSON.stringify(b);
      }).join('') : '(empty)';

      html += `<details style="margin-top:6px" open>
        <summary style="cursor:pointer;font-size:12px;color:var(--blue)">
          LLM Response — ${turn.llm_call.output_tokens} tokens, ${turn.llm_call.duration_ms}ms, stop: ${escHtml(turn.llm_call.stop_reason)}
        </summary>
        <pre style="max-height:400px">${escHtml(truncate(respText, 3000))}</pre>
      </details>`;
    }

    // Tool calls
    turn.tool_calls.forEach(tc => {
      const badgeCls = tc.is_error ? 'badge-err' : 'badge-ok';
      const badgeText = tc.is_error ? 'ERROR' : 'OK';
      html += `<div style="margin-top:8px;padding:8px;background:var(--bg);border-radius:6px;border-left:2px solid var(--orange)">
        <div class="card-header">
          <span class="badge badge-tool">TOOL</span>
          ${escHtml(tc.tool_call.name)}
          <span class="badge ${badgeCls}">${badgeText}</span>
          <span style="font-size:11px;color:var(--text-dim);font-weight:400">${tc.duration_ms}ms</span>
        </div>
        <details style="margin-top:4px">
          <summary style="cursor:pointer;font-size:11px;color:var(--blue)">Arguments</summary>
          <pre>${escHtml(formatJson(tc.tool_call.arguments))}</pre>
        </details>
        <details style="margin-top:4px">
          <summary style="cursor:pointer;font-size:11px;color:var(--blue)">Result</summary>
          <pre>${escHtml(truncate(tc.result ? formatToolOutput(tc.result) : '(no output)', 2000))}</pre>
        </details>
      </div>`;
    });

    turnCard.innerHTML = html;
    container.appendChild(turnCard);
  });

  // Summary
  const summary = document.createElement('div');
  summary.className = 'card end-ok';
  summary.innerHTML = `
    <div class="card-header">Summary</div>
    <div style="font-size:13px;margin-top:4px">
      ${record.turns.length} turns,
      ${record.total_input_tokens + record.total_output_tokens} total tokens
      (${record.total_input_tokens} in + ${record.total_output_tokens} out),
      ${(record.total_duration_ms / 1000).toFixed(1)}s
    </div>`;
  container.appendChild(summary);

  // Load tracing logs for this run
  loadLogs(currentRunId, container);

  document.getElementById('status').textContent = `Inspecting run`;
  document.getElementById('status').style.color = 'var(--blue)';
}

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
        Tracing Logs (${logs.length} events)
      </summary>
      <div style="margin-top:8px;max-height:500px;overflow-y:auto">`;

    for (const log of logs) {
      const levelColor = {
        'ERROR': 'var(--red)',
        'WARN': 'var(--orange)',
        'INFO': 'var(--blue)',
        'DEBUG': 'var(--text-dim)',
      }[log.level] || 'var(--text-dim)';

      const fields = Object.entries(log.fields)
        .map(([k, v]) => `<span style="color:var(--text-dim)">${escHtml(k)}</span>=<span style="color:var(--text)">${escHtml(truncate(v.replace(/^"|"$/g, ''), 120))}</span>`)
        .join(' ');

      html += `<div style="font-family:var(--mono);font-size:11px;padding:3px 6px;border-bottom:1px solid var(--border)">
        <span style="color:${levelColor};font-weight:600;width:40px;display:inline-block">${log.level}</span>
        <span style="color:var(--text-dim)">${log.target.split('::').pop()}</span>
        <span style="color:var(--text)">${escHtml(log.message)}</span>
        ${fields ? `<div style="margin-left:46px;color:var(--text-dim)">${fields}</div>` : ''}
      </div>`;
    }

    html += `</div></details>`;
    logsCard.innerHTML = html;
    container.appendChild(logsCard);
  } catch (e) {
    // Log fetch failed — not critical
  }
}

// Init
loadRuns();
// Auto-select from URL param
const params = new URLSearchParams(window.location.search);
const autoRun = params.get('run');
if (autoRun) loadRecord(autoRun);
