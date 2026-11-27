// Agent chat controller: provider and model choice, sign-in for providers that
// sign in from Tauros, and streamed turns (text, tool calls, errors, completion).
import { writable, get } from 'svelte/store';

export const PROVIDERS = [
  { id: 'claude', name: 'Claude' },
  { id: 'codex_chatgpt', name: 'Codex' },
  { id: 'antigravity', name: 'Antigravity' },
];

// Only a status enum crosses IPC: OAuth URLs, login IDs and tokens never reach the UI.
export const AUTH = {
  codex_chatgpt: {
    title: 'ChatGPT plan', label: 'Sign in with ChatGPT', commands: 'codex_login', opening: 'Opening ChatGPT sign-in…',
    messages: {
      idle: 'Connect with your ChatGPT account. No API key needed.',
      pending: 'Complete sign-in in your browser, then return here.',
      signed_in: 'Connected with ChatGPT. Model access will be checked on your next message.',
      cancelled: 'Sign-in cancelled. You can try again.', failed: 'Sign-in failed. Try again.', expired: 'Sign-in expired. Try again.',
    },
  },
  antigravity: {
    title: 'Google account', label: 'Sign in with Google', commands: 'antigravity_login', opening: 'Opening Google sign-in…',
    messages: {
      idle: 'Checking your Antigravity sign-in…',
      signed_out: 'Sign in with your Google account to use Antigravity. No API key needed.',
      pending: 'Complete sign-in in your browser, then return here.',
      signed_in: 'Connected with your Google account. Shared with your other Antigravity apps.',
      cancelled: 'Sign-in cancelled. You can try again.', failed: 'Sign-in failed. Try again.', expired: 'Sign-in expired. Try again.',
    },
  },
};

export const toolName = (name) => (name.startsWith('mcp__') ? name.split('__').slice(2).join('__') || name : name);

// Tool results are usually MCP content blocks; show their text, else the JSON.
export function formatOutput(output) {
  if (Array.isArray(output) && output.every((b) => b && b.type === 'text')) return output.map((b) => b.text).join('\n');
  if (typeof output === 'string') return output;
  return JSON.stringify(output, null, 2);
}

