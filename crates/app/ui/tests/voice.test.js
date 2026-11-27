// Dictation against local IPC peers (ported from tests/voice_ui.cjs): the result
// only reaches the draft, typing during recording is kept, errors and
// cancellation are reported and late results are dropped. No device or provider.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { writable, get } from 'svelte/store';
import { createVoice } from '../src/lib/voice.js';

async function scenario(mode, engine, audio) {
  const draft = writable('Existing draft');
  const calls = [];
  let complete, ready, timers = 0, beeps = 0, resumes = 0;
  class CueAudio {
    constructor() { this.state = 'running'; this.currentTime = 0; }
    resume() { resumes++; return Promise.resolve(); }
    createOscillator() {
      if (audio === 'failed') throw Error('Output unavailable');
      return { frequency: {}, connect() {}, disconnect() {}, start() { beeps++; }, stop() { this.onended(); } };
    }
    createGain() { return { gain: { setValueAtTime() {}, linearRampToValueAtTime() {} }, connect() {}, disconnect() {} }; }
  }
  const ipc = {
    invoke: async (command, args) => {
      calls.push([command, args]);
      if (command === 'claude_voice_start') return new Promise((resolve) => { ready = resolve; });
      if (command === 'voice_status') return { ready: true, message: 'Ready', max_seconds: 60 };
      if (command === 'voice_finish' || command === 'claude_voice_finish') {
        if (mode === 'failure') throw Error('No speech detected');
        if (mode === 'cancel') return new Promise((resolve) => { complete = resolve; });
        return 'Mostra la mappa';
      }
    },
  };
  const voice = createVoice({
    ipc, draft, settings: () => ({ engine, language: 'it' }),
    audioContext: () => (audio === 'absent' ? undefined : CueAudio),
    timers: { setTimeout: () => ++timers, clearTimeout() {} },
  });
  const start = voice.toggle();
  if (engine === 'claude') {
    assert.equal(get(voice.phase), 'starting');
    assert.match(get(voice.feedback).text, /Wait before speaking/);
    assert.equal(timers, 0);
    assert.equal(beeps, 0);
    assert.equal(resumes, audio === 'absent' ? 0 : 1);
    ready();
  }
  await start;
  assert.equal(timers, 1);
  assert.equal(beeps, engine === 'claude' && audio === 'available' ? 1 : 0);
  if (engine === 'claude') assert.match(get(voice.feedback).text, /Speak now/);
  assert.equal(voice.active, true);
  draft.update((v) => v + ' typed during recording');
  const finish = voice.toggle();
  if (mode === 'cancel') { await voice.cancel(); complete('Late result'); }
  await finish;
  assert.equal(voice.active, false);
  assert.equal(get(draft), mode === 'success' ? 'Existing draft typed during recording\nMostra la mappa' : 'Existing draft typed during recording');
  assert(!calls.some(([c]) => c === 'chat_send'), 'dictation never sends a message');
  assert.equal(calls.some(([c]) => c === 'voice_start'), engine === 'offline');
  assert(calls.some(([c]) => c === (engine === 'claude' ? 'claude_voice_start' : 'voice_start')));
  assert.match(get(voice.feedback).text, mode === 'success' ? /Review it/ : mode === 'cancel' ? /cancelled/ : /No speech/);
}

for (const audio of ['available', 'absent', 'failed'])
  for (const engine of ['offline', 'claude'])
    for (const mode of ['success', 'failure', 'cancel'])
      test(`dictation ${engine} ${mode} with audio ${audio}`, () => scenario(mode, engine, audio));

test('a busy agent or sign-in blocks recording', async () => {
  const calls = [];
  const voice = createVoice({ ipc: { invoke: async (c) => { calls.push(c); } }, draft: writable(''),
    settings: () => ({ engine: 'offline', language: 'it' }), blocked: () => true });
  await voice.toggle();
  assert.equal(get(voice.phase), 'idle');
  assert.deepEqual(calls, []);
});
