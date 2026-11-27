// Application state shared by the shell and the views: one instance of each
// controller, the active view, panel visibility and the actions views declare.
import { writable, get, derived } from 'svelte/store';
import { tauriIpc, log } from './ipc.js';
import { uiPrefs, setPref } from './prefs.js';
import { t } from './i18n.js';
import { notify } from './notify.js';
import { createEmu } from './emu.js';
import { createChat } from './chat.js';
import { createVoice } from './voice.js';
import { createProject } from './project.js';
import { createHistory } from './history.js';
import { createMapgen } from './mapgen.js';
import { createGame } from './game.js';
import { createProjects, pointsStore } from './projects.js';

// Views in navigation order. Planned ones are shown disabled with their task.
export const VIEWS = [
  { id: 'project', label: 'Project', icon: 'home' },
  { id: 'emulator', label: 'Emulator', icon: 'gamepad' },
  { id: 'maps', label: 'Maps', icon: 'map', wip: 'APP-001' },
  { id: 'trainers', label: 'Trainers', icon: 'user', wip: 'APP-002' },
  { id: 'scripts', label: 'Scripts', icon: 'code', wip: 'APP-002' },
  { id: 'graphics', label: 'Graphics', icon: 'image', wip: 'GFX-002' },
  { id: 'mapgen', label: 'Map generator', icon: 'sparkles' },
  { id: 'history', label: 'History', icon: 'history' },
  { id: 'settings', label: 'Settings', icon: 'settings' },
];

const tr = (key, params) => get(t)(key, params);
const ipc = tauriIpc();

export const view = writable('project');
export const paletteOpen = writable(false);
export const miniGame = writable(false);       // compact game player outside the Emulator view
export const viewActions = writable([]);       // header actions of the active view

export const emu = createEmu({ ipc, notify, t: tr });
let voice;
export const chat = createChat({
  ipc, t: tr,
  context: () => { const p = get(project.point); return p.map ? { map: p.map, x: Number(p.x), y: Number(p.y) } : {}; },
  voiceActive: () => voice?.active,
  onTurnEnd: () => {
    project.refresh();
    emu.refreshSlots();
    if (get(view) === 'history') history.load();
  },
});
voice = createVoice({
  ipc, draft: chat.draft,
  settings: () => ({ engine: get(uiPrefs).voiceEngine, language: get(uiPrefs).voiceLanguage }),
  blocked: () => get(chat.busy) || get(chat.auth).busy,
});
export { voice };
const points = pointsStore(uiPrefs, setPref);
export const project = createProject({ ipc, t: tr, notify, isAgentBusy: () => get(chat.busy),
  lastPoint: points.last, rememberPoint: points.remember });
export const projects = createProjects({ ipc, prefs: uiPrefs, setPref, notify, t: tr });
export const history = createHistory({ ipc, t: tr, notify, isAgentBusy: () => get(chat.busy) });
export const mapgen = createMapgen({ ipc });
export const game = createGame({ ipc, emu, voiceActive: () => voice.active });
export { ipc };

// The emulator follows the agent's busy state (slot and transport guards).
chat.busy.subscribe((busy) => emu.busy.set(busy));

export const agentOpen = derived(uiPrefs, ($p) => $p.agentOpen);
export const toggleAgent = () => setPref('agentOpen', !get(uiPrefs).agentOpen);

export function navigate(id) {
  const target = VIEWS.find((v) => v.id === id);
  if (!target || target.wip) return;
  view.set(id);
}

// The game pauses while nobody can see it, and resumes only if it paused it.
let pausedByVisibility = false;
export function syncGameVisibility() {
  const visible = get(view) === 'emulator' || get(miniGame);
  const status = get(emu.status);
  if (!visible && status.loaded && !status.paused && status.mode !== 'agent_held') {
    pausedByVisibility = true;
    ipc.invoke('set_paused', { paused: true }).catch(() => {});
  } else if (visible && pausedByVisibility) {
    pausedByVisibility = false;
    ipc.invoke('set_paused', { paused: false }).catch(() => {});
  }
}
view.subscribe(syncGameVisibility);
miniGame.subscribe(syncGameVisibility);

// Text fields take the keyboard: the backend stops reading game keys while one
// has focus (or while dictation runs).
function trackTyping() {
  const field = (el) => !!el?.matches?.('textarea, select, input:not([type=checkbox]):not([type=radio]):not([type=range]):not([type=file])');
  document.addEventListener('focusin', (e) => { if (field(e.target)) ipc.invoke('set_typing', { typing: true }).catch(() => {}); });
  document.addEventListener('focusout', (e) => {
    if (field(e.target) && !field(e.relatedTarget)) ipc.invoke('set_typing', { typing: !!voice.active }).catch(() => {});
  });
}

export async function start() {
  window.addEventListener('error', (e) => log(ipc, `script error: ${e.message} at ${e.filename}:${e.lineno}`));
  window.addEventListener('unhandledrejection', (e) => log(ipc, `unhandled: ${e.reason}`));
  trackTyping();
  document.addEventListener('visibilitychange', () => {
    if (get(emu.config).prefs.pause_on_blur) ipc.invoke('set_paused', { paused: document.hidden }).catch(() => {});
  });
  const attached = game.attach().catch((err) => { notify(`${err}`, 'error'); return null; });
  await emu.init();
  await projects.init();
  projects.refresh();
  await project.refresh();
  // A ROM opened from the command line goes straight to the game; otherwise the
  // Project view shows the project or the projects to open.
  if (!get(project.info) && !get(chat.project) && (await Promise.race([attached, new Promise((r) => setTimeout(() => r(null), 300))]))) view.set('emulator');
  else view.set('project');
  chat.init();
  voice.status();
}
