// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

let currentSource = null;
let stats = { turns: 0, toolCalls: 0, tokens: 0, startTime: 0 };
let currentMessageEl = null;
let currentStreamingEl = null;
let lastRunId = null;

// ---------------------------------------------------------------------------
// Init: load tool list
// ---------------------------------------------------------------------------

const CORE_TOOLS = [
  'read_file', 'grep_search', 'list_files', 'find_files',
  'create_file', 'file_edit', 'execute_shell',
];

async function loadTools() {
  try {
    const res = await fetch('/api/tools');
    const tools = await res.json();
    const picker = document.getElementById('tool-picker');
    picker.innerHTML = '';
    tools.forEach(t => {
      const label = document.createElement('label');
      const checked = CORE_TOOLS.includes(t.name) ? 'checked' : '';
      label.innerHTML = `
        <input type="checkbox" value="${t.name}" ${checked}>
        <span>
          <strong>${t.name}</strong>
          <span class="tool-desc">${escHtml(truncate(t.description, 80))}</span>
        </span>`;
      picker.appendChild(label);
    });
  } catch (e) {
    console.error('Failed to load tools:', e);
  }
}

function selectAllTools() {
  document.querySelectorAll('#tool-picker input').forEach(c => c.checked = true);
}
function selectNoTools() {
  document.querySelectorAll('#tool-picker input').forEach(c => c.checked = false);
}
function selectCoreTools() {
  document.querySelectorAll('#tool-picker input').forEach(c => {
    c.checked = CORE_TOOLS.includes(c.value);
  });
}

// ---------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------

async function startRun() {
  // Persist all settings before running
  persistSettings();

  // Close previous
  if (currentSource) {
    currentSource.close();
    currentSource = null;
  }

  const btn = document.getElementById('btn-run');
  btn.disabled = true;
  btn.textContent = 'Running...';

  const eventsEl = document.getElementById('events');
  eventsEl.innerHTML = '';
  currentMessageEl = null;
  currentStreamingEl = null;
  stats = { turns: 0, toolCalls: 0, tokens: 0, startTime: Date.now() };
  updateStats();
  setStatus('Starting...', 'var(--blue)');

  const provider = document.querySelector('input[name="provider"]:checked').value;
  const selectedTools = Array.from(
    document.querySelectorAll('#tool-picker input:checked')
  ).map(c => c.value);

  const apiKey = document.getElementById('apikey').value.trim();
  const baseUrl = document.getElementById('baseurl').value.trim();
  const workspace = document.getElementById('workspace').value.trim();

  const body = {
    provider,
    model: document.getElementById('model').value.trim(),
    system_prompt: document.getElementById('system').value,
    user_prompt: document.getElementById('prompt').value,
    tools: selectedTools.length > 0 ? selectedTools : null,
    max_iterations: parseInt(document.getElementById('maxiter').value) || 10,
  };

  if (apiKey) body.api_key = apiKey;
  if (baseUrl) body.base_url = baseUrl;
  if (workspace) body.workspace = workspace;

  try {
    const res = await fetch('/api/run', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });

    if (!res.ok) {
      const text = await res.text();
      addErrorCard('Failed to start: ' + text);
      resetButton();
      return;
    }

    const { run_id, tools } = await res.json();
    lastRunId = run_id;
    addInfoCard(`Agent started with ${tools.length} tools: ${tools.join(', ')}`);

    // Connect SSE
    currentSource = new EventSource(`/api/events/${run_id}`);
    currentSource.onmessage = (e) => {
      try {
        const event = JSON.parse(e.data);
        handleEvent(event);
      } catch (err) {
        console.error('Failed to parse event:', err, e.data);
      }
    };
    currentSource.onerror = () => {
      currentSource.close();
      currentSource = null;
      resetButton();
    };
  } catch (e) {
    addErrorCard('Network error: ' + e.message);
    resetButton();
  }
}

function resetButton() {
  const btn = document.getElementById('btn-run');
  btn.disabled = false;
  btn.textContent = 'Run';
}

