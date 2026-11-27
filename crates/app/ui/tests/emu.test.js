// Emulator controller against mocked IPC (ported from tests/emu_ui.cjs): saved
// bindings and preferences, status summaries that never show a held preview's
// frame rate, guarded transport actions, and the fast-forward multiplier.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { get } from 'svelte/store';
import { createEmu, modeSummary, nextSpeed, speedBadge } from '../src/lib/emu.js';
import { translate } from '../src/lib/i18n.js';

const t = (k, p) => translate('en', k, p);

function setup() {
  const calls = [];
  let statusHandler = null;
  const saved = {
    keymap: { a: 'j', b: 'z', select: 'backspace', start: 'enter', right: 'arrowright', left: 'arrowleft', up: 'arrowup', down: 'arrowdown', r: 's', l: 'a' },
    prefs: { scale: '2x', aspect_locked: true, muted: false, volume: 80, speed: 1, ff_speed: 8, pause_on_blur: true, rewind_enabled: true },
  };
  const notes = [];
  const ipc = {
    channel(fn) { statusHandler = fn; return {}; },
    invoke: async (cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === 'emu_get_config') return structuredClone(saved);
      if (cmd === 'emu_set_prefs') { saved.prefs = args.prefs; return structuredClone(saved); }
      if (cmd === 'emu_savestate_slots') return Array.from({ length: 9 }, (_, i) => ({ slot: i + 1, available: i === 0 }));
      if (cmd === 'emu_step_frame') return 101;
      if (cmd === 'emu_rewind') return 85;
      if (cmd === 'emu_screenshot') return { path: '/tmp/shots/shot.png' };
      return null;
    },
  };
  const emu = createEmu({ ipc, notify: (text, kind) => notes.push([text, kind]), t });
  return { emu, calls, notes, saved, status: (st) => statusHandler(st) };
}

const STATUS = { loaded: true, held: false, speed: 1, fps: 0, frame_count: 300, rewind_count: 5, rom_name: 'MAP_PETALBURG_CITY (15, 10)' };

test('loads saved bindings and preferences', async () => {
  const { emu } = setup();
  await emu.init();
  assert.equal(emu.volume(), 0.8);
  assert.equal(emu.buttonBit({ code: 'KeyJ', key: 'j' }), 0, 'remapped A key');
  assert.equal(emu.buttonBit({ code: 'KeyX', key: 'x' }), undefined, 'old A key no longer fires');
  assert.equal(get(emu.slots)[0].available, true);
});

test('a held preview never reports a frame rate and blocks transport', async () => {
  const { emu, calls, status } = setup();
  await emu.init();
  status({ ...STATUS, mode: 'agent_held', paused: false, fps: 10 });
  assert.match(modeSummary(get(emu.status), t), /Agent held/);
  assert.doesNotMatch(modeSummary(get(emu.status), t), /fps/);
  await emu.step(); await emu.rewind(); await emu.togglePause();
  assert(!calls.some(([c]) => ['emu_step_frame', 'emu_rewind', 'set_paused'].includes(c)));
});

test('paused status allows step, rewind and screenshot', async () => {
  const { emu, calls, notes, status } = setup();
  await emu.init();
  status({ ...STATUS, mode: 'paused', paused: true });
  assert.equal(modeSummary(get(emu.status), t), 'Paused at frame 300');
  await emu.step(); await emu.rewind(); await emu.screenshot();
  for (const c of ['emu_step_frame', 'emu_rewind', 'emu_screenshot']) assert(calls.some(([n]) => n === c), c);
  assert.match(notes.at(-1)[0], /shot\.png/);
});

test('a busy agent blocks transport actions', async () => {
  const { emu, calls, status } = setup();
  await emu.init();
  status({ ...STATUS, mode: 'paused', paused: true });
  emu.busy.set(true);
  await emu.step(); await emu.reset();
  assert(!calls.some(([c]) => c === 'emu_step_frame' || c === 'emu_reset'));
});

test('speed cycles and fast-forward remembers the multiplier', async () => {
  const { emu, saved } = setup();
  await emu.init();
  assert.equal(nextSpeed(1), 2); assert.equal(nextSpeed(8), 0); assert.equal(nextSpeed(0), 1);
  assert.equal(speedBadge(0), '∞'); assert.equal(speedBadge(4), '4×');
  await emu.toggleFastForward();
  assert.equal(saved.prefs.speed, 8, 'fast-forward uses the remembered multiplier');
  await emu.toggleFastForward();
  assert.equal(saved.prefs.speed, 1);
  await emu.setSpeed(2);
  assert.deepEqual([saved.prefs.speed, saved.prefs.ff_speed], [2, 2]);
  await emu.setSpeed(1);
  await emu.toggleFastForward();
  assert.equal(saved.prefs.speed, 2);
});
