// EMU-004 shell: draws frames streamed by the emulator thread, plays their audio
// with Web Audio and sends the held keys back as a button mask (Button::ALL order).
const { invoke, Channel } = window.__TAURI__.core;

const WIDTH = 240, HEIGHT = 160, VIDEO_BYTES = WIDTH * HEIGHT * 4, AUDIO_RATE = 32768;
// Bit order: A, B, Select, Start, Right, Left, Up, Down, R, L.
const KEYS = { KeyX: 0, KeyZ: 1, Backspace: 2, Enter: 3, ArrowRight: 4, ArrowLeft: 5, ArrowUp: 6, ArrowDown: 7, KeyS: 8, KeyA: 9 };
// Fallbacks when the physical code is missing (some input methods, e.g. IBus,
// make WebKitGTK report an empty code): by key name, then by legacy keyCode.
const BY_KEY = { x: 0, z: 1, Backspace: 2, Enter: 3, ArrowRight: 4, ArrowLeft: 5, ArrowUp: 6, ArrowDown: 7, s: 8, a: 9 };
const BY_KEYCODE = { 88: 0, 90: 1, 8: 2, 13: 3, 39: 4, 37: 5, 38: 6, 40: 7, 83: 8, 65: 9 };
function buttonBit(event) {
  if (window.emuControls) return window.emuControls.buttonBit(event);
  if (event.code in KEYS) return KEYS[event.code];
  const key = event.key && event.key.length === 1 ? event.key.toLowerCase() : event.key;
  if (key in BY_KEY) return BY_KEY[key];
  return BY_KEYCODE[event.keyCode];
}

const log = (message) => invoke('log', { message: String(message) });
window.addEventListener('error', (e) => log(`script error: ${e.message} at ${e.filename}:${e.lineno}`));
window.addEventListener('unhandledrejection', (e) => log(`unhandled: ${e.reason}`));

const canvas = document.getElementById('screen');
const ctx2d = canvas.getContext('2d');
const image = ctx2d.createImageData(WIDTH, HEIGHT);
const status = document.getElementById('status');

let audio = null, gainNode = null, nextTime = 0, frames = 0, lastFps = performance.now(), romName = '', seen = 0;

function ensureAudio() {
  if (!audio) {
    audio = new AudioContext();
    gainNode = audio.createGain();
    gainNode.connect(audio.destination);
    audio.onstatechange = () => log(`audio ${audio.state} at ${audio.sampleRate} Hz`);
  }
  if (audio.state === 'suspended') audio.resume();
}

function playAudio(samples) {
  if (window.voiceControls?.active) return;
  const n = samples.length / 2;
  if (!audio || audio.state !== 'running' || n === 0) return;
  if (gainNode) {
    const muted = window.emuControls?.isMuted();
    const vol = window.emuControls ? window.emuControls.getVolume() : 1;
    gainNode.gain.value = muted ? 0 : vol;
  }
  const now = audio.currentTime;
  if (nextTime < now + 0.03) nextTime = now + 0.06;      // underrun: restart with headroom
  if (nextTime > now + 0.25) return;                      // too far ahead: drop to catch up
  const buffer = audio.createBuffer(2, n, AUDIO_RATE);
  const left = buffer.getChannelData(0), right = buffer.getChannelData(1);
  for (let i = 0; i < n; i++) {
    left[i] = samples[2 * i] / 32768;
    right[i] = samples[2 * i + 1] / 32768;
  }
  const source = audio.createBufferSource();
  source.buffer = buffer;
  source.connect(gainNode || audio.destination);
  source.start(nextTime);
  nextTime += n / AUDIO_RATE;
}

