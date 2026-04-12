// ---------------------------------------------------------------------------
// Inspector — three-column: runs | timeline | detail
// ---------------------------------------------------------------------------

let currentRunId = null;
let allLogs = [];
let selectedBlock = null;

// Stack of open sub-agent modals. Topmost is the most recently opened.
// ESC key or a modal's own close button pops the topmost.
const subAgentModalStack = [];

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
      container.innerHTML = '<div style="color:var(--text-dim);font-size:12px">No completed runs. Go to Playground first.</div>';
      return;
    }
    runs.forEach(r => {
      const el = document.createElement('div');
      el.className = 'card';
      el.style.cursor = 'pointer';
      el.style.marginBottom = '4px';
      el.style.padding = '6px 8px';
      el.innerHTML = `
        <div style="font-weight:600;font-size:12px">${escHtml(r.model_id)}</div>
        <div style="font-size:10px;color:var(--text-dim)">${r.turns}T ${r.total_tokens}tok ${(r.duration_ms/1000).toFixed(1)}s</div>`;
      el.onclick = () => loadRecord(r.run_id);
      container.appendChild(el);
    });
  } catch (e) {
    container.innerHTML = `<div style="color:var(--red);font-size:12px">${e.message}</div>`;
  }
}

// ---------------------------------------------------------------------------
// Load record + logs
// ---------------------------------------------------------------------------

async function loadRecord(runId) {
  currentRunId = runId;
  const timeline = document.getElementById('inspector-content');
  const detail = document.getElementById('detail-panel');
  timeline.innerHTML = '<div style="color:var(--text-dim);margin:auto">Loading...</div>';
  detail.innerHTML = '<div style="color:var(--text-dim);font-size:12px;padding:16px">Loading...</div>';

  try {
    const [recRes, logsRes] = await Promise.all([
      fetch(`/api/records/${runId}`),
      fetch(`/api/logs/${runId}`),
    ]);
    if (!recRes.ok) { timeline.innerHTML = '<div style="color:var(--red);margin:auto">Record not found</div>'; return; }
    const record = await recRes.json();
    allLogs = logsRes.ok ? await logsRes.json() : [];
    renderTimeline(record, timeline);
    detail.innerHTML = '<div style="color:var(--text-dim);font-size:12px;padding:16px">Click any block to see details</div>';
    document.getElementById('status').textContent = 'Inspecting run';
    document.getElementById('status').style.color = 'var(--blue)';
  } catch (e) {
    timeline.innerHTML = `<div style="color:var(--red);margin:auto">${e.message}</div>`;
  }
}

// ---------------------------------------------------------------------------
// Select a block → show detail in right panel
// ---------------------------------------------------------------------------

function selectBlock(el, detailFn) {
  // Scope the selection + detail panel lookup to the nearest enclosing
  // `.inspector-body` — this works for both the main inspector view and
  // each sub-agent modal (which reuses the same structure). Lets the
  // same renderTimeline / click handlers work recursively without any
  // per-view plumbing.
  const view = el.closest('.inspector-body') || document.querySelector('.inspector-body');
  view.querySelectorAll('.event-block.selected').forEach(b => b.classList.remove('selected'));
  el.classList.add('selected');
  const panel = view.querySelector('.inspector-detail');
  panel.innerHTML = '';
  detailFn(panel);
  panel.scrollTop = 0;
}

function addDetailSection(panel, title, contentHtml) {
  const section = document.createElement('div');
  section.className = 'detail-section';
  section.innerHTML = `<h4>${title}</h4>${contentHtml}`;
  panel.appendChild(section);
}

// Filter logs by message keywords
function filterLogs(keywords) {
  return allLogs.filter(l => keywords.some(kw => l.message.toLowerCase().includes(kw.toLowerCase()) || l.target.includes(kw)));
}

