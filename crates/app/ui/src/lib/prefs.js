// Interface preferences kept in this browser profile (theme, language, dictation
// defaults). Reads and writes never throw: the app works without storage.
import { writable } from 'svelte/store';

const KEY = 'tauros.ui.v1';
// projectsDir '' means the default folder; lastPoints maps a project path to its
// last boot point { map, x, y }.
const DEFAULTS = { theme: 'system', language: 'en', voiceEngine: 'offline', voiceLanguage: 'it', agentOpen: true,
  projectsDir: '', lastPoints: {} };

function load() {
  try { return { ...DEFAULTS, ...JSON.parse(localStorage.getItem(KEY) || '{}') }; } catch { return { ...DEFAULTS }; }
}

export const uiPrefs = writable(load());
uiPrefs.subscribe((value) => {
  try { localStorage.setItem(KEY, JSON.stringify(value)); } catch { /* storage unavailable */ }
});

export function setPref(key, value) {
  uiPrefs.update((prefs) => ({ ...prefs, [key]: value }));
}
