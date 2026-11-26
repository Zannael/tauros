// Execute the actual emulator UI (APP-003 Block 1) against mocked Tauri IPC.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');

const code = fs.readFileSync('crates/app/ui/emu_ui.js', 'utf8');

function makeElement(tag = 'div', attrs = {}) {
  const listeners = {};
  const el = {
    tagName: tag.toUpperCase(),
    id: attrs.id || '',
    value: attrs.value || '',
    disabled: false,
    hidden: false,
    textContent: '',
    style: {},
    dataset: { ...(attrs.dataset || {}) },
    attrs: {},
    children: [],
    classList: {
      set: new Set(),
      add(c) { this.set.add(c); },
      remove(c) { this.set.delete(c); },
      toggle(c, force) {
        if (force === undefined) {
          if (this.set.has(c)) this.set.delete(c); else this.set.add(c);
        } else if (force) this.set.add(c); else this.set.delete(c);
      },
      contains(c) { return this.set.has(c); },
    },
    setAttribute(k, v) { this.attrs[k] = String(v); },
    getAttribute(k) { return this.attrs[k]; },
    addEventListener(name, fn) { (listeners[name] ||= []).push(fn); },
    dispatch(name, event = {}) {
      for (const fn of listeners[name] || []) fn({ target: el, preventDefault(){}, stopPropagation(){}, ...event });
    },
    click() { this.dispatch('click'); },
    focus() {},
    blur() {},
    append(...nodes) { this.children.push(...nodes); },
    appendChild(node) { this.children.push(node); return node; },
    querySelector() { return null; },
    querySelectorAll() { return []; },
    closest() { return null; },
    contains() { return false; },
    get firstChild() { return this.children[0] || null; },
  };
  return el;
}

const ids = [
  'emu-menubar', 'screen', 'screen-stage', 'screen-badge', 'status',
  'emu-mode-pill', 'emu-frame-readout', 'emu-audio-readout', 'emu-rewind-readout',
  'emu-toast', 'help', 'tb-pause', 'tb-step', 'tb-rewind', 'tb-speed', 'tb-reset',
  'tb-shot', 'tb-mute', 'menu-volume', 'menu-volume-label', 'sav-file', 'state-file',
  'emu-rom-modal', 'emu-keys-modal', 'emu-log-modal', 'play-banner', 'play-banner-text',
  'play-banner-boot', 'play-live-chip', 'play-map', 'play-x', 'play-y', 'play-preview',
  'menu-aspect', 'menu-mute', 'menu-ff-toggle', 'menu-blur-pause', 'menu-rewind-toggle',
  'menu-pause', 'menu-step', 'menu-reset', 'menu-rewind', 'menu-shot', 'menu-rom-info',
  'menu-export-sav', 'menu-import-sav', 'menu-save-state-file', 'menu-load-state-file',
  'menu-slots-save', 'menu-slots-load', 'rom-info-body', 'rom-info-close',
  'keys-grid', 'keys-error', 'keys-reset', 'keys-cancel', 'keys-save',
  'emu-log-list', 'emu-log-refresh', 'emu-log-clear', 'emu-log-close', 'file',
];

