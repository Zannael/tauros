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
  if (now - lastFps >= 1000 && !status.dataset.focus) {
    const fps = frames * 1000 / (now - lastFps);
    if (seen < 400) log(`${fps.toFixed(1)} fps`);
    status.textContent = `${romName} · ${fps.toFixed(1)} fps` +
      (audio && audio.state === 'running' ? '' : ' · sound off (click or press a key)');
    frames = 0;
    lastFps = now;
  }
}

let mask = 0, rawLogs = 0;
function updateKey(event, down) {
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
window.addEventListener('blur', () => {
  if (mask) { mask = 0; invoke('set_buttons', { mask }); }
  status.dataset.focus = 'lost';
  status.textContent = 'Keyboard focus lost: click the game to play.';
});
window.addEventListener('focus', () => { delete status.dataset.focus; });
// Keep keyboard focus on the page itself, not on the Open ROM button.
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
