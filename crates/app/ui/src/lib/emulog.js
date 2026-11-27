// mGBA core log: the backend hands over new entries on each read, so the view
// keeps them for the session (bounded).
import { writable } from 'svelte/store';

const store = writable([]);
export const emuLog = {
  subscribe: store.subscribe,
  async refresh(emu) {
    try { const entries = await emu.logs(); if (entries.length) store.update((list) => [...list, ...entries].slice(-2000)); }
    catch (err) { store.update((list) => [...list, { level: 'ERROR', message: `Log read failed: ${err}` }]); }
  },
  clear() { store.set([]); },
};