function renderLogEntries(logs) {
  if (!logs.length) return '<div style="color:var(--text-dim)">No logs for this event</div>';
  const colors = { 'ERROR': 'var(--red)', 'WARN': 'var(--orange)', 'INFO': 'var(--blue)', 'DEBUG': 'var(--text-dim)' };
  return logs.map(l => {
    const fields = Object.entries(l.fields).map(([k, v]) => {
      const val = String(v).replace(/^"|"$/g, '');
      if (val.length > 100) {
        let formatted = val;
        try { formatted = JSON.stringify(JSON.parse(val), null, 2); } catch {}
        return `<div style="margin-top:2px"><span style="color:var(--text-dim)">${escHtml(k)}:</span><pre>${escHtml(formatted)}</pre></div>`;
      }
      return `<span style="color:var(--text-dim)">${escHtml(k)}</span>=<span>${escHtml(val)}</span> `;
    }).join('');
    return `<div style="padding:4px 0;border-bottom:1px solid var(--border);font-family:var(--mono);font-size:11px">
      <span style="color:${colors[l.level] || 'var(--text-dim)'};font-weight:600">${l.level}</span>
      <span style="color:var(--text)">${escHtml(l.message)}</span>
      <div style="margin-left:4px;margin-top:2px">${fields}</div>
    </div>`;
  }).join('');
}

// ---------------------------------------------------------------------------
// Render timeline (left center column)
// ---------------------------------------------------------------------------

function renderTimeline(record, container) {
  container.innerHTML = '';

  // Overview block
  renderOverviewBlock(record, container);

  // Turns
  let prevInputTokens = 0;
  // Detect fallback turns
  const fallbackTurns = new Set();
  allLogs.forEach((log, i) => {
    if (log.message.includes('falling back to non-streaming')) {
      const completedBefore = allLogs.slice(0, i).filter(l => l.message.includes('turn completed')).length;
      fallbackTurns.add(completedBefore + 1);
    }
  });

  record.turns.forEach(turn => {
    renderTurnBlock(turn, prevInputTokens, container, fallbackTurns.has(turn.turn_number));
    prevInputTokens = turn.llm_call.input_tokens;
  });

  // Summary
  renderSummaryBlock(record, container);
}

// ---------------------------------------------------------------------------
// Overview block
// ---------------------------------------------------------------------------

function renderOverviewBlock(record, container) {
  const c = record.config_snapshot;
  const totalTokens = record.total_input_tokens + record.total_output_tokens;
  const totalTools = record.turns.reduce((sum, t) => sum + t.tool_calls.length, 0);

  const el = document.createElement('div');
  el.className = 'card start event-block';
  el.innerHTML = `
    <div class="card-header" style="font-size:13px">Run Overview</div>
    <div style="font-size:11px;color:var(--text-dim);margin-top:4px">
      ${escHtml(c.model_id)} · ${record.turns.length}T · ${totalTokens.toLocaleString()} tok · ${(record.total_duration_ms/1000).toFixed(1)}s
    </div>`;

  el.onclick = () => selectBlock(el, panel => {
    addDetailSection(panel, 'Configuration', `
      <div style="display:grid;grid-template-columns:1fr 1fr;gap:3px;font-size:12px">
        <div>Model: <strong>${escHtml(c.model_id)}</strong></div>
        <div>Turns: <strong>${record.turns.length}</strong></div>
        <div>Duration: <strong>${(record.total_duration_ms/1000).toFixed(1)}s</strong></div>
        <div>Tokens: <strong>${totalTokens.toLocaleString()}</strong></div>
        <div>Tools: <strong>${c.tool_names.length}</strong></div>
        <div>Tool calls: <strong>${totalTools}</strong></div>
        <div>Max iter: <strong>${c.max_iterations}</strong></div>
        <div>In/Out: <strong>${record.total_input_tokens.toLocaleString()} / ${record.total_output_tokens.toLocaleString()}</strong></div>
      </div>`);
    addDetailSection(panel, 'System Prompt', `<pre>${escHtml(c.system_prompt || '(empty)')}</pre>`);
    if (c.tool_definitions?.length) {
      addDetailSection(panel, `Tool Definitions (${c.tool_definitions.length})`,
        c.tool_definitions.map(td =>
          `<details style="margin-bottom:4px"><summary style="cursor:pointer"><strong>${escHtml(td.name)}</strong> <span style="color:var(--text-dim)">${escHtml(truncate(td.description,50))}</span></summary><pre>${escHtml(formatJson(td.parameters))}</pre></details>`
        ).join(''));
    }
  });

  container.appendChild(el);
}

