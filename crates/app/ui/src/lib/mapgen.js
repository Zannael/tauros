// GFX-009 map generator: drives scripts/mapgen.py through the backend and keeps
// each run's steps (layout, inventory, tileset batches) for review.
import { writable, get } from 'svelte/store';

export const STEP_STATUS = { pending: 'To do', planning: 'Planning…', generating: 'Generating…', checking: 'Checking…',
  running: 'Running…', review: 'To review', accepted: 'Accepted', failed: 'Failed' };

export function createMapgen({ ipc }) {
  const state = writable({ run: null, runs: [], keyReady: true, busy: false, activity: '', error: '', log: '' });
  const shown = writable({});   // `${run}:${step}` → version index being reviewed
  const images = new Map();
  const set = (change) => state.update((s) => ({ ...s, ...change }));
  const log = (text) => state.update((s) => ({ ...s, log: `${new Date().toLocaleTimeString()} ${text}\n${s.log}`.slice(0, 20000) }));
  const read = (args) => ipc.invoke('mapgen_run', { args, onEvent: ipc.channel(() => {}) });

  const api = {
    state, shown,
    path: (file) => `${get(state).run.dir}/${file}`,

    async refresh(select) {
      try {
        const result = await read(['list']);
        const want = select || get(state).run?.id || result.runs[0]?.id;
        const run = want ? (await read(['state', want])).state : null;
        set({ runs: result.runs, keyReady: result.key_ready, run });
      } catch (error) { set({ error: String(error) }); }
    },

    call(args, label) {
      set({ error: '', busy: true, activity: label });
      const channel = ipc.channel((event) => {
        if (event.event === 'progress') { set({ activity: event.message }); log(event.message); }
        if (event.event === 'state' && (!get(state).run || event.state.id === get(state).run.id)) set({ run: event.state });
      });
      return ipc.invoke('mapgen_run', { args, onEvent: channel })
        .catch((error) => { set({ error: String(error) }); log(`Error: ${error}`); throw error; })
        .finally(() => { set({ busy: false, activity: '' }); api.refresh(); });
    },

    stop() { set({ activity: 'Stopping…' }); ipc.invoke('mapgen_cancel').catch(() => {}); },
    setError(error) { set({ error }); },

    image(path) {
      if (!images.has(path)) {
        images.set(path, ipc.invoke('mapgen_image', { path }).then((bytes) => new Promise((resolve, reject) => {
          const reader = new FileReader();
          reader.onload = () => resolve(reader.result);
          reader.onerror = () => reject(reader.error);
          reader.readAsDataURL(new Blob([bytes instanceof ArrayBuffer ? bytes : new Uint8Array(bytes)], { type: 'image/png' }));
        })).catch((error) => { images.delete(path); log(`Image not shown: ${error}`); return ''; }));
      }
      return images.get(path);
    },

    shownIndex(step, key) {
      const run = get(state).run;
      return get(shown)[`${run.id}:${key}`] ?? step.accepted ?? step.versions.length - 1;
    },
    show(key, index) { shown.update((s) => ({ ...s, [`${get(state).run.id}:${key}`]: index })); },
    forget(key) { shown.update((s) => { const next = { ...s }; delete next[`${get(state).run.id}:${key}`]; return next; }); },
  };
  return api;
}
