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

let audio = null, nextTime = 0, frames = 0, lastFps = performance.now(), romName = '', seen = 0;

function ensureAudio() {
  if (!audio) {
    audio = new AudioContext();
    audio.onstatechange = () => log(`audio ${audio.state} at ${audio.sampleRate} Hz`);
  }
  if (audio.state === 'suspended') audio.resume();
}

function playAudio(samples) {
  const n = samples.length / 2;
  if (!audio || audio.state !== 'running' || n === 0) return;
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
  source.connect(audio.destination);
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
    status.textContent = `${romName} · ${fps.toFixed(1)} fps` +
      (audio && audio.state === 'running' ? '' : ' · sound off (click the game)');
    frames = 0;
    lastFps = now;
  }
}

let mask = 0, rawLogs = 0;
const isTextField = (el) => el && (el.tagName === 'TEXTAREA' || el.tagName === 'INPUT');
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
document.addEventListener('visibilitychange', () => invoke('set_paused', { paused: document.hidden }));

document.getElementById('open').addEventListener('click', (e) => { ensureAudio(); document.getElementById('file').click(); e.target.blur(); });
document.getElementById('file').addEventListener('change', async (e) => {
  const file = e.target.files[0];
  if (!file) return;
  try {
    await invoke('load_rom', new Uint8Array(await file.arrayBuffer()));
    romName = file.name;
    status.textContent = romName;
  } catch (err) {
    status.textContent = `Cannot load ${file.name}: ${err}`;
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
      btn.textContent = 'Restore';
      btn.addEventListener('click', async () => {
        try {
          await invoke('history_undo', { targetCommit: entry.commit });
          status.textContent = `Restored snapshot ${entry.short_commit}`;
          await loadHistory();
        } catch (err) {
          status.textContent = `Undo failed: ${err}`;
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
let chatBusy = false, chatGeneration = 0;   // a new chat ignores the old turn's late events

invoke('chat_info').then((project) => {
  document.getElementById('chat-scope').textContent = project
    ? `Tauros tools on ${project}`
    : 'No project open: start with --project to give the agent the Tauros tools';
});

// Text fields take the keyboard: the app stops reading game keys while one has focus.
for (const field of [chatInput]) {
  field.addEventListener('focus', () => invoke('set_typing', { typing: true }));
  field.addEventListener('blur', () => invoke('set_typing', { typing: false }));
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
  chatSend.disabled = busy;
  chatStop.hidden = !busy;
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
        chatAppend('error', event.message);
        break;
      case 'turn_completed': {
        if (agent) agent.classList.remove('streaming');
        const usage = event.usage ? ` · ${event.usage.input_tokens} in / ${event.usage.output_tokens} out tokens` : '';
        if (event.finish_reason !== 'completed' || usage) chatAppend('meta', `${event.finish_reason}${usage}`);
        setChatBusy(false);
        break;
      }
      default:
        break;   // turn_started, thinking_delta and status are not shown
    }
    chatScroll();
  };
  setChatBusy(true);
  invoke('chat_send', { text, onEvent: channel }).catch((err) => {
    chatAppend('error', `${err}`);
    setChatBusy(false);
  });
}

document.getElementById('chat-form').addEventListener('submit', (e) => {
  e.preventDefault();
  const text = chatInput.value.trim();
  if (!text || chatBusy) return;
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