// ---------------------------------------------------------------------------
// Turn block (contains LLM request, response, tools)
// ---------------------------------------------------------------------------

function renderTurnBlock(turn, prevInputTokens, container, usedFallback) {
  const lc = turn.llm_call;
  const turnTokens = lc.input_tokens + lc.output_tokens;
  const tokenDelta = prevInputTokens > 0 ? lc.input_tokens - prevInputTokens : 0;
  const fb = usedFallback ? ' <span style="background:#553300;color:var(--orange);font-size:9px;padding:1px 5px;border-radius:6px">fallback</span>' : '';

  // Turn wrapper
  const turnEl = document.createElement('div');
  turnEl.style.cssText = 'border-left:2px solid var(--purple);padding-left:10px;margin-left:4px';

  // Turn header
  const header = document.createElement('div');
  header.style.cssText = 'font-size:12px;color:var(--text-dim);margin-bottom:4px;display:flex;justify-content:space-between';
  header.innerHTML = `<span style="font-weight:600;color:var(--text)">Turn ${turn.turn_number}${fb}</span><span style="font-family:var(--mono)">${(turn.duration_ms/1000).toFixed(1)}s · ${turnTokens.toLocaleString()} tok</span>`;
  turnEl.appendChild(header);

  // LLM Request block
  const reqEl = document.createElement('div');
  reqEl.className = 'card event-block';
  reqEl.style.cssText = 'padding:6px 10px;margin-bottom:4px;border-left-color:var(--blue)';
  reqEl.innerHTML = `
    <div style="display:flex;justify-content:space-between;font-size:11px">
      <span style="color:var(--blue);font-weight:600">LLM Request</span>
      <span style="color:var(--text-dim);font-family:var(--mono)">${lc.messages_sent_count} msgs · ${lc.input_tokens.toLocaleString()} in tok</span>
    </div>
    ${tokenDelta > 0 ? `<div style="font-size:10px;color:var(--orange)">+${tokenDelta.toLocaleString()} from prev turn</div>` : ''}`;

  reqEl.onclick = () => selectBlock(reqEl, panel => {
    addDetailSection(panel, `LLM Request — Turn ${turn.turn_number}`, `
      <div style="font-size:12px;margin-bottom:6px">
        Messages: ${lc.messages_sent_count} · Input tokens: ${lc.input_tokens.toLocaleString()}
        ${tokenDelta > 0 ? `<br><span style="color:var(--orange)">+${tokenDelta.toLocaleString()} tokens growth from previous turn</span>` : ''}
      </div>`);
    addDetailSection(panel, 'Messages Sent', `<pre>${escHtml(formatJson(lc.messages_sent))}</pre>`);
    const reqLogs = filterLogs(['LLM request', 'LLM stream request', 'sending HTTP', 'before_llm_call']);
    if (reqLogs.length) addDetailSection(panel, 'Related Logs', renderLogEntries(reqLogs));
  });
  turnEl.appendChild(reqEl);

  // Middleware hooks between request and response
  const middlewareLogs = filterLogs(['middleware hook', 'before_llm', 'after_llm']);
  if (middlewareLogs.length) {
    turnEl.appendChild(makeMiddlewareBlock(middlewareLogs, 'LLM Middleware'));
  }

  // Arrow
  turnEl.appendChild(makeArrow());

  // LLM Response block
  const respEl = document.createElement('div');
  respEl.className = 'card event-block';
  const stopColor = lc.stop_reason === 'tool_use' ? 'var(--orange)' : lc.stop_reason === 'end_turn' ? 'var(--green)' : 'var(--red)';
  respEl.style.cssText = `padding:6px 10px;margin-bottom:4px;border-left-color:${stopColor}`;

  let respPreview = '';
  const resp = lc.response;
  if (resp?.content) {
    for (const b of resp.content) {
      if ((b.type === 'text' || b.Text) && (b.text || b.Text?.text)) {
        respPreview = truncate(b.text || b.Text?.text || '', 80);
      }
      if (b.type === 'tool_use') {
        respPreview = `→ ${b.name}(${truncate(JSON.stringify(b.input), 50)})`;
      }
    }
  }

  respEl.innerHTML = `
    <div style="display:flex;justify-content:space-between;font-size:11px">
      <span style="color:var(--green);font-weight:600">LLM Response</span>
      <span style="color:var(--text-dim);font-family:var(--mono)">${lc.output_tokens.toLocaleString()} out · ${(lc.duration_ms/1000).toFixed(1)}s</span>
    </div>
    <div style="margin-top:2px"><span style="background:${stopColor}22;color:${stopColor};font-size:9px;padding:1px 5px;border-radius:6px">${lc.stop_reason}</span></div>
    ${respPreview ? `<div style="font-size:11px;color:var(--text-dim);margin-top:3px;font-family:var(--mono)">${escHtml(respPreview)}</div>` : ''}`;

  respEl.onclick = () => selectBlock(respEl, panel => {
    addDetailSection(panel, `LLM Response — Turn ${turn.turn_number}`, `
      <div style="font-size:12px">
        Stop: <strong style="color:${stopColor}">${lc.stop_reason}</strong> ·
        Tokens: ${lc.output_tokens.toLocaleString()} ·
        Duration: ${(lc.duration_ms/1000).toFixed(1)}s
      </div>`);
    if (resp?.content) {
      for (const b of resp.content) {
        if ((b.type === 'text' || b.Text) && (b.text || b.Text?.text)) {
          addDetailSection(panel, 'Response Text', `<pre>${escHtml(b.text || b.Text?.text || '')}</pre>`);
        }
        if (b.type === 'tool_use') {
          addDetailSection(panel, `Tool Call: ${b.name}`, `<pre>${escHtml(formatJson(b.input))}</pre>`);
        }
      }
    }
    addDetailSection(panel, 'Full Response Object', `<pre>${escHtml(formatJson(resp))}</pre>`);
    const respLogs = filterLogs(['HTTP response', 'after_llm_call', 'fallback', 'LLM stream produced']);
    if (respLogs.length) addDetailSection(panel, 'Related Logs', renderLogEntries(respLogs));
  });
  turnEl.appendChild(respEl);

  // Tool calls
  for (const tc of turn.tool_calls) {
    turnEl.appendChild(makeArrow());

    const toolEl = document.createElement('div');
    toolEl.className = 'card event-block';
    const isErr = tc.is_error;
    const isSubAgent = tc.tool_call.name === 'agent';
    const borderColor = isSubAgent ? 'var(--purple)' : (isErr ? 'var(--red)' : 'var(--orange)');
    toolEl.style.cssText = `padding:6px 10px;margin-bottom:4px;border-left-color:${borderColor}`;

    const statusBadge = isErr ? '<span class="badge badge-err">ERR</span>' : '<span class="badge badge-ok">OK</span>';
    const typeBadge = isSubAgent
      ? '<span style="background:#2d1f3d;color:var(--purple);font-size:9px;padding:1px 5px;border-radius:6px">SUB-AGENT</span>'
      : '<span class="badge badge-tool">TOOL</span>';
    toolEl.innerHTML = `
      <div style="display:flex;justify-content:space-between;font-size:11px">
        <div>${typeBadge} <strong>${escHtml(tc.tool_call.name)}</strong> ${statusBadge}</div>
        <span style="color:var(--text-dim);font-family:var(--mono)">${tc.duration_ms}ms</span>
      </div>
      ${isSubAgent ? `<div style="font-size:10px;color:var(--text-dim);margin-top:2px">${escHtml(truncate(tc.tool_call.arguments.task || tc.tool_call.arguments.description || '', 80))}</div>` : ''}`;

    toolEl.onclick = () => {
      // Tier B: sub-agents with a nested RunRecord open a full-screen
      // modal that reuses renderTimeline on tc.sub_run. No more
      // log-scraping, pixel-perfect parity with the parent view,
      // recursive for grandchildren.
      if (isSubAgent && tc.sub_run) {
        openSubAgentModal(tc);
        return;
      }

      selectBlock(toolEl, panel => {
        addDetailSection(panel, `${isSubAgent ? 'Sub-Agent' : 'Tool'}: ${tc.tool_call.name}`, `
          <div style="font-size:12px">
            Status: ${isErr ? '<span style="color:var(--red)">ERROR</span>' : '<span style="color:var(--green)">OK</span>'} ·
            Duration: ${tc.duration_ms}ms
          </div>`);
        addDetailSection(panel, 'Input Arguments', `<pre>${escHtml(formatJson(tc.tool_call.arguments))}</pre>`);

        // Legacy fallback: sub-agent without structured sub_run (records
        // captured before Tier B). Reconstructs child activity from logs.
        if (isSubAgent) {
          renderSubAgentTimeline(panel, tc);
        }

        const output = tc.result ? formatToolOutput(tc.result) : '(no output)';
        addDetailSection(panel, `Output (${output.length} chars)`, `<pre>${escHtml(output)}</pre>`);
        const toolLogs = filterLogs(['tool execution', tc.tool_call.name, 'before_tool', 'after_tool']);
        if (toolLogs.length) addDetailSection(panel, 'Related Logs', renderLogEntries(toolLogs));
      });
    };
    turnEl.appendChild(toolEl);
  }

  container.appendChild(turnEl);
}

