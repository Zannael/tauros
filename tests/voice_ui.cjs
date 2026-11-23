// Execute the actual dictation UI against local IPC peers; no device/provider call.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const code = fs.readFileSync('crates/app/ui/voice.js', 'utf8');
function element() {
  return { value: '', disabled: false, hidden: false, textContent: '', classList: {toggle(){}},
    addEventListener(name, fn) { this[name] = fn; }, focus(){}, matches(){return false;} };
}
const ids = ['voice-record','voice-cancel','voice-language','voice-engine','voice-status','chat-new','chat-provider','auth-login'];
async function scenario(mode, engine = 'offline', audio = 'available') {
  const elements = Object.fromEntries(ids.map(id => [id, element()])); elements['voice-language'].value = 'it';
  elements['voice-engine'].value = engine;
  const draft = element(); draft.value = 'Existing draft';
  const calls = []; let complete, ready, timers = 0, beeps = 0, resumes = 0;
  class CueAudio {
    constructor() { this.state = 'running'; this.currentTime = 0; }
    resume() { resumes++; return Promise.resolve(); }
    createOscillator() {
      if (audio === 'failed') throw Error('Output unavailable');
      return { frequency: {}, connect(){}, disconnect(){}, start(){beeps++;}, stop(){this.onended();} };
    }
    createGain() {
      return {gain: {setValueAtTime(){}, linearRampToValueAtTime(){}}, connect(){}, disconnect(){}};
    }
  }
  const context = vm.createContext({ window: audio === 'absent' ? {} : {AudioContext: CueAudio}, chatBusy: false, authBusy: false, modelsReady: true,
    chatInput: draft, chatSend: element(), chatModel: element(),
    document: {getElementById: id => elements[id]}, setTimeout: () => ++timers, clearTimeout(){},
    invoke: async (command, args) => {
      calls.push([command,args]);
      if (command === 'claude_voice_start') return new Promise(resolve => { ready = resolve; });
      if (command === 'voice_status') return {ready: true,message:'Ready',max_seconds:60};
      if (command === 'voice_finish' || command === 'claude_voice_finish') {
        if (mode === 'failure') throw Error('No speech detected');
        if (mode === 'cancel') return new Promise(resolve => {complete = resolve;});
        return 'Mostra la mappa';
      }
    }});
  vm.runInContext(code, context); await Promise.resolve();
  assert.equal(elements['voice-record'].textContent, engine === 'claude' ? 'Prepare microphone' : 'Record');
  const start = elements['voice-record'].click();
  if (engine === 'claude') {
    assert.equal(elements['voice-record'].disabled, true);
    assert.match(elements['voice-status'].textContent, /Wait before speaking/);
    assert.equal(timers, 0);
    assert.equal(beeps, 0);
    assert.equal(resumes, audio === 'absent' ? 0 : 1);
    ready();
  }
  await start;
  assert.equal(timers, 1);
  assert.equal(beeps, engine === 'claude' && audio === 'available' ? 1 : 0);
  if (engine === 'claude') assert.match(elements['voice-status'].textContent, /Speak now/);
  assert.equal(context.window.voiceControls.active,true);
  assert.equal(context.chatSend.disabled,true);
  assert.equal(elements['chat-new'].disabled,true);
  draft.value += ' typed during recording';
  const finish = elements['voice-record'].click();
  if(mode === 'cancel') {
    await elements['voice-cancel'].click(); complete('Late result');
  }
  await finish;
  assert.equal(context.window.voiceControls.active,false);
  assert.equal(context.chatSend.disabled,false);
  assert.equal(draft.value, mode === 'success' ? 'Existing draft typed during recording\nMostra la mappa' : 'Existing draft typed during recording');
  assert(!calls.some(([command]) => command === 'chat_send'));
  assert(calls.some(([command]) => command === (engine === 'claude' ? 'claude_voice_start' : 'voice_start')));
  assert.equal(calls.some(([command]) => command === 'voice_start'), engine === 'offline');
  assert.match(elements['voice-status'].textContent, mode === 'success' ? /Review it/ : mode === 'cancel' ? /cancelled/ : /No speech/);
}
(async () => {
  for (const audio of ['available','absent','failed'])
    for (const engine of ['offline','claude'])
      for (const mode of ['success','failure','cancel']) await scenario(mode, engine, audio);
  console.log('Voice UI passed: review-only draft, preserved typing, errors, cancellation and late-result suppression.');
})().catch(error => {console.error(error);process.exitCode=1;});
