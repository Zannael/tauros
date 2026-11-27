// Emulator controller: persisted preferences and key bindings, the playback status
// streamed by the backend, savestates, saves, screenshots and core toggles.
import { writable, get } from 'svelte/store';
import { DEFAULT_KEYMAP, buttonBit } from './keys.js';

export const SPEED_ORDER = [1, 2, 4, 8, 0];
export const DEFAULT_PREFS = {
  scale: 'auto', aspect_locked: true, muted: false, volume: 100,
  speed: 1, ff_speed: 4, pause_on_blur: true, rewind_enabled: true,
};
export const IDLE_STATUS = {
  mode: 'idle', loaded: false, paused: false, held: false, speed: 1, fps: 0,
  frame_count: 0, rewind_count: 0, rom_name: '', rom_sha256: null, rom_info: null,
};

export const speedBadge = (speed) => (speed === 0 ? '∞' : `${speed}×`);
export const nextSpeed = (speed) => SPEED_ORDER[(SPEED_ORDER.indexOf(speed) + 1) % SPEED_ORDER.length] ?? 1;

// One short line for the status bar; a held preview never shows a frame rate.
export function modeSummary(status, t) {
  switch (status.mode) {
    case 'idle': return t('No game running');
    case 'agent_held': return t('Agent held · preview paused');
    case 'paused': return t('Paused at frame {n}', { n: status.frame_count });
    case 'fast_forward': return t('Fast {speed}', { speed: speedBadge(status.speed) }) +
      (status.fps > 0 ? ` · ${status.fps.toFixed(0)} fps` : '');
    default: return status.fps > 0 ? `${status.fps.toFixed(1)} fps` : t('Running');
  }
}

const fileName = (path) => String(path).split('/').pop();

export function createEmu({ ipc, notify, t = (s, p) => s }) {
  const config = writable({ keymap: { ...DEFAULT_KEYMAP }, prefs: { ...DEFAULT_PREFS } });
  const status = writable({ ...IDLE_STATUS });
  const slots = writable(Array.from({ length: 9 }, (_, i) => ({ slot: i + 1, available: false })));
  const busy = writable(false);
  const fail = (label) => (err) => notify(`${t(label)}: ${err}`, 'error');

  async function savePrefs(patch) {
    const prefs = { ...get(config).prefs, ...patch };
    try { config.set(await ipc.invoke('emu_set_prefs', { prefs })); } catch (err) { fail('Cannot save the preference')(err); }
  }

  async function refreshSlots() {
    try { slots.set(await ipc.invoke('emu_savestate_slots')); } catch (err) { fail('Cannot read the savestate slots')(err); }
  }

  const controllable = () => { const s = get(status); return s.loaded && s.mode !== 'agent_held' && !get(busy); };

  const api = {
    config, status, slots, busy,
    buttonBit: (event) => buttonBit(get(config).keymap, event),
    isMuted: () => get(config).prefs.muted,
    volume: () => get(config).prefs.volume / 100,
    savePrefs,
    refreshSlots,

    async init() {
      try { config.set(await ipc.invoke('emu_get_config')); } catch { /* defaults */ }
      ipc.invoke('attach_status', { onStatus: ipc.channel((st) => status.set(st)) }).catch(() => {});
      await refreshSlots();
    },

    async togglePause() {
      const s = get(status);
      if (!s.loaded || s.mode === 'agent_held') return;
      await ipc.invoke('set_paused', { paused: !s.paused }).catch(fail('Cannot pause'));
    },
    async step() {
      if (!controllable()) return;
      try { await ipc.invoke('emu_step_frame'); } catch (err) { fail('Frame advance failed')(err); }
    },
    async reset() {
      if (!controllable()) return;
      try { await ipc.invoke('emu_reset'); await refreshSlots(); notify(t('Console reset')); } catch (err) { fail('Reset failed')(err); }
    },
    async rewind() {
      if (!controllable() || get(status).rewind_count === 0) return;
      try { await ipc.invoke('emu_rewind'); } catch (err) { fail('Rewind failed')(err); }
    },
    // The badge button cycles 1×, 2×, 4×, 8×, unlimited; any multiplier other than
    // 1× is also remembered for the fast-forward shortcut.
    async setSpeed(speed) {
      await savePrefs(speed === 1 ? { speed } : { speed, ff_speed: speed });
    },
    async cycleSpeed() { await api.setSpeed(nextSpeed(get(config).prefs.speed)); },
    async toggleFastForward() {
      const { speed, ff_speed } = get(config).prefs;
      await savePrefs({ speed: speed === 1 ? ff_speed : 1 });
    },
    async screenshot() {
      try { const res = await ipc.invoke('emu_screenshot'); notify(t('Screenshot saved: {file}', { file: fileName(res.path) }), 'success'); }
      catch (err) { fail('Screenshot failed')(err); }
    },
    async exportSave() {
      try { const res = await ipc.invoke('emu_export_save'); notify(t('Save exported: {file}', { file: fileName(res.path) }), 'success'); }
      catch (err) { fail('Save export failed')(err); }
    },
    async importSave(file) {
      try { await ipc.invoke('emu_import_save', new Uint8Array(await file.arrayBuffer())); notify(t('Imported {file}', { file: file.name }), 'success'); }
      catch (err) { fail('Save import failed')(err); }
    },
    async saveStateFile() {
      try { const res = await ipc.invoke('emu_save_state_file'); notify(t('State saved: {file}', { file: fileName(res.path) }), 'success'); }
      catch (err) { fail('Cannot save the state')(err); }
    },
    async loadStateFile(file) {
      try { await ipc.invoke('emu_load_state_file', new Uint8Array(await file.arrayBuffer())); notify(t('Loaded state {file}', { file: file.name }), 'success'); }
      catch (err) { fail('Cannot load the state')(err); }
    },
    async saveSlot(slot) {
      try { await ipc.invoke('emu_save_slot', { slot }); await refreshSlots(); notify(t('Saved to slot {n}', { n: slot }), 'success'); }
      catch (err) { fail('Quick save failed')(err); }
    },
    async loadSlot(slot) {
      try { await ipc.invoke('emu_load_slot', { slot }); notify(t('Loaded slot {n}', { n: slot })); }
      catch (err) { fail('Quick load failed')(err); }
    },
    async loadRom(file) {
      try {
        await ipc.invoke('load_rom', new Uint8Array(await file.arrayBuffer()));
        await ipc.invoke('emu_set_rom_name', { name: file.name }).catch(() => {});
        await refreshSlots();
      } catch (err) { fail(`Cannot load ${file.name}`)(err); }
    },
    async setKeymap(keymap) {
      config.set(await ipc.invoke('emu_set_keymap', { keymap }));
    },
    logs: () => ipc.invoke('emu_logs'),
    layers: () => ipc.invoke('emu_layers'),
    setLayer: (kind, id, enabled) =>
      ipc.invoke(kind === 'video' ? 'emu_set_video_layer' : 'emu_set_audio_channel', { id, enabled }),
  };
  return api;
}
