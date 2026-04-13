// ---------------------------------------------------------------------------
// storage.js — Persist settings to localStorage with provider profiles
// ---------------------------------------------------------------------------

const STORAGE_KEY = 'alva_app_eval';
const CRYPTO_KEY_NAME = 'alva_eval_ck';

// ---------------------------------------------------------------------------
// AES-GCM encryption via Web Crypto API
//
// Key derivation: PBKDF2 from a stable device fingerprint (user agent +
// screen + timezone + language). The encrypted data is only decryptable
// on the same browser/device profile.
// ---------------------------------------------------------------------------

async function _getDeviceFingerprint() {
  const parts = [
    navigator.userAgent,
    screen.width + 'x' + screen.height + 'x' + screen.colorDepth,
    Intl.DateTimeFormat().resolvedOptions().timeZone,
    navigator.language,
    'alva-eval-salt-2026',
  ];
  return parts.join('|');
}

async function _deriveKey() {
  const fp = await _getDeviceFingerprint();
  const enc = new TextEncoder();
  const keyMaterial = await crypto.subtle.importKey(
    'raw', enc.encode(fp), 'PBKDF2', false, ['deriveKey']
  );
  return crypto.subtle.deriveKey(
    { name: 'PBKDF2', salt: enc.encode('alva-eval-v1'), iterations: 100000, hash: 'SHA-256' },
    keyMaterial,
    { name: 'AES-GCM', length: 256 },
    false,
    ['encrypt', 'decrypt']
  );
}

// Cache the derived key for the session (avoid re-deriving on every call)
let _cachedKey = null;
async function getCryptoKey() {
  if (!_cachedKey) _cachedKey = await _deriveKey();
  return _cachedKey;
}

/** Encrypt a string → base64(iv + ciphertext). Returns empty string for empty input. */
async function obfuscate(s) {
  if (!s) return '';
  try {
    const key = await getCryptoKey();
    const enc = new TextEncoder();
    const iv = crypto.getRandomValues(new Uint8Array(12));
    const ciphertext = await crypto.subtle.encrypt(
      { name: 'AES-GCM', iv },
      key,
      enc.encode(s)
    );
    // Concatenate iv + ciphertext, encode as base64
    const combined = new Uint8Array(iv.length + ciphertext.byteLength);
    combined.set(iv);
    combined.set(new Uint8Array(ciphertext), iv.length);
    return btoa(String.fromCharCode(...combined));
  } catch (e) {
    console.error('Encrypt failed:', e);
    return '';
  }
}

/** Decrypt base64(iv + ciphertext) → plaintext string. Returns empty string on failure. */
async function deobfuscate(s) {
  if (!s) return '';
  try {
    const key = await getCryptoKey();
    const combined = Uint8Array.from(atob(s), c => c.charCodeAt(0));
    const iv = combined.slice(0, 12);
    const ciphertext = combined.slice(12);
    const plainBuf = await crypto.subtle.decrypt(
      { name: 'AES-GCM', iv },
      key,
      ciphertext
    );
    return new TextDecoder().decode(plainBuf);
  } catch (e) {
    // Key mismatch (different device) or corrupted data — silently return empty
    console.warn('Decrypt failed (device changed?), clearing key');
    return '';
  }
}

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

function defaultStore() {
  return {
    profiles: [
      {
        id: 'default',
        name: 'Anthropic Default',
        type: 'anthropic',
        api_key_ob: '',
        model: 'claude-sonnet-4-20250514',
        base_url: '',
      },
    ],
    selected_profile: 'default',
    workspace: '',
    system_prompt: 'You are a helpful coding assistant. Use tools when appropriate.',
    max_iterations: 10,
    selected_extensions: null, // null = use defaults from server
  };
}

function loadStore() {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return defaultStore();
    const data = JSON.parse(raw);
    // Ensure required fields exist (migration safety)
    if (!data.profiles || !data.profiles.length) return defaultStore();
    return data;
  } catch {
    return defaultStore();
  }
}

function saveStore(store) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(store));
}

// ---------------------------------------------------------------------------
// Profile CRUD
// ---------------------------------------------------------------------------

function getProfiles() {
  return loadStore().profiles;
}

function getSelectedProfileId() {
  return loadStore().selected_profile || 'default';
}

function getSelectedProfile() {
  const store = loadStore();
  return store.profiles.find(p => p.id === store.selected_profile) || store.profiles[0];
}

function saveCurrentProfile(profile) {
  const store = loadStore();
  const idx = store.profiles.findIndex(p => p.id === profile.id);
  if (idx >= 0) {
    store.profiles[idx] = profile;
  } else {
    store.profiles.push(profile);
  }
  store.selected_profile = profile.id;
  saveStore(store);
}