function onFrame(data) {
  const bytes = data instanceof ArrayBuffer ? new Uint8Array(data) : new Uint8Array(data);
  image.data.set(bytes.subarray(0, VIDEO_BYTES));
  ctx2d.putImageData(image, 0, 0);
  const audioBytes = bytes.subarray(VIDEO_BYTES);
  playAudio(new Int16Array(audioBytes.buffer.slice(audioBytes.byteOffset, audioBytes.byteOffset + audioBytes.byteLength)));
  frames++;
  if (seen++ === 0) log(`first frame: ${bytes.length} bytes (${data instanceof ArrayBuffer ? 'binary' : typeof data})`);
  const now = performance.now();
  if (now - lastFps >= 1000) {
    const fps = frames * 1000 / (now - lastFps);
    if (seen < 400) log(`${fps.toFixed(1)} fps`);
    if (!window.emuControls) {
      status.textContent = `${romName} · ${fps.toFixed(1)} fps` +
        (audio && audio.state === 'running' ? '' : ' · sound off (click the game)');
    }
    frames = 0;
    lastFps = now;
  }
}

let mask = 0, rawLogs = 0;
const isTextField = (el) => el && ['TEXTAREA', 'INPUT', 'SELECT'].includes(el.tagName);
function updateKey(event, down) {
  if (isTextField(event.target)) return;
  const bit = buttonBit(event);
  if (rawLogs++ < 4) log(`${event.type} code=${JSON.stringify(event.code)} key=${JSON.stringify(event.key)} keyCode=${event.keyCode} -> ${bit}`);
  if (bit === undefined) return;
  event.preventDefault();
  const next = down ? mask | (1 << bit) : mask & ~(1 << bit);
  if (next !== mask) {
    mask = next;
    invoke('set_buttons', { mask }).catch((err) => log(`set_buttons failed: ${err}`));
  }
}
window.addEventListener('keydown', (e) => { ensureAudio(); updateKey(e, true); });
window.addEventListener('keyup', (e) => updateKey(e, false));
// On Linux the app reads the keyboard on the GTK window (main.rs) and these
// handlers never see the mapped keys; elsewhere they are the input path.
window.addEventListener('blur', () => { if (mask) { mask = 0; invoke('set_buttons', { mask }); } });
canvas.tabIndex = 0;
canvas.addEventListener('mousedown', () => canvas.focus());
window.addEventListener('load', () => { window.focus(); canvas.focus(); });
canvas.addEventListener('click', ensureAudio);

document.getElementById('open').addEventListener('click', (e) => { ensureAudio(); document.getElementById('file').click(); e.target.blur(); });
document.getElementById('file').addEventListener('change', async (e) => {
  const input = e.target;
  const file = input.files[0];
  if (!file) return;
  try {
    const bytes = new Uint8Array(await file.arrayBuffer());
    await invoke('load_rom', bytes);
    romName = file.name;
    status.textContent = romName;
    await invoke('emu_set_rom_name', { name: file.name }).catch(() => {});
    window.emuControls?.refreshSlots();
  } catch (err) {
    log(`load_rom failed for ${file.name}: ${err}`);
    status.textContent = `Cannot load ${file.name}: ${err}`;
    window.emuControls?.showError(`Cannot load ${file.name}: ${err}`);
  } finally {
    input.value = '';
  }
});

const channel = new Channel();
channel.onmessage = onFrame;
invoke('attach', { onFrame: channel })
  .then((name) => { if (name) { romName = name; status.textContent = name; } })
  .catch((err) => { status.textContent = `${err}`; });

// APP-004: Change history view over G3-001 snapshot commits with non-destructive undo.
const historyPanel = document.getElementById('history-panel');
const historyList = document.getElementById('history-list');
const historyStatus = document.getElementById('history-status');
let historyRestoring = false;