export function createChat({ ipc, t = (s) => s, context = () => ({}), onTurnEnd = () => {}, voiceActive = () => false }) {
  const project = writable(null);
  const provider = writable('claude');
  const models = writable({ list: [], ready: false, placeholder: 'Loading…', current: '' });
  const entries = writable([]);
  const busy = writable(false);
  const auth = writable({ state: 'idle', message: '', error: '', busy: false });
  const draft = writable('');
  let generation = 0, modelGeneration = 0, loginGeneration = 0, nextId = 1;

  const append = (entry) => { const item = { id: nextId++, ...entry }; entries.update((list) => [...list, item]); return item.id; };
  const patch = (id, change) => entries.update((list) => list.map((e) => (e.id === id ? { ...e, ...change(e) } : e)));

  function setModelsPlaceholder(text) { models.set({ list: [], ready: false, placeholder: text, current: '' }); }

  function authRequired(error) {
    if (get(provider) !== 'antigravity' || !`${error}`.includes('Antigravity sign-in required')) return false;
    showLogin('signed_out');
    return true;
  }

  async function loadModels() {
    const gen = ++modelGeneration;
    setModelsPlaceholder('Loading…');
    try {
      const list = await ipc.invoke('chat_models');
      if (gen !== modelGeneration) return;
      if (!list.length) { setModelsPlaceholder('Default'); return; }
      const current = (list.find((m) => m.current) || list[0]).id;
      models.set({ list, ready: true, placeholder: '', current });
    } catch (error) {
      if (gen !== modelGeneration) return;
      setModelsPlaceholder(authRequired(error) || /sign-in|sign in|auth/i.test(`${error}`) ? 'Sign in to choose' : 'Unavailable');
    }
  }

  function showLogin(state, error = '') {
    const spec = AUTH[get(provider)];
    if (!spec) return;
    if (state === 'signed_in' && get(auth).state === 'pending') setTimeout(loadModels, 0);
    auth.set({ state, message: error || spec.messages[state] || spec.messages.failed, error, busy: state === 'pending' });
  }

  async function checkAuth(name) {
    const gen = ++loginGeneration;
    if (!AUTH[name]) { auth.set({ state: 'idle', message: '', error: '', busy: false }); return; }
    showLogin('idle');
    if (name !== 'antigravity') return;
    try { const state = await ipc.invoke('antigravity_login_check'); if (gen === loginGeneration) showLogin(state); }
    catch (error) { if (gen === loginGeneration) showLogin('failed', `${error}`); }
  }

  async function pollLogin(gen) {
    if (gen !== loginGeneration || !get(auth).busy) return;
    try {
      const state = await ipc.invoke(`${AUTH[get(provider)].commands}_status`);
      if (gen !== loginGeneration) return;
      showLogin(state);
      if (state === 'pending') setTimeout(() => pollLogin(gen), 1500);
    } catch (error) { if (gen === loginGeneration) showLogin('failed', `${error}`); }
  }

  const api = {
    project, provider, models, entries, busy, auth, draft,

    async init() {
      ipc.invoke('chat_info').then((path) => project.set(path || null)).catch(() => {});
      await loadModels();
    },

    async selectProvider(name) {
      const previous = get(provider);
      try {
        await ipc.invoke('chat_select', { provider: name });
        provider.set(name);
        generation++;
        entries.set([]);
        await checkAuth(name);
        if (name !== 'antigravity' || get(auth).state === 'signed_in') await loadModels();
        else setModelsPlaceholder('Sign in to choose');
      } catch (error) {
        provider.set(previous);
        append({ kind: 'error', text: `${error}` });
      }
    },

    async setModel(id) {
      const before = get(models);
      try {
        await ipc.invoke('chat_set_model', { model: id });
        models.set({ ...before, current: id });
        const name = before.list.find((m) => m.id === id)?.name || id;
        append({ kind: 'meta', text: t('Model: {name} (from the next message)', { name }) });
      } catch (error) {
        models.set({ ...before });
        append({ kind: 'error', text: `${error}` });
      }
    },

    async login() {
      const spec = AUTH[get(provider)];
      if (!spec) return;
      const gen = ++loginGeneration;
      auth.set({ state: 'pending', message: spec.opening, error: '', busy: true });
      try {
        const state = await ipc.invoke(`${spec.commands}_start`);
        if (gen !== loginGeneration) return;
        showLogin(state);
        if (state === 'pending') pollLogin(gen);
      } catch (error) { if (gen === loginGeneration) showLogin('failed', `${error}`); }
    },

    async cancelLogin() {
      ++loginGeneration;
      try { showLogin(await ipc.invoke(`${AUTH[get(provider)].commands}_cancel`)); }
      catch (error) { showLogin('failed', `${error}`); }
    },

    canSend: () => !get(busy) && !get(auth).busy && !voiceActive(),

    async send(text) {
      text = text.trim();
      if (!text || !api.canSend()) return false;
      append({ kind: 'user', text });
      const gen = generation;
      let agent = null;
      const tools = new Map();
      const before = get(project) ? await ipc.invoke('history_list', { limit: 1 }).then((l) => l[0]?.commit, () => undefined) : undefined;
      const channel = ipc.channel((event) => {
        if (gen !== generation) return;
        switch (event.type) {
          case 'text_delta':
            if (agent === null) agent = append({ kind: 'agent', text: '', streaming: true });
            patch(agent, (e) => ({ text: e.text + event.delta }));
            break;
          case 'tool_call_started':
            if (agent !== null) { patch(agent, () => ({ streaming: false })); agent = null; }
            tools.set(event.call_id, append({ kind: 'tool', name: toolName(event.name), state: 'running', args: event.arguments, output: null }));
            break;
          case 'tool_call_completed': {
            const id = tools.get(event.call_id);
            if (id) patch(id, () => ({ state: event.is_error ? 'failed' : 'done', output: formatOutput(event.output) }));
            break;
          }
          case 'error':
            if (event.code === 'auth_required') authRequired(event.message);
            append({ kind: 'error', text: event.message });
            break;
          case 'turn_completed': {
            if (agent !== null) patch(agent, () => ({ streaming: false }));
            const usage = event.usage ? ` · ${event.usage.input_tokens} in / ${event.usage.output_tokens} out tokens` : '';
            if (event.finish_reason !== 'completed' || usage) append({ kind: 'meta', text: `${event.finish_reason}${usage}` });
            busy.set(false);
            api.recordChanges(before);
            onTurnEnd();
            break;
          }
          default: break; // turn_started, thinking_delta and status are not shown
        }
      });
      busy.set(true);
      const { map = null, x = null, y = null } = context();
      ipc.invoke('chat_send', { text, selectedMap: map, selectedX: x, selectedY: y, onEvent: channel }).catch((err) => {
        busy.set(false);
        if (authRequired(err)) append({ kind: 'meta', text: t('Sign in above, then send the message again.') });
        else append({ kind: 'error', text: `${err}` });
      });
      return true;
    },

    // Lists the history entries this turn added, so the reply shows what changed.
    async recordChanges(before) {
      if (before === undefined) return;
      try {
        const list = await ipc.invoke('history_list', { limit: 20 });
        const index = before ? list.findIndex((e) => e.commit === before) : list.length;
        const added = list.slice(0, index < 0 ? list.length : index);
        if (added.length) append({ kind: 'changes', changes: added });
      } catch { /* history unavailable */ }
    },

    stop: () => ipc.invoke('chat_cancel').catch((err) => append({ kind: 'error', text: `${err}` })),

    async reset() {
      generation++;
      await ipc.invoke('chat_reset');
      entries.set([]);
      busy.set(false);
    },
  };
  return api;
}
