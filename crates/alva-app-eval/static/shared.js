// ---------------------------------------------------------------------------
// shared.js — Common utilities used across all pages
// ---------------------------------------------------------------------------

function escHtml(s) {
  const d = document.createElement('div');
  d.textContent = s;
  return d.innerHTML;
}

function truncate(s, max) {
  if (!s) return '';
  return s.length > max ? s.slice(0, max) + '...' : s;
}

function formatJson(obj) {
  try {
    if (typeof obj === 'string') return obj;
    return JSON.stringify(obj, null, 2);
  } catch { return String(obj); }
}

function formatToolOutput(result) {
  if (!result || !result.content) return '(empty)';
  return result.content.map(c => {
    if (c.type === 'text' || c.Text) return (c.text || c.Text?.text || '');
    if (c.type === 'image' || c.Image) return '[image]';
    return JSON.stringify(c);
  }).join('\n');
}

function scrollBottom() {
  const el = document.getElementById('events');
  if (el) el.scrollTop = el.scrollHeight;
}

// ---------------------------------------------------------------------------
// Nav active state
// ---------------------------------------------------------------------------

function setActiveNav() {
  const path = window.location.pathname;
  document.querySelectorAll('.nav-link').forEach(a => {
    a.classList.toggle('active', a.getAttribute('href') === path ||
      (path === '/' && a.getAttribute('href') === '/'));
  });
}
document.addEventListener('DOMContentLoaded', setActiveNav);