async function loadHistory() {
  historyList.innerHTML = '';
  try {
    const entries = await invoke('history_list', { limit: 30 });
    if (!entries.length) {
      const li = document.createElement('li');
      li.className = 'history-row';
      li.textContent = 'No commits recorded yet.';
      historyList.appendChild(li);
      return;
    }
    for (const entry of entries) {
      const li = document.createElement('li');
      li.className = 'history-row';
      const badge = document.createElement('span');
      badge.className = 'history-origin';
      badge.textContent = entry.provenance || entry.origin;
      const summary = document.createElement('span');
      summary.className = 'history-summary';
      summary.textContent = entry.summary;
      const sha = document.createElement('span');
      sha.className = 'history-sha';
      sha.textContent = entry.short_commit;
      const btn = document.createElement('button');
      btn.textContent = 'Restore state';
      btn.title = `Restore files to commit ${entry.short_commit}, including this commit's changes`;
      btn.disabled = historyRestoring;
      btn.addEventListener('click', async () => {
        if (historyRestoring) return;
        historyRestoring = true;
        historyStatus.classList.remove('error');
        historyStatus.textContent = `Restoring state ${entry.short_commit}…`;
        historyList.querySelectorAll('button').forEach(button => { button.disabled = true; });
        try {
          const receipt = await invoke('history_undo', { targetCommit: entry.commit });
          historyStatus.textContent = receipt.undo_commit
            ? `Restored state ${entry.short_commit}. Recorded undo ${receipt.undo_commit.slice(0, 8)}. Source files restored; rebuild and reload to update the preview.`
            : `No files changed: sources already match state ${entry.short_commit}.`;
          await loadHistory();
        } catch (err) {
          historyStatus.classList.add('error');
          historyStatus.textContent = `Restore failed: ${err}`;
        } finally {
          historyRestoring = false;
          historyList.querySelectorAll('button').forEach(button => { button.disabled = false; });
        }
        canvas.focus();
      });
      li.append(badge, summary, sha, btn);
      historyList.appendChild(li);
    }
  } catch (err) {
    const li = document.createElement('li');
    li.className = 'history-row';
    li.textContent = `${err}`;
    historyList.appendChild(li);
  }
}

document.getElementById('toggle-history').addEventListener('click', (e) => {
  historyPanel.classList.toggle('open');
  if (historyPanel.classList.contains('open')) loadHistory();
  e.target.blur();
  canvas.focus();
});
document.getElementById('refresh-history').addEventListener('click', (e) => {
  loadHistory();
  e.target.blur();
  canvas.focus();
});


// AGT-003: chat with the agent. Each turn's events (text and thinking deltas, tool
// calls, status, errors, completion) arrive on a channel as they stream.
const chatLog = document.getElementById('chat-log');
const chatInput = document.getElementById('chat-input');
const chatSend = document.getElementById('chat-send');
const chatStop = document.getElementById('chat-stop');
let authBusy = false;
const chatModel = document.getElementById('chat-model');
let modelsReady = false, modelGeneration = 0, currentModel = '';
let chatBusy = false, chatGeneration = 0;   // a new chat ignores the old turn's late events

invoke('chat_info').then((project) => {
  document.getElementById('chat-scope').textContent = project
    ? `Project · ${project.split(/[\\/]/).filter(Boolean).pop()}`
    : 'No project open: start with --project to give the agent the Tauros tools';
});

// Text fields take the keyboard: the app stops reading game keys while one has focus.
for (const field of document.querySelectorAll('textarea, input:not([type=file]), select')) {
  field.addEventListener('focus', () => invoke('set_typing', { typing: true }));
  field.addEventListener('blur', () => invoke('set_typing', { typing: !!window.voiceControls?.active }));
}

function chatAppend(className, text) {
  const div = document.createElement('div');
  div.className = `msg ${className}`;
  div.textContent = text;
  chatLog.appendChild(div);
  chatScroll();
  return div;
}

function chatScroll() {
  chatLog.scrollTop = chatLog.scrollHeight;
}

const toolName = (name) => (name.startsWith('mcp__') ? name.split('__').slice(2).join('__') || name : name);

// Tool results are usually MCP content blocks; show their text, else the JSON.
function formatOutput(output) {
  if (Array.isArray(output) && output.every((b) => b && b.type === 'text')) return output.map((b) => b.text).join('\n');
  if (typeof output === 'string') return output;
  return JSON.stringify(output, null, 2);
}