// ---------------------------------------------------------------------------
// Event handling
// ---------------------------------------------------------------------------

function handleEvent(event) {
  const type = event.type;
  const eventsEl = document.getElementById('events');

  switch (type) {
    case 'AgentStart':
      setStatus('Running', 'var(--green)');
      break;

    case 'AgentEnd': {
      if (currentSource) { currentSource.close(); currentSource = null; }
      currentMessageEl = null;
      currentStreamingEl = null;
      const elapsed = ((Date.now() - stats.startTime) / 1000).toFixed(1);
      if (event.error) {
        addCard('end-err', 'Agent Error', event.error);
        setStatus('Error', 'var(--red)');
      } else {
        addCard('end-ok', 'Agent Finished',
          `${stats.turns} turns, ${stats.toolCalls} tool calls, ~${stats.tokens} tokens, ${elapsed}s`);
        // Add Inspector link
        if (lastRunId) {
          const link = document.createElement('a');
          link.href = `/inspector.html?run=${lastRunId}`;
          link.textContent = 'View Details in Inspector';
          link.style.cssText = 'color:var(--blue);font-size:12px;display:block;margin-top:6px';
          document.getElementById('events').lastElementChild.appendChild(link);
        }
        setStatus('Done', 'var(--green)');
      }
      resetButton();
      break;
    }

    case 'TurnStart':
      stats.turns++;
      updateStats();
      eventsEl.appendChild(makeTurnSeparator());
      break;

    case 'TurnEnd':
      break;

    case 'MessageStart': {
      currentMessageEl = document.createElement('div');
      currentMessageEl.className = 'card message';
      const header = document.createElement('div');
      header.className = 'card-header';
      header.textContent = 'Assistant';
      currentMessageEl.appendChild(header);

      currentStreamingEl = document.createElement('div');
      currentStreamingEl.className = 'streaming-text';
      currentMessageEl.appendChild(currentStreamingEl);

      eventsEl.appendChild(currentMessageEl);
      scrollBottom();
      break;
    }

    case 'MessageUpdate': {
      const delta = event.delta;
      if (!delta) break;

      // Handle different delta types (serde externally tagged enum)
      if (typeof delta === 'string') {
        // Unit variants: "Start", "Done" — ignore
      } else if (delta.TextDelta) {
        if (currentStreamingEl) {
          currentStreamingEl.textContent += delta.TextDelta.text;
          scrollBottom();
        }
      } else if (delta.ReasoningDelta) {
        if (currentStreamingEl) {
          currentStreamingEl.textContent += delta.ReasoningDelta.text;
          scrollBottom();
        }
      } else if (delta.Usage) {
        const u = delta.Usage;
        stats.tokens = u.total_tokens || (u.input_tokens || 0) + (u.output_tokens || 0);
        updateStats();
      } else if (delta.ToolCallDelta) {
        const tc = delta.ToolCallDelta;
        if (tc.name && currentStreamingEl) {
          currentStreamingEl.textContent += `\n[calling ${tc.name}...]`;
        }
      }
      break;
    }

    case 'MessageEnd':
      currentMessageEl = null;
      currentStreamingEl = null;
      break;

    case 'MessageError':
      currentMessageEl = null;
      currentStreamingEl = null;
      addCard('msg-error', 'Message Error', event.error);
      break;

    case 'ToolExecutionStart': {
      stats.toolCalls++;
      updateStats();
      const tc = event.tool_call;
      const card = document.createElement('div');
      card.className = 'card tool-start';
      card.innerHTML = `
        <div class="card-header">
          <span class="badge badge-tool">TOOL</span>
          ${escHtml(tc.name)}
        </div>
        <pre>${escHtml(formatJson(tc.arguments))}</pre>`;
      eventsEl.appendChild(card);
      scrollBottom();
      break;
    }

    case 'ToolExecutionUpdate':
      break;

    case 'ToolExecutionEnd': {
      const tc = event.tool_call;
      const result = event.result;
      const isError = result && result.is_error;
      const card = document.createElement('div');
      card.className = `card tool-end ${isError ? 'error' : ''}`;
      const badgeClass = isError ? 'badge-err' : 'badge-ok';
      const badgeText = isError ? 'ERROR' : 'OK';
      const text = result ? formatToolOutput(result) : '(no output)';
      card.innerHTML = `
        <div class="card-header">
          <span class="badge ${badgeClass}">${badgeText}</span>
          ${escHtml(tc.name)} result
        </div>
        <pre>${escHtml(truncate(text, 2000))}</pre>`;
      eventsEl.appendChild(card);
      scrollBottom();
      break;
    }

    default:
      console.log('Unknown event type:', type, event);
  }
}

