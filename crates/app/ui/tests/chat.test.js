// Agent chat controller against mocked IPC: streamed text and tool calls, the
// changes a turn recorded, provider changes, sign-in states and send guards.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { get } from 'svelte/store';
import { createChat } from '../src/lib/chat.js';

function setup(overrides = {}) {
  const calls = [];
  let turn = null, history = [{ commit: 'aaa', summary: 'before' }];
  const ipc = {
    channel(fn) { return { fn }; },
    invoke: async (cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === 'chat_info') return '/data/projects/demo';
      if (cmd === 'chat_models') return [{ id: 'm1', name: 'Model one', current: true }, { id: 'm2', name: 'Model two' }];
      if (cmd === 'chat_send') { turn = args.onEvent.fn; return null; }
      if (cmd === 'history_list') return history;
      if (overrides[cmd]) return overrides[cmd](args);
      return null;
    },
  };
  let ended = 0;
  const chat = createChat({ ipc, context: () => ({ map: 'MAP_X', x: 3, y: 4 }), onTurnEnd: () => ended++, voiceActive: () => overrides.voice === true });
  return { chat, calls, emit: (e) => turn(e), setHistory: (h) => { history = h; }, ended: () => ended };
}

test('a streamed turn shows text, tools and the recorded changes', async () => {
  const { chat, calls, emit, setHistory, ended } = setup();
  await chat.init();
  assert.equal(get(chat.models).current, 'm1');
  assert.equal(await chat.send('  Add a trainer  '), true);
  assert.deepEqual(calls.find(([c]) => c === 'chat_send')[1].selectedMap, 'MAP_X');
  assert.equal(get(chat.busy), true);
  assert.equal(chat.canSend(), false);
  emit({ type: 'text_delta', delta: 'Work' });
  emit({ type: 'text_delta', delta: 'ing' });
  emit({ type: 'tool_call_started', call_id: 'c1', name: 'mcp__tauros__create_trainer', arguments: { name: 'X' } });
  emit({ type: 'tool_call_completed', call_id: 'c1', is_error: false, output: [{ type: 'text', text: 'created' }] });
  setHistory([{ commit: 'bbb', summary: 'create_trainer', origin: 'agent-generated' }, { commit: 'aaa', summary: 'before' }]);
  emit({ type: 'turn_completed', finish_reason: 'completed' });
  await new Promise((r) => setTimeout(r, 0));
  const entries = get(chat.entries);
  assert.deepEqual(entries.map((e) => e.kind), ['user', 'agent', 'tool', 'changes']);
  assert.equal(entries[0].text, 'Add a trainer');
  assert.equal(entries[1].text, 'Working');
  assert.equal(entries[1].streaming, false);
  assert.deepEqual([entries[2].name, entries[2].state, entries[2].output], ['create_trainer', 'done', 'created']);
  assert.deepEqual(entries[3].changes.map((c) => c.commit), ['bbb']);
  assert.equal(get(chat.busy), false);
  assert.equal(ended(), 1);
});

test('a new chat ignores the old turn’s late events', async () => {
  const { chat, emit } = setup();
  await chat.send('first');
  await chat.reset();
  emit({ type: 'text_delta', delta: 'late' });
  assert.deepEqual(get(chat.entries), []);
});

test('changing provider clears the conversation and checks Antigravity sign-in', async () => {
  const { chat, calls } = setup({ antigravity_login_check: () => 'signed_out' });
  await chat.send('hello');
  await chat.selectProvider('antigravity');
  assert.deepEqual(get(chat.entries), []);
  assert.equal(get(chat.provider), 'antigravity');
  assert.equal(get(chat.auth).state, 'signed_out');
  assert.equal(get(chat.models).placeholder, 'Sign in to choose');
  assert(!calls.slice(-2).some(([c]) => c === 'chat_models'), 'models wait for sign-in');
});

test('Codex sign-in shows pending, then success, and blocks sending meanwhile', async () => {
  const { chat } = setup({ codex_login_start: () => 'pending', codex_login_status: () => 'signed_in' });
  await chat.selectProvider('codex_chatgpt');
  const login = chat.login();
  assert.equal(get(chat.auth).busy, true);
  assert.equal(chat.canSend(), false);
  await login;
  await new Promise((r) => setTimeout(r, 0));
  assert.equal(get(chat.auth).state, 'signed_in');
  assert.equal(get(chat.auth).busy, false);
  assert.equal(chat.canSend(), true);
});

test('dictation in progress blocks sending', async () => {
  const { chat, calls } = setup({ voice: true });
  assert.equal(await chat.send('hello'), false);
  assert(!calls.some(([c]) => c === 'chat_send'));
});

test('a failing send is reported and frees the chat', async () => {
  const failing = createChat({ ipc: { channel: () => ({}), invoke: async (c) => { if (c === 'chat_send') throw 'runtime missing'; return []; } } });
  await failing.send('x');
  await new Promise((r) => setTimeout(r, 0));
  assert.equal(get(failing.busy), false);
  assert.equal(get(failing.entries).at(-1).text, 'runtime missing');
});