function setChatBusy(busy) {
  chatBusy = busy;
  chatSend.disabled = busy || authBusy;
  document.getElementById('chat-provider').disabled = busy || authBusy;
  document.getElementById('chat-new').disabled = authBusy;
  document.getElementById('auth-login').disabled = busy || authBusy;
  chatModel.disabled = busy || authBusy || !modelsReady;
  chatStop.hidden = !busy;
  updatePreviewControls();
  window.emuControls?.setBusy(busy);
  window.voiceControls?.refresh();
}

function startTurn(text) {
  chatAppend('user', text);
  let agent = null;      // the assistant bubble receiving text deltas
  const tools = new Map();
  const channel = new Channel();
  const generation = chatGeneration;
  channel.onmessage = (event) => {
    if (generation !== chatGeneration) return;
    switch (event.type) {
      case 'text_delta':
        if (!agent) agent = chatAppend('agent streaming', '');
        agent.textContent += event.delta;
        break;
      case 'tool_call_started': {
        if (agent) { agent.classList.remove('streaming'); agent = null; }
        const details = document.createElement('details');
        details.className = 'tool';
        const summary = document.createElement('summary');
        const name = document.createElement('span');
        name.className = 'tool-name';
        name.textContent = toolName(event.name);
        const state = document.createElement('span');
        state.className = 'tool-state';
        state.textContent = 'running…';
        summary.append(name, state);
        const args = document.createElement('pre');
        args.textContent = `arguments: ${JSON.stringify(event.arguments, null, 2)}`;
        details.append(summary, args);
        chatLog.appendChild(details);
        tools.set(event.call_id, { details, state });
        break;
      }
      case 'tool_call_completed': {
        const tool = tools.get(event.call_id);
        if (!tool) break;
        tool.details.classList.add(event.is_error ? 'failed' : 'ok');
        tool.state.textContent = event.is_error ? 'failed' : 'done';
        const out = document.createElement('pre');
        out.textContent = `result: ${formatOutput(event.output)}`;
        tool.details.appendChild(out);
        if (event.is_error) tool.details.open = true;
        break;
      }
      case 'error':
        if (event.code === 'auth_required') handleAuthRequired(event.message);
        chatAppend('error', event.message);
        break;
      case 'turn_completed': {
        if (agent) agent.classList.remove('streaming');
        const usage = event.usage ? ` · ${event.usage.input_tokens} in / ${event.usage.output_tokens} out tokens` : '';
        if (event.finish_reason !== 'completed' || usage) chatAppend('meta', `${event.finish_reason}${usage}`);
        setChatBusy(false);
        if (historyPanel.classList.contains('open')) loadHistory();
        refreshProjectMaps().catch((err) => log(`project refresh failed: ${err}`));
        window.emuControls?.refreshProjectContext();
        window.emuControls?.refreshSlots();
        break;
      }
      default:
        break;   // turn_started, thinking_delta and status are not shown
    }
    chatScroll();
  };
  setChatBusy(true);
  const selectedMap = playMap && playMap.value ? playMap.value : null;
  const selectedX = playX && playX.value !== '' ? Number(playX.value) : null;
  const selectedY = playY && playY.value !== '' ? Number(playY.value) : null;
  invoke('chat_send', { text, selectedMap, selectedX, selectedY, onEvent: channel }).catch((err) => {
    setChatBusy(false);
    if (handleAuthRequired(err)) chatAppend('meta', 'Sign in with Google above, then send the message again.');
    else chatAppend('error', `${err}`);
  });
}