// ---------------------------------------------------------------------------
// Summary block
// ---------------------------------------------------------------------------

function renderSummaryBlock(record, container) {
  const totalTokens = record.total_input_tokens + record.total_output_tokens;
  const totalTools = record.turns.reduce((sum, t) => sum + t.tool_calls.length, 0);

  const el = document.createElement('div');
  el.className = 'card end-ok event-block';
  el.style.padding = '8px 12px';
  let tokenBars = '';
  if (record.turns.length > 1) {
    tokenBars = `<div style="margin-top:8px;font-size:10px">
      ${record.turns.map(t => {
        const tok = t.llm_call.input_tokens + t.llm_call.output_tokens;
        const pct = totalTokens > 0 ? (tok / totalTokens * 100).toFixed(0) : 0;
        return `<div style="display:flex;align-items:center;gap:4px;margin-bottom:2px">
          <span style="width:40px;color:var(--text-dim)">T${t.turn_number}</span>
          <div style="flex:1;height:5px;background:var(--bg3);border-radius:3px;overflow:hidden"><div style="height:100%;width:${pct}%;background:var(--blue);border-radius:3px"></div></div>
          <span style="width:55px;text-align:right;font-family:var(--mono);color:var(--text-dim)">${tok.toLocaleString()}</span>
        </div>`;
      }).join('')}
    </div>`;
  }

  el.innerHTML = `
    <div style="display:flex;gap:16px;font-size:12px;justify-content:center">
      <div style="text-align:center"><div style="font-size:18px;font-weight:700">${record.turns.length}</div><div style="font-size:10px;color:var(--text-dim)">Turns</div></div>
      <div style="text-align:center"><div style="font-size:18px;font-weight:700">${totalTokens.toLocaleString()}</div><div style="font-size:10px;color:var(--text-dim)">Tokens</div></div>
      <div style="text-align:center"><div style="font-size:18px;font-weight:700">${(record.total_duration_ms/1000).toFixed(1)}s</div><div style="font-size:10px;color:var(--text-dim)">Duration</div></div>
      <div style="text-align:center"><div style="font-size:18px;font-weight:700">${totalTools}</div><div style="font-size:10px;color:var(--text-dim)">Tool Calls</div></div>
    </div>
    ${tokenBars}`;

  el.onclick = () => selectBlock(el, panel => {
    addDetailSection(panel, 'Summary', `
      <div style="font-size:12px">
        Turns: ${record.turns.length}<br>
        Total tokens: ${totalTokens.toLocaleString()} (${record.total_input_tokens.toLocaleString()} in + ${record.total_output_tokens.toLocaleString()} out)<br>
        Duration: ${(record.total_duration_ms/1000).toFixed(1)}s<br>
        Tool calls: ${totalTools}
      </div>`);
    // Token per turn breakdown
    if (record.turns.length > 1) {
      let breakdown = record.turns.map(t => {
        const tok = t.llm_call.input_tokens + t.llm_call.output_tokens;
        const pct = totalTokens > 0 ? (tok / totalTokens * 100).toFixed(0) : 0;
        return `Turn ${t.turn_number}: ${tok.toLocaleString()} tokens (${pct}%) — ${(t.duration_ms/1000).toFixed(1)}s`;
      }).join('<br>');
      addDetailSection(panel, 'Per-Turn Breakdown', `<div style="font-size:12px;font-family:var(--mono)">${breakdown}</div>`);
    }
    // All logs
    addDetailSection(panel, `All Logs (${allLogs.length})`, renderLogEntries(allLogs));
  });
  container.appendChild(el);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Sub-agent modal — recursive re-use of renderTimeline for tc.sub_run
// ---------------------------------------------------------------------------

/// Open a full-screen modal showing the sub-agent's own timeline.
///
/// The modal reuses the same DOM structure as the main inspector body
/// (`.inspector-body` → `.inspector-timeline` + `.inspector-detail`)
/// so `renderTimeline`, `selectBlock`, and every click handler work
/// recursively with zero per-view plumbing. Nested sub-agents stack:
/// clicking a nested `agent` tool call inside a modal pushes another
/// modal on top, and the top-left ← Back button pops one layer at a
/// time. ESC also pops the topmost.
function openSubAgentModal(tc) {
  const subRun = tc.sub_run;
  if (!subRun) return;

  const role = (tc.tool_call.arguments && tc.tool_call.arguments.role) || 'sub-agent';
  const task = (tc.tool_call.arguments && (tc.tool_call.arguments.task || tc.tool_call.arguments.description)) || '';

  const depth = subAgentModalStack.length + 1;
  const zIndex = 1000 + depth * 10;

  const modal = document.createElement('div');
  modal.className = 'sub-agent-modal';
  modal.style.zIndex = zIndex;
  modal.innerHTML = `
    <div class="modal-header">
      <button class="btn-sm modal-close" title="Close (Esc)">← Back</button>
      <div class="modal-title">
        <span style="background:#2d1f3d;color:var(--purple);font-size:10px;padding:2px 6px;border-radius:6px;font-weight:600">SUB-AGENT · L${depth}</span>
        <strong style="color:var(--purple)">${escHtml(role)}</strong>
        <span style="color:var(--text-dim);font-size:12px">${escHtml(truncate(task, 120))}</span>
      </div>
    </div>
    <div class="inspector-body">
      <div class="inspector-timeline"></div>
      <div class="inspector-detail">
        <div style="color:var(--text-dim);font-size:12px;padding:16px">Click any block to see details</div>
      </div>
    </div>`;

  modal.querySelector('.modal-close').onclick = () => closeTopSubAgentModal();

  const timelineCol = modal.querySelector('.inspector-timeline');
  document.body.appendChild(modal);
  subAgentModalStack.push(modal);

  // Reuse the same renderTimeline that powers the parent view — the
  // child's RunRecord has the identical shape so nothing special is
  // needed here. Nested sub-agents inside this timeline will trigger
  // openSubAgentModal again via the same click handler in
  // renderTurnBlock.
  renderTimeline(subRun, timelineCol);
}

function closeTopSubAgentModal() {
  const modal = subAgentModalStack.pop();
  if (modal) modal.remove();
}

// ESC key pops the topmost open sub-agent modal.
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape' && subAgentModalStack.length > 0) {
    e.preventDefault();
    closeTopSubAgentModal();
  }
});