function addNewProfile(name) {
  const store = loadStore();
  const id = 'p_' + Date.now();
  const profile = {
    id,
    name,
    type: 'anthropic',
    api_key_ob: '',
    model: 'claude-sonnet-4-20250514',
    base_url: '',
  };
  store.profiles.push(profile);
  store.selected_profile = id;
  saveStore(store);
  return profile;
}

function deleteProfileById(id) {
  const store = loadStore();
  store.profiles = store.profiles.filter(p => p.id !== id);
  if (!store.profiles.length) {
    store.profiles = defaultStore().profiles;
  }
  if (store.selected_profile === id) {
    store.selected_profile = store.profiles[0].id;
  }
  saveStore(store);
}

// ---------------------------------------------------------------------------
// Settings (non-profile fields)
// ---------------------------------------------------------------------------

function saveSettings(settings) {
  const store = loadStore();
  Object.assign(store, settings);
  saveStore(store);
}

function getSettings() {
  const store = loadStore();
  return {
    workspace: store.workspace || '',
    system_prompt: store.system_prompt || '',
    max_iterations: store.max_iterations || 10,
    selected_extensions: store.selected_extensions || null,
  };
}

// ---------------------------------------------------------------------------
// UI integration — called from app.js
// ---------------------------------------------------------------------------

/** Populate the profile <select> dropdown. */
function renderProfileSelect() {
  const select = document.getElementById('profile-select');
  if (!select) return;
  const profiles = getProfiles();
  const selectedId = getSelectedProfileId();
  select.innerHTML = profiles
    .map(p => `<option value="${p.id}" ${p.id === selectedId ? 'selected' : ''}>${escHtml(p.name)}</option>`)
    .join('');
}

/** Load the selected profile into form fields (async — decrypts API key). */
async function loadProfile() {
  const select = document.getElementById('profile-select');
  if (!select) return;
  const store = loadStore();
  store.selected_profile = select.value;
  saveStore(store);

  const profile = getSelectedProfile();
  if (!profile) return;

  // Provider type radio
  const radio = document.querySelector(`input[name="provider"][value="${profile.type}"]`);
  if (radio) radio.checked = true;

  document.getElementById('model').value = profile.model || '';
  document.getElementById('apikey').value = await deobfuscate(profile.api_key_ob);
  document.getElementById('baseurl').value = profile.base_url || '';
}

/** Save current form fields into the selected profile (async — encrypts API key). */
async function saveProfile() {
  const profile = getSelectedProfile();
  if (!profile) return;

  profile.type = document.querySelector('input[name="provider"]:checked')?.value || 'anthropic';
  profile.model = document.getElementById('model').value;
  profile.api_key_ob = await obfuscate(document.getElementById('apikey').value);
  profile.base_url = document.getElementById('baseurl').value;

  const nameInput = prompt('Profile name:', profile.name);
  if (nameInput !== null) profile.name = nameInput;

  saveCurrentProfile(profile);
  renderProfileSelect();
}

/** Create a new empty profile. */
async function addProfile() {
  const name = prompt('New profile name:');
  if (!name) return;
  addNewProfile(name);
  renderProfileSelect();
  await loadProfile();
}

/** Delete the currently selected profile. */
async function deleteProfile() {
  const profile = getSelectedProfile();
  if (!profile) return;
  if (!confirm(`Delete "${profile.name}"?`)) return;
  deleteProfileById(profile.id);
  renderProfileSelect();
  await loadProfile();
}

/** Save non-profile settings (workspace, system prompt, etc.) from the form. */
function persistSettings() {
  const selectedExtensions = Array.from(
    document.querySelectorAll('#extension-picker input:checked')
  ).map(c => c.value);

  saveSettings({
    workspace: document.getElementById('workspace')?.value || '',
    system_prompt: document.getElementById('system')?.value || '',
    max_iterations: parseInt(document.getElementById('maxiter')?.value) || 10,
    selected_extensions: selectedExtensions.length > 0 ? selectedExtensions : null,
  });
}

/** Restore non-profile settings into form fields. Called after tools are loaded. */
function restoreSettings() {
  const s = getSettings();
  if (document.getElementById('workspace')) document.getElementById('workspace').value = s.workspace;
  if (document.getElementById('system')) document.getElementById('system').value = s.system_prompt;
  if (document.getElementById('maxiter')) document.getElementById('maxiter').value = s.max_iterations;
  // Tool checkboxes are restored in app.js after tool list loads
}

/** Restore extension checkbox state from saved settings. Called after extension picker is populated. */
function restoreExtensionSelection() {
  const s = getSettings();
  if (!s.selected_extensions) return; // Use defaults from server
  document.querySelectorAll('#extension-picker input[type="checkbox"]').forEach(cb => {
    cb.checked = s.selected_extensions.includes(cb.value);
  });
}