document.getElementById('chat-form').addEventListener('submit', (e) => {
  e.preventDefault();
  const text = chatInput.value.trim();
  if (!text || chatBusy || authBusy || window.voiceControls?.active) return;
  chatInput.value = '';
  startTurn(text);
});
chatInput.addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
    e.preventDefault();
    document.getElementById('chat-form').requestSubmit();
  }
});
chatStop.addEventListener('click', () => invoke('chat_cancel').catch((err) => chatAppend('error', `${err}`)));
document.getElementById('chat-new').addEventListener('click', async () => {
  chatGeneration++;
  await invoke('chat_reset');
  chatLog.innerHTML = '';
  setChatBusy(false);
  chatInput.focus();
});

// APP-003: map-specific bounds and separate built-ROM preview/build actions.
const playBar = document.getElementById('play-bar');
const playMap = document.getElementById('play-map');
const playX = document.getElementById('play-x');
const playY = document.getElementById('play-y');
const playBuild = document.getElementById('play-build');
const playPreview = document.getElementById('play-preview');
const playStatus = document.getElementById('play-status');
let mapBounds = new Map(), previewPoints = new Map(), previousMap = '', previewBusy = false;

function validPreviewPoint(map, x, y) {
  const bounds = mapBounds.get(map);
  return bounds && Number.isInteger(x) && Number.isInteger(y) &&
    x >= 0 && y >= 0 && x < bounds.width && y < bounds.height && x <= 32767 && y <= 32767;
}
function currentPreviewPointValid() {
  return playX.value !== '' && playY.value !== '' &&
    validPreviewPoint(playMap.value, Number(playX.value), Number(playY.value));
}
function updatePreviewControls() {
  playBuild.disabled = playPreview.disabled = previewBusy || chatBusy || !currentPreviewPointValid();
}
function applyPreviewBounds(reset) {
  const bounds = mapBounds.get(playMap.value);
  playX.max = bounds ? Math.min(bounds.width - 1, 32767) : 0;
  playY.max = bounds ? Math.min(bounds.height - 1, 32767) : 0;
  const saved = previewPoints.get(playMap.value);
  if (saved && validPreviewPoint(playMap.value, saved.x, saved.y)) {
    playX.value = saved.x;
    playY.value = saved.y;
  } else if (bounds && (reset || !currentPreviewPointValid())) {
    playX.value = bounds.default_x;
    playY.value = bounds.default_y;
  }
  playMap.title = bounds ? `${bounds.width} × ${bounds.height} tiles; choose a walkable point` : '';
  updatePreviewControls();
}
playMap.addEventListener('change', () => {
  if (previousMap && playX.value !== '' && playY.value !== '' &&
      validPreviewPoint(previousMap, Number(playX.value), Number(playY.value))) {
    previewPoints.set(previousMap, { x: Number(playX.value), y: Number(playY.value) });
  }
  previousMap = playMap.value;
  applyPreviewBounds(true);
  playStatus.className = '';
  playStatus.textContent = 'Preview uses the last build and starts fresh at this point. Coordinates are bounded; walkability is not checked.';
});
for (const input of [playX, playY]) input.addEventListener('input', () => {
  previewPoints.delete(playMap.value);
  updatePreviewControls();
});

async function refreshProjectMaps() {
  const info = await invoke('project_info');
  if (!info) return;
  const selected = playMap.value || info.map;
  const firstLoad = !playMap.options.length;
  mapBounds = new Map((info.map_bounds || []).map(bounds => [bounds.id, bounds]));
  playMap.replaceChildren();
  for (const map of info.maps) playMap.add(new Option(map, map, false, map === selected));
  if (firstLoad) {
    playX.value = info.x;
    playY.value = info.y;
  }
  previousMap = playMap.value;
  applyPreviewBounds(false);
  playBar.classList.add('open');
}
refreshProjectMaps().catch((err) => log(`project_info failed: ${err}`));