// ---------------------------------------------------------------------------
// Sub-agent timeline (legacy — used only for records captured BEFORE Tier B
// when tc.sub_run is not populated; kept for historical record inspection).
// ---------------------------------------------------------------------------

function renderSubAgentTimeline(panel, tc) {
  // Find all child-agent activity logs.
  // These are LLM requests, tool executions, and turn completions that happened
  // during this agent tool call. We identify them by looking for logs between
  // the parent's "tool execution completed" events.
  //
  // Heuristic: find all "LLM stream request" / "tool execution completed" /
  // "turn completed" / "HTTP response" / "fallback" logs that are NOT the
  // parent's own turns. The child's logs are interleaved with parent logs,
  // but we can distinguish them because:
  // - Child's "turn completed" logs have lower turn numbers (restart from 1)
  // - Child's LLM requests happen after parent's ToolExecutionStart for "agent"

  const childLogs = allLogs.filter(l => {
    const msg = l.message.toLowerCase();
    return msg.includes('llm stream request') ||
           msg.includes('llm request body') ||
           msg.includes('http response') ||
           msg.includes('fallback') ||
           msg.includes('tool execution completed') ||
           msg.includes('turn completed') ||
           msg.includes('sending http') ||
           msg.includes('empty response');
  });

  if (childLogs.length === 0) {
    addDetailSection(panel, 'Sub-Agent Activity', '<div style="color:var(--text-dim)">No child agent logs captured</div>');
    return;
  }

  // Build a mini-timeline
  let html = '<div style="border-left:2px solid var(--purple);padding-left:8px;margin-left:4px">';

  // Group into "turns" by looking for "turn completed" entries
  let currentTurn = [];
  let turnNum = 0;

  for (const log of childLogs) {
    const msg = log.message;

    if (msg.includes('turn completed')) {
      turnNum++;
      const turnDuration = log.fields.duration_ms || '?';
      const toolCalls = log.fields.tool_calls || '0';
      html += `<div style="margin-bottom:6px">`;
      html += `<div style="font-size:11px;font-weight:600;color:var(--purple)">Child Turn ${turnNum} <span style="font-weight:400;color:var(--text-dim)">${turnDuration}ms · ${toolCalls} tools</span></div>`;

      // Show the events that led to this turn
      for (const ev of currentTurn) {
        const color = { 'ERROR': 'var(--red)', 'WARN': 'var(--orange)', 'INFO': 'var(--blue)', 'DEBUG': 'var(--text-dim)' }[ev.level] || 'var(--text-dim)';
        const shortFields = Object.entries(ev.fields)
          .filter(([k]) => !['body_preview', 'remaining_buffer', 'preview'].includes(k))
          .map(([k, v]) => `${k}=${truncate(String(v).replace(/^"|"$/g, ''), 40)}`)
          .join(' ');
        html += `<div style="font-size:10px;font-family:var(--mono);padding:1px 0;color:var(--text-dim)">
          <span style="color:${color}">${ev.level}</span> ${escHtml(ev.message)} <span style="opacity:0.6">${shortFields}</span>
        </div>`;
      }
      html += `</div>`;
      currentTurn = [];
    } else {
      currentTurn.push(log);
    }
  }

  // Any remaining events after last turn
  if (currentTurn.length > 0) {
    for (const ev of currentTurn) {
      const color = { 'ERROR': 'var(--red)', 'WARN': 'var(--orange)', 'INFO': 'var(--blue)', 'DEBUG': 'var(--text-dim)' }[ev.level] || 'var(--text-dim)';
      html += `<div style="font-size:10px;font-family:var(--mono);padding:1px 0;color:var(--text-dim)">
        <span style="color:${color}">${ev.level}</span> ${escHtml(ev.message)}
      </div>`;
    }
  }

  html += '</div>';
  addDetailSection(panel, `Sub-Agent Activity (${childLogs.length} events)`, html);
}

function makeArrow() {
  const arrow = document.createElement('div');
  arrow.style.cssText = 'text-align:center;color:var(--text-dim);font-size:12px;line-height:1;margin:1px 0';
  arrow.textContent = '↓';
  return arrow;
}

function makeMiddlewareBlock(logs, label) {
  const el = document.createElement('div');
  el.className = 'card event-block';
  el.style.cssText = 'padding:4px 10px;margin-bottom:2px;border-left-color:var(--text-dim);background:transparent;border:1px dashed var(--border);font-size:10px;color:var(--text-dim)';
  el.innerHTML = `${label} (${logs.length} hooks)`;
  el.onclick = () => selectBlock(el, panel => {
    addDetailSection(panel, label, renderLogEntries(logs));
  });
  return el;
}

// ---------------------------------------------------------------------------
// Bootstrap
// ---------------------------------------------------------------------------

loadRuns();
const params = new URLSearchParams(window.location.search);
const autoRun = params.get('run');
if (autoRun) loadRecord(autoRun);
