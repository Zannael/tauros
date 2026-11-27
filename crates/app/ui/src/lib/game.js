// Frames streamed by the emulator thread: video goes to every visible screen,
// audio plays through Web Audio, and held keys go back as a button mask.
import { isTextField } from './keys.js';
import { log } from './ipc.js';

const WIDTH = 240, HEIGHT = 160, VIDEO_BYTES = WIDTH * HEIGHT * 4, AUDIO_RATE = 32768;

export function createGame({ ipc, emu, voiceActive = () => false }) {
  const screens = new Set();
  let last = null, audio = null, gain = null, nextTime = 0, mask = 0, seen = 0;

  function ensureAudio() {
    if (!audio) {
      audio = new AudioContext();
      gain = audio.createGain();
      gain.connect(audio.destination);
    }
    if (audio.state === 'suspended') audio.resume();
  }

  function playAudio(samples) {
    if (voiceActive()) return;
    const n = samples.length / 2;
    if (!audio || audio.state !== 'running' || n === 0) return;
    gain.gain.value = emu.isMuted() ? 0 : emu.volume();
    const now = audio.currentTime;
    if (nextTime < now + 0.03) nextTime = now + 0.06;   // underrun: restart with headroom
    if (nextTime > now + 0.25) return;                   // too far ahead: drop to catch up
    const buffer = audio.createBuffer(2, n, AUDIO_RATE);
    const left = buffer.getChannelData(0), right = buffer.getChannelData(1);
    for (let i = 0; i < n; i++) { left[i] = samples[2 * i] / 32768; right[i] = samples[2 * i + 1] / 32768; }
    const source = audio.createBufferSource();
    source.buffer = buffer;
    source.connect(gain);
    source.start(nextTime);
    nextTime += n / AUDIO_RATE;
  }

  function draw(ctx) {
    if (!last) return;
    const image = ctx.createImageData(WIDTH, HEIGHT);
    image.data.set(last);
    ctx.putImageData(image, 0, 0);
  }

  function onFrame(data) {
    const bytes = new Uint8Array(data);
    last = bytes.subarray(0, VIDEO_BYTES);
    for (const ctx of screens) draw(ctx);
    const audioBytes = bytes.subarray(VIDEO_BYTES);
    playAudio(new Int16Array(audioBytes.buffer.slice(audioBytes.byteOffset, audioBytes.byteOffset + audioBytes.byteLength)));
    if (seen++ === 0) log(ipc, `first frame: ${bytes.length} bytes`);
  }

  // On Linux the app reads the keyboard on the GTK window (main.rs) and these
  // handlers never see the mapped keys; elsewhere they are the input path.
  function updateKey(event, down) {
    if (isTextField(event.target) || event.ctrlKey || event.metaKey || event.altKey) return;
    const bit = emu.buttonBit(event);
    if (bit === undefined) return;
    event.preventDefault();
    const next = down ? mask | (1 << bit) : mask & ~(1 << bit);
    if (next !== mask) { mask = next; ipc.invoke('set_buttons', { mask }).catch((err) => log(ipc, `set_buttons failed: ${err}`)); }
  }

  return {
    WIDTH, HEIGHT,
    ensureAudio,
    attach() {
      window.addEventListener('keydown', (e) => { ensureAudio(); updateKey(e, true); });
      window.addEventListener('keyup', (e) => updateKey(e, false));
      window.addEventListener('blur', () => { if (mask) { mask = 0; ipc.invoke('set_buttons', { mask }); } });
      return ipc.invoke('attach', { onFrame: ipc.channel(onFrame) });
    },
    addScreen(ctx) { screens.add(ctx); draw(ctx); return () => screens.delete(ctx); },
  };
}