async function runPreview(build) {
  if (!currentPreviewPointValid()) {
    playStatus.className = 'error';
    playStatus.textContent = 'Choose integer coordinates inside this map.';
    return;
  }
  const map = playMap.value, x = Number(playX.value), y = Number(playY.value);
  previewPoints.set(map, { x, y });
  previewBusy = true;
  playMap.disabled = playX.disabled = playY.disabled = true;
  updatePreviewControls();
  playStatus.className = '';
  playStatus.textContent = `${build ? 'Building and booting' : 'Starting last-built preview of'} ${map} (${x}, ${y})…`;
  try {
    const report = await invoke(build ? 'build_and_play' : 'preview_map', { map, x, y });
    romName = `${map} (${x}, ${y})`;
    playStatus.textContent = `${build ? 'Built' : 'Previewing last build'} ${report.rom_sha256.slice(0, 12)} at ${romName}.`;
    window.emuControls?.refreshProjectContext();
    window.emuControls?.refreshSlots();
  } catch (err) {
    playStatus.className = 'error';
    playStatus.textContent = `${build ? 'Build & play' : 'Preview'} failed: ${err}`;
  } finally {
    previewBusy = false;
    playMap.disabled = playX.disabled = playY.disabled = false;
    updatePreviewControls();
    canvas.focus();
  }
}
playBuild.addEventListener('click', () => runPreview(true));
playPreview.addEventListener('click', () => runPreview(false));

// A provider change starts a fresh conversation. Never mix vendor histories.
let selectedProvider = 'claude';
document.getElementById('chat-provider').addEventListener('change', async (event) => {
  const provider = event.target.value;
  try {
    await invoke('chat_select', { provider });
    selectedProvider = provider;
    chatGeneration++;
    chatLog.innerHTML = '';
    showAuthPanel(provider).then(() => {
      if (provider !== 'antigravity' || authPanel.dataset.state === 'signed_in') loadModels();
      else modelPlaceholder('Sign in to choose');
    });
  } catch (error) {
    event.target.value = selectedProvider;
    chatAppend('error', `${error}`);
  }
});


// AGT-011: the runtime's own model list; a change applies to the next message
// and keeps the conversation. Loading starts the agent session but no turn.
function modelPlaceholder(text) {
  modelsReady = false;
  chatModel.replaceChildren(new Option(text, ''));
  setChatBusy(chatBusy);
}
async function loadModels() {
  const generation = ++modelGeneration;
  modelPlaceholder('Loading…');
  try {
    const models = await invoke('chat_models');
    if (generation !== modelGeneration) return;
    if (!models.length) { modelPlaceholder('Default'); return; }
    chatModel.replaceChildren(...models.map((m) => {
      const option = new Option(m.name, m.id, false, m.current);
      if (m.description) option.title = m.description;
      return option;
    }));
    currentModel = chatModel.value;
    modelsReady = true;
    setChatBusy(chatBusy);
  } catch (error) {
    if (generation !== modelGeneration) return;
    modelPlaceholder(handleAuthRequired(error) || /sign-in|sign in|auth/i.test(`${error}`) ? 'Sign in to choose' : 'Unavailable');
    chatModel.title = `${error}`;
  }
}
chatModel.addEventListener('change', async () => {
  const model = chatModel.value;
  try {
    await invoke('chat_set_model', { model });
    currentModel = model;
    chatAppend('meta', `Model: ${chatModel.selectedOptions[0].text} (from the next message)`);
  } catch (error) {
    chatModel.value = currentModel;
    chatAppend('error', `${error}`);
  }
});