// ---------------------------------------------------------------------------
// DOM helpers
// ---------------------------------------------------------------------------

function addCard(cls, title, body) {
  const el = document.createElement('div');
  el.className = `card ${cls}`;
  el.innerHTML = `<div class="card-header">${escHtml(title)}</div>`;
  if (body) {
    const p = document.createElement('div');
    p.style.marginTop = '4px';
    p.style.fontSize = '13px';
    p.textContent = body;
    el.appendChild(p);
  }
  document.getElementById('events').appendChild(el);
  scrollBottom();
}

function addInfoCard(text) {
  const el = document.createElement('div');
  el.className = 'card start';
  el.innerHTML =
    `<div class="card-header">Info</div>` +
    `<div style="margin-top:4px;font-size:12px;color:var(--text-dim)">${escHtml(text)}</div>`;
  document.getElementById('events').appendChild(el);
}

function addErrorCard(text) {
  addCard('end-err', 'Error', text);
  setStatus('Error', 'var(--red)');
}

function makeTurnSeparator() {
  const el = document.createElement('div');
  el.className = 'card turn';
  el.textContent = `Turn ${stats.turns}`;
  return el;
}

// ---------------------------------------------------------------------------
// UI state helpers
// ---------------------------------------------------------------------------

function setStatus(text, color) {
  const el = document.getElementById('status');
  el.textContent = text;
  el.style.color = color || 'var(--text-dim)';
}

function updateStats() {
  const el = document.getElementById('stats');
  const elapsed = stats.startTime
    ? ((Date.now() - stats.startTime) / 1000).toFixed(1)
    : '0.0';
  el.innerHTML = `
    Turns: <span class="val">${stats.turns}</span> &nbsp;
    Tools: <span class="val">${stats.toolCalls}</span> &nbsp;
    Tokens: <span class="val">${stats.tokens}</span> &nbsp;
    Time: <span class="val">${elapsed}s</span>`;
}

// ---------------------------------------------------------------------------
// Event listeners
// ---------------------------------------------------------------------------

// Provider switch: update default model
document.querySelectorAll('input[name="provider"]').forEach(r => {
  r.addEventListener('change', () => {
    const model = document.getElementById('model');
    if (r.value === 'anthropic' && model.value.startsWith('gpt')) {
      model.value = 'claude-sonnet-4-20250514';
    } else if (r.value === 'openai' && model.value.startsWith('claude')) {
      model.value = 'gpt-4o';
    }
  });
});

// Ctrl+Enter / Cmd+Enter to run
document.addEventListener('keydown', (e) => {
  if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
    e.preventDefault();
    startRun();
  }
});

// ---------------------------------------------------------------------------
// Directory browser
// ---------------------------------------------------------------------------

async function openBrowser() {
  const browser = document.getElementById('dir-browser');
  browser.style.display = 'block';
  const current = document.getElementById('workspace').value.trim() || null;
  await browseDir(current);
}

async function browseDir(path) {
  const browser = document.getElementById('dir-browser');
  try {
    const res = await fetch('/api/browse', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path }),
    });
    if (!res.ok) { browser.innerHTML = `<div style="color:var(--red);font-size:12px;padding:6px">${await res.text()}</div>`; return; }
    const data = await res.json();
    renderBrowser(data);
  } catch (e) {
    browser.innerHTML = `<div style="color:var(--red);font-size:12px;padding:6px">${e.message}</div>`;
  }
}