async function run() {
  const elements = Object.fromEntries(ids.map((id) => [id, makeElement('div', { id })]));
  elements['play-map'].value = 'MAP_PETALBURG_CITY';
  elements['play-x'].value = '15';
  elements['play-y'].value = '10';
  for (const modal of ['emu-rom-modal', 'emu-keys-modal', 'emu-log-modal', 'play-banner', 'screen-badge', 'play-live-chip']) {
    elements[modal].hidden = true;
  }

  let statusHandler = null;
  const calls = [];
  let savedConfig = {
    keymap: { a: 'j', b: 'z', select: 'backspace', start: 'enter', right: 'arrowright', left: 'arrowleft', up: 'arrowup', down: 'arrowdown', r: 's', l: 'a' },
    prefs: { scale: '2x', aspect_locked: true, muted: false, volume: 80, speed: 1, pause_on_blur: true, rewind_enabled: true },
  };

  class MockChannel {
    set onmessage(fn) { this._fn = fn; statusHandler = fn; }
    get onmessage() { return this._fn; }
  }

  const docListeners = {};
  const winListeners = {};
  const context = vm.createContext({
    document: {
      readyState: 'complete',
      getElementById: (id) => elements[id] || makeElement('div', { id }),
      createElement: (tag) => makeElement(tag),
      addEventListener(name, fn) { (docListeners[name] ||= []).push(fn); },
    },
    window: {
      __TAURI__: {
        core: {
          Channel: MockChannel,
          invoke: async (cmd, args) => {
            calls.push([cmd, args]);
            if (cmd === 'emu_get_config') return JSON.parse(JSON.stringify(savedConfig));
            if (cmd === 'emu_set_prefs') { savedConfig.prefs = args.prefs; return JSON.parse(JSON.stringify(savedConfig)); }
            if (cmd === 'emu_set_keymap') { savedConfig.keymap = args.keymap; return JSON.parse(JSON.stringify(savedConfig)); }
            if (cmd === 'emu_savestate_slots') return Array.from({ length: 9 }, (_, i) => ({ slot: i + 1, available: i === 0 }));
            if (cmd === 'project_context') {
              return {
                selected_map: 'MAP_PETALBURG_CITY', selected_x: 15, selected_y: 10,
                live_status: 'ok', live_map: 'MAP_PETALBURG_CITY', live_x: 15, live_y: 10,
                loaded_rom_sha256: 'abcdef123456', rom_stale: false, new_build_pending: true,
              };
            }
            if (cmd === 'emu_step_frame') return 101;
            if (cmd === 'emu_rewind') return 85;
            if (cmd === 'emu_screenshot') return { path: '/tmp/shot.png', bytes: 1200, frame_count: 101 };
            if (cmd === 'emu_logs') return [{ level: 'WARN', category: 1, message: 'test log line' }];
            return null;
          },
        },
      },
      addEventListener(name, fn) { (winListeners[name] ||= []).push(fn); },
    },
    setTimeout,
    clearTimeout,
  });

  vm.runInContext(code, context);
  await new Promise((r) => setTimeout(r, 20));

  const emu = context.window.emuControls;
  assert.ok(emu, 'window.emuControls should be registered');
  assert.equal(elements['screen'].style.width, '480px');
  assert.equal(emu.getVolume(), 0.8);
  assert.equal(emu.buttonBit({ code: 'KeyJ', key: 'j' }), 0, 'remapped A key works');
  assert.equal(emu.buttonBit({ code: 'KeyX', key: 'x' }), undefined, 'old A key no longer fires');
  assert.equal(elements['play-live-chip'].textContent, 'Live: MAP_PETALBURG_CITY (15, 10)');
  assert.equal(elements['play-banner'].hidden, false, 'post-build banner appears when new_build_pending is true');

  // Agent held status never displays 10.0 fps
  statusHandler({
    mode: 'agent_held', loaded: true, paused: false, held: true,
    speed: 1, fps: 0, frame_count: 300, rewind_count: 5, rom_name: 'MAP_PETALBURG_CITY (15, 10)',
  });
  assert.equal(elements['emu-mode-pill'].textContent, 'Agent held');
  assert.match(elements['status'].textContent, /Agent held/);
  assert.doesNotMatch(elements['status'].textContent, /fps/);
  assert.equal(elements['screen-badge'].hidden, false);
  assert.equal(elements['tb-pause'].disabled, true);
  assert.equal(elements['tb-step'].disabled, true);

  // Paused status unlocks step & rewind
  statusHandler({
    mode: 'paused', loaded: true, paused: true, held: false,
    speed: 1, fps: 0, frame_count: 300, rewind_count: 5, rom_name: 'MAP_PETALBURG_CITY (15, 10)',
  });
  assert.equal(elements['tb-pause'].textContent, '▶ Resume');
  assert.equal(elements['tb-step'].disabled, false);
  assert.equal(elements['tb-rewind'].disabled, false);

  elements['tb-step'].click();
  elements['tb-rewind'].click();
  elements['tb-shot'].click();
  await new Promise((r) => setTimeout(r, 20));
  assert.ok(calls.some(([c]) => c === 'emu_step_frame'));
  assert.ok(calls.some(([c]) => c === 'emu_rewind'));
  assert.ok(calls.some(([c]) => c === 'emu_screenshot'));

  console.log('emu_ui.cjs: all scenarios passed');
}

run().catch((err) => {
  console.error(err);
  process.exit(1);
});
