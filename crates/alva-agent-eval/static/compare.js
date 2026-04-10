let sourceA = null;
let sourceB = null;
let runIdA = null;
let runIdB = null;
let doneA = false;
let doneB = false;

function buildRunRequest(suffix) {
  return {
    provider: document.querySelector(`input[name="provider-${suffix}"]:checked`).value,
    api_key: document.getElementById(`apikey-${suffix}`).value,
    model: document.getElementById(`model-${suffix}`).value,
    system_prompt: document.getElementById(`system-${suffix}`).value,
    user_prompt: document.getElementById(`prompt-${suffix}`).value,
    max_iterations: parseInt(document.getElementById(`maxiter-${suffix}`).value) || 10,
  };
}

async function startCompare() {
  // Close previous
  if (sourceA) { sourceA.close(); sourceA = null; }
  if (sourceB) { sourceB.close(); sourceB = null; }
  doneA = false;
  doneB = false;

  document.getElementById('events-a').innerHTML = '';
  document.getElementById('events-b').innerHTML = '';
  document.getElementById('diff-summary').style.display = 'none';
  document.getElementById('status').textContent = 'Starting both runs...';
  document.getElementById('status').style.color = 'var(--blue)';

  const btn = document.getElementById('btn-compare');
  btn.disabled = true;
  btn.textContent = 'Running...';

  try {
    const body = {
      run_a: buildRunRequest('a'),
      run_b: buildRunRequest('b'),
    };

    const res = await fetch('/api/compare', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });

    if (!res.ok) {
      document.getElementById('status').textContent = 'Failed: ' + await res.text();
      document.getElementById('status').style.color = 'var(--red)';
      btn.disabled = false; btn.textContent = 'Run Both';
      return;
    }

    const { run_id_a, run_id_b } = await res.json();
    runIdA = run_id_a;
    runIdB = run_id_b;

    // Open two SSE streams
    connectSSE('a', run_id_a);
    connectSSE('b', run_id_b);
  } catch (e) {
    document.getElementById('status').textContent = 'Error: ' + e.message;
    document.getElementById('status').style.color = 'var(--red)';
    btn.disabled = false; btn.textContent = 'Run Both';
  }
}

function connectSSE(suffix, runId) {
  const eventsEl = document.getElementById(`events-${suffix}`);
  const source = new EventSource(`/api/events/${runId}`);

  if (suffix === 'a') sourceA = source;
  else sourceB = source;

  source.onmessage = (e) => {
    try {
      const event = JSON.parse(e.data);
      renderSimpleEvent(event, eventsEl);

      if (event.type === 'AgentEnd') {
        source.close();
        if (suffix === 'a') { sourceA = null; doneA = true; }
        else { sourceB = null; doneB = true; }
        checkBothDone();
      }
    } catch (err) {
      console.error('Parse error:', err);
    }
  };
  source.onerror = () => {
    source.close();
    if (suffix === 'a') { sourceA = null; doneA = true; }
    else { sourceB = null; doneB = true; }
    checkBothDone();
  };
}

function renderSimpleEvent(event, container) {
  const type = event.type;
  if (type === 'AgentStart' || type === 'TurnStart' || type === 'TurnEnd') return;

  const el = document.createElement('div');
  el.style.cssText = 'font-size:12px;padding:4px 8px;border-left:2px solid var(--border);margin-bottom:4px';

  if (type === 'MessageUpdate' && event.delta) {
    if (event.delta.TextDelta) {
      el.style.borderLeftColor = 'var(--blue)';
      el.textContent = truncate(event.delta.TextDelta.text, 100);
    } else return;
  } else if (type === 'ToolExecutionStart') {
    el.style.borderLeftColor = 'var(--orange)';
    el.innerHTML = `<strong>${escHtml(event.tool_call.name)}</strong>`;
  } else if (type === 'ToolExecutionEnd') {
    el.style.borderLeftColor = event.result?.is_error ? 'var(--red)' : 'var(--green)';
    el.textContent = `${event.tool_call.name} → ${event.result?.is_error ? 'ERROR' : 'OK'}`;
  } else if (type === 'AgentEnd') {
    el.style.borderLeftColor = event.error ? 'var(--red)' : 'var(--green)';
    el.innerHTML = `<strong>${event.error ? 'Error' : 'Done'}</strong>`;
  } else return;

  container.appendChild(el);
  container.scrollTop = container.scrollHeight;
}

async function checkBothDone() {
  if (!doneA || !doneB) return;

  const btn = document.getElementById('btn-compare');
  btn.disabled = false;
  btn.textContent = 'Run Both';

  // Fetch diff
  try {
    const res = await fetch(`/api/compare/${runIdA}/${runIdB}`);
    if (!res.ok) return;
    const data = await res.json();

    const diff = data.diff;
    const summary = document.getElementById('diff-summary');
    summary.style.display = 'flex';
    summary.innerHTML = `
      <div class="stats">
        Turns: <span class="val">${diff.turns_a}</span> vs <span class="val">${diff.turns_b}</span> &nbsp;|&nbsp;
        Tokens: <span class="val">${diff.tokens_a}</span> vs <span class="val">${diff.tokens_b}</span> &nbsp;|&nbsp;
        Time: <span class="val">${(diff.duration_a_ms/1000).toFixed(1)}s</span> vs <span class="val">${(diff.duration_b_ms/1000).toFixed(1)}s</span>
        ${diff.tools_only_a.length > 0 ? `&nbsp;|&nbsp; A only: <span style="color:var(--orange)">${diff.tools_only_a.join(', ')}</span>` : ''}
        ${diff.tools_only_b.length > 0 ? `&nbsp;|&nbsp; B only: <span style="color:var(--orange)">${diff.tools_only_b.join(', ')}</span>` : ''}
      </div>`;

    document.getElementById('status').textContent = 'Comparison complete';
    document.getElementById('status').style.color = 'var(--green)';
  } catch (e) {
    console.error('Failed to fetch diff:', e);
  }
}