function renderBrowser(data) {
  const browser = document.getElementById('dir-browser');
  let html = `<div style="font-size:11px;color:var(--text-dim);padding:4px 6px;border-bottom:1px solid var(--border);display:flex;justify-content:space-between;align-items:center">
    <span style="word-break:break-all">${escHtml(data.current)}</span>
    <button onclick="selectWorkspace('${escAttr(data.current)}')" style="background:var(--blue);border:none;color:#fff;border-radius:4px;padding:2px 8px;cursor:pointer;font-size:11px;white-space:nowrap;margin-left:8px">Select</button>
  </div>`;

  if (data.parent) {
    html += `<div onclick="browseDir('${escAttr(data.parent)}')" style="padding:4px 6px;cursor:pointer;font-size:12px;color:var(--blue)" onmouseover="this.style.background='var(--bg3)'" onmouseout="this.style.background=''">.. (parent)</div>`;
  }

  for (const entry of data.entries) {
    if (entry.is_dir) {
      html += `<div onclick="browseDir('${escAttr(entry.path)}')" style="padding:3px 6px;cursor:pointer;font-size:12px" onmouseover="this.style.background='var(--bg3)'" onmouseout="this.style.background=''">📁 ${escHtml(entry.name)}</div>`;
    }
  }

  browser.innerHTML = html;
}

function selectWorkspace(path) {
  document.getElementById('workspace').value = path;
  document.getElementById('dir-browser').style.display = 'none';
}

function escAttr(s) {
  return s.replace(/\\/g, '\\\\').replace(/'/g, "\\'");
}

// ---------------------------------------------------------------------------
// Skills
// ---------------------------------------------------------------------------

async function scanSkills() {
  const picker = document.getElementById('skill-picker');
  picker.innerHTML = '<div style="color:var(--text-dim);font-size:12px;padding:8px">Scanning...</div>';
  try {
    const sourcesRes = await fetch('/api/skills/sources');
    const sources = await sourcesRes.json();

    let allSkills = [];
    for (const source of sources) {
      if (source.exists) {
        const res = await fetch('/api/skills/scan', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ path: source.path }),
        });
        const skills = await res.json();
        allSkills.push(...skills);
      }
    }

    picker.innerHTML = '';
    if (allSkills.length === 0) {
      picker.innerHTML = '<div style="color:var(--text-dim);font-size:12px;padding:8px">No skills found</div>';
      return;
    }
    allSkills.forEach(s => {
      const label = document.createElement('label');
      label.innerHTML = `
        <input type="checkbox" value="${s.name}" checked>
        <span>
          <strong>${s.name}</strong>
          <span class="tool-desc">${escHtml(truncate(s.description, 80))}</span>
        </span>`;
      picker.appendChild(label);
    });
  } catch (e) {
    picker.innerHTML = `<div style="color:var(--red);font-size:12px;padding:8px">Error: ${e.message}</div>`;
  }
}

// ---------------------------------------------------------------------------
// Auto-save settings on input changes
// ---------------------------------------------------------------------------

// Debounce helper
let _saveTimer = null;
function autoSave() {
  clearTimeout(_saveTimer);
  _saveTimer = setTimeout(() => persistSettings(), 500);
}

// Attach auto-save to settings fields (not profile fields — those use Save button)
['system', 'workspace', 'maxiter'].forEach(id => {
  const el = document.getElementById(id);
  if (el) el.addEventListener('input', autoSave);
});

// Auto-save tool selection changes
document.getElementById('tool-picker')?.addEventListener('change', autoSave);

// ---------------------------------------------------------------------------
// Bootstrap
// ---------------------------------------------------------------------------

// 1. Render profile selector and load saved profile (async — decrypts key)
renderProfileSelect();
loadProfile().then(() => {
  // 2. Restore non-profile settings (workspace, system prompt, max iterations)
  restoreSettings();
});

// 3. Load tools, then restore saved tool selection
loadTools().then(() => restoreToolSelection());