// The runtime owns the browser callback and credential store. Only a status
// enum crosses IPC: OAuth URLs, login IDs and tokens stay out of the chat log.
// One panel serves every provider that signs in from Tauros.
const authPanel = document.getElementById('agent-auth');
const authLogin = document.getElementById('auth-login');
const authLoginCancel = document.getElementById('auth-login-cancel');
const authLoginStatus = document.getElementById('auth-login-status');
const authProviders = {
  codex_chatgpt: {
    title: 'ChatGPT plan', label: 'Sign in with ChatGPT', commands: 'codex_login', opening: 'Opening ChatGPT sign-in…',
    messages: {
      idle: 'Connect with your ChatGPT account. No API key needed.', pending: 'Complete sign-in in your browser, then return here.',
      signed_in: 'Connected with ChatGPT. Model access will be checked on your next message.',
      cancelled: 'Sign-in cancelled. You can try again.',
      failed: 'Sign-in failed. Try again.', expired: 'Sign-in expired. Try again.',
    },
  },
  antigravity: {
    title: 'Google account', label: 'Sign in with Google', commands: 'antigravity_login', opening: 'Opening Google sign-in…',
    messages: {
      idle: 'Checking your Antigravity sign-in…', signed_out: 'Sign in with your Google account to use Antigravity. No API key needed.',
      pending: 'Complete sign-in in your browser, then return here.',
      signed_in: 'Connected with your Google account. Shared with your other Antigravity apps.',
      cancelled: 'Sign-in cancelled. You can try again.',
      failed: 'Sign-in failed. Try again.', expired: 'Sign-in expired. Try again.',
    },
  },
};
let loginGeneration = 0;
const authFor = () => authProviders[selectedProvider];
function showLoginStatus(state) {
  const auth = authFor();
  if (!auth) return;
  if (state === 'signed_in' && authPanel.dataset.state === 'pending') setTimeout(loadModels, 0);
  authBusy = state === 'pending';
  authPanel.dataset.state = state;
  authLogin.textContent = state === 'signed_in' ? 'Check connection' : 'Sign in';
  authLoginStatus.textContent = auth.messages[state] || auth.messages.failed;
  authLoginCancel.hidden = !authBusy;
  authLoginCancel.disabled = false;
  setChatBusy(chatBusy);
}
function showAuthError(error) {
  showLoginStatus('failed');
  authLoginStatus.textContent = `${error}`;
}
// Antigravity checks its cached sign-in silently; nothing opens in the browser.
async function showAuthPanel(provider) {
  const auth = authProviders[provider];
  const generation = ++loginGeneration;
  authPanel.hidden = !auth;
  if (!auth) { authBusy = false; setChatBusy(chatBusy); return; }
  document.getElementById('auth-title').textContent = auth.title;
  authLogin.setAttribute('aria-label', auth.label);
  showLoginStatus('idle');
  if (provider !== 'antigravity') return;
  authLogin.disabled = true;
  try {
    const state = await invoke('antigravity_login_check');
    if (generation === loginGeneration) showLoginStatus(state);
  } catch (error) {
    if (generation === loginGeneration) showAuthError(error);
  }
}
async function pollLogin(generation) {
  if (generation !== loginGeneration || !authBusy) return;
  try {
    const state = await invoke(`${authFor().commands}_status`);
    if (generation !== loginGeneration) return;
    showLoginStatus(state);
    if (state === 'pending') setTimeout(() => pollLogin(generation), 1500);
  } catch (error) {
    if (generation === loginGeneration) showAuthError(error);
  }
}
authLogin.addEventListener('click', async () => {
  const auth = authFor();
  if (!auth) return;
  const generation = ++loginGeneration;
  authBusy = true;
  setChatBusy(chatBusy);
  authPanel.dataset.state = 'pending';
  authLoginStatus.textContent = auth.opening;
  try {
    const state = await invoke(`${auth.commands}_start`);
    if (generation !== loginGeneration) return;
    showLoginStatus(state);
    if (state === 'pending') pollLogin(generation);
  } catch (error) {
    if (generation === loginGeneration) showAuthError(error);
  }
});
authLoginCancel.addEventListener('click', async () => {
  ++loginGeneration;
  authLoginCancel.disabled = true;
  try { showLoginStatus(await invoke(`${authFor().commands}_cancel`)); }
  catch (error) { showAuthError(error); }
});
// A reply refused for a missing Antigravity sign-in reopens the panel instead.
function handleAuthRequired(error) {
  if (selectedProvider !== 'antigravity' || !`${error}`.includes('Antigravity sign-in required')) return false;
  showLoginStatus('signed_out');
  return true;
}

loadModels();
