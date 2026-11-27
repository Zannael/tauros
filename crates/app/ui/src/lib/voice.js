// AGT-004 dictation: explicit recording into the chat draft, never an automatic
// agent turn. The engine and speech language come from Settings.
import { writable, get } from 'svelte/store';

export function createVoice({ ipc, draft, settings, blocked = () => false, audioContext = () => globalThis.AudioContext,
  timers = { setTimeout: (...a) => setTimeout(...a), clearTimeout: (id) => clearTimeout(id) } }) {
  const phase = writable('idle');
  // `raw` texts come from the backend and are shown as sent; others are i18n keys.
  const feedback = writable({ text: '', error: false, raw: false, params: null });
  let generation = 0, timer = null, cue = null;
  const message = (text, error = false, params = null) => feedback.set({ text, error, raw: false, params });
  const raw = (text, error = false) => feedback.set({ text: String(text), error, raw: true, params: null });
  const cloud = () => settings().engine === 'claude';

  // Resume during the click gesture so WebKit can play the later ready cue.
  function prepareCue() {
    try { const Audio = audioContext(); if (!Audio) return; cue ||= new Audio(); cue.resume().catch(() => {}); }
    catch { /* visual readiness remains */ }
  }
  function readyCue() {
    try {
      if (!cue || cue.state !== 'running') return;
      const tone = cue.createOscillator(), gain = cue.createGain(), now = cue.currentTime;
      tone.frequency.value = 880;
      gain.gain.setValueAtTime(0, now);
      gain.gain.linearRampToValueAtTime(0.12, now + 0.01);
      gain.gain.linearRampToValueAtTime(0, now + 0.15);
      tone.connect(gain); gain.connect(cue.destination);
      tone.onended = () => { tone.disconnect(); gain.disconnect(); };
      tone.start(now); tone.stop(now + 0.16);
    } catch { /* a failed cue must not interrupt dictation */ }
  }
  function idle() {
    timers.clearTimeout(timer);
    phase.set('idle');
    const el = globalThis.document?.activeElement;
    ipc.invoke('set_typing', { typing: !!el?.matches?.('textarea, input, select') }).catch(() => {});
  }

  async function finish() {
    if (get(phase) !== 'recording') return;
    timers.clearTimeout(timer);
    phase.set('transcribing');
    const isCloud = cloud();
    message(isCloud ? 'Finishing Claude dictation and retrieving the draft…' : 'Transcribing on this computer… You can cancel.');
    const token = generation;
    try {
      const text = await ipc.invoke(isCloud ? 'claude_voice_finish' : 'voice_finish', { language: settings().language });
      if (token !== generation) return;
      // Preserve edits typed while the microphone or transcriber was active.
      draft.update((value) => value + (value && !/\s$/.test(value) ? '\n' : '') + text);
      message('Transcription added to the draft. Review it, then press Send.');
    } catch (error) {
      if (token !== generation) return;
      raw(error?.message ?? error, true);
    } finally {
      if (token === generation) idle();
    }
  }

  const api = {
    phase, feedback,
    get active() { return get(phase) !== 'idle'; },

    async status() {
      if (cloud()) {
        message('Experimental Claude CLI dictation: microphone audio goes to Anthropic using your Claude sign-in. Italian. Prepare the microphone, then wait for the beep and Speak now before speaking. Review before Send.');
        return;
      }
      try { const s = await ipc.invoke('voice_status'); raw(s.message, !s.ready); }
      catch (error) { raw(`Cannot check local transcription: ${error}`, true); }
    },

    async toggle() {
      if (get(phase) === 'recording') { await finish(); return; }
      if (get(phase) !== 'idle' || blocked()) return;
      phase.set('starting');
      const token = ++generation;
      const isCloud = cloud();
      if (isCloud) prepareCue();
      message(isCloud ? 'Wait before speaking. Start after the beep and “Speak now”… Connecting and warming up Claude audio. Audio goes to Anthropic.' : 'Opening microphone…');
      try {
        const status = isCloud ? { ready: true, max_seconds: 60 } : await ipc.invoke('voice_status');
        if (!status.ready) throw new Error(status.message);
        await ipc.invoke(isCloud ? 'claude_voice_start' : 'voice_start');
        phase.set('recording');
        ipc.invoke('set_buttons', { mask: 0 }).catch(() => {});
        ipc.invoke('set_typing', { typing: true }).catch(() => {});
        message(isCloud ? 'Speak now · Claude cloud dictation · Italian · maximum {n} seconds. Press Stop & transcribe when finished.'
          : 'Recording · maximum {n} seconds. Press Stop & transcribe when finished.', false, { n: status.max_seconds });
        if (isCloud) readyCue();
        timer = timers.setTimeout(finish, status.max_seconds * 1000);
      } catch (error) {
        if (token !== generation) return;
        raw(error?.message ?? error, true);
        idle();
      }
    },

    async cancel() {
      if (['idle', 'starting', 'cancelling'].includes(get(phase))) return;
      ++generation; // late transcription results must never reach the draft
      timers.clearTimeout(timer);
      phase.set('cancelling');
      message('Cancelling voice input…');
      try { await ipc.invoke(cloud() ? 'claude_voice_cancel' : 'voice_cancel'); message('Voice input cancelled. Your draft is unchanged.'); }
      catch (error) { raw(`Cannot cancel voice input: ${error}`, true); }
      finally { idle(); }
    },
  };
  return api;
}
