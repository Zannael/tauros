// AGT-004: explicit local dictation into the draft, never an automatic agent turn.
(() => {
  const record = document.getElementById('voice-record');
  const cancel = document.getElementById('voice-cancel');
  const language = document.getElementById('voice-language');
  const engine = document.getElementById('voice-engine');
  const feedback = document.getElementById('voice-status');
  let phase = 'idle', generation = 0, timer = null, cueAudio = null;
  function prepareCue() {
    // Resume during the click gesture so WebKit can play the later ready cue.
    try {
      const Audio = window.AudioContext || window.webkitAudioContext;
      if (!Audio) return;
      cueAudio ||= new Audio();
      cueAudio.resume().catch(() => {});
    } catch (_) { /* Visual readiness remains available without audio output. */ }
  }
  function readyCue() {
    try {
      if (!cueAudio || cueAudio.state !== 'running') return;
      const tone = cueAudio.createOscillator(), gain = cueAudio.createGain();
      const now = cueAudio.currentTime;
      tone.frequency.value = 880;
      gain.gain.setValueAtTime(0, now);
      gain.gain.linearRampToValueAtTime(0.12, now + 0.01);
      gain.gain.linearRampToValueAtTime(0, now + 0.15);
      tone.connect(gain);
      gain.connect(cueAudio.destination);
      tone.onended = () => { tone.disconnect(); gain.disconnect(); };
      tone.start(now);
      tone.stop(now + 0.16);
    } catch (_) { /* A failed cue must not interrupt dictation. */ }
  }
  function message(text, error = false) {
    feedback.textContent = text;
    feedback.classList.toggle('error', error);
  }
  function refresh() {
    const active = phase !== 'idle';
    record.textContent = phase === 'recording' ? 'Stop & transcribe' : phase === 'transcribing' ? 'Transcribing…' : phase === 'starting' ? 'Wait — preparing…' : engine.value === 'claude' ? 'Prepare microphone' : 'Record';
    record.disabled = chatBusy || authBusy || ['starting', 'transcribing', 'cancelling'].includes(phase);
    cancel.hidden = !active;
    cancel.textContent = phase === 'transcribing' ? 'Cancel transcription' : 'Cancel recording';
    cancel.disabled = ['starting', 'cancelling'].includes(phase);
    engine.disabled = active;
    language.disabled = active || engine.value === 'claude';
    chatSend.disabled = chatBusy || authBusy || active;
    document.getElementById('chat-new').disabled = authBusy || active;
    document.getElementById('chat-provider').disabled = chatBusy || authBusy || active;
    chatModel.disabled = chatBusy || authBusy || active || !modelsReady;
    document.getElementById('auth-login').disabled = chatBusy || authBusy || active;
  }
  function idle() {
    clearTimeout(timer);
    phase = 'idle';
    refresh();
    const field = document.activeElement;
    invoke('set_typing', { typing: !!field?.matches('textarea, input, select') }).catch(() => {});
  }
  window.voiceControls = { get active() { return phase !== 'idle'; }, refresh };
  async function finish() {
    if (phase !== 'recording') return;
    clearTimeout(timer);
    phase = 'transcribing';
    refresh();
    const cloud = engine.value === 'claude';
    message(cloud ? 'Finishing Claude dictation and retrieving the draft…' : 'Transcribing on this computer… You can cancel.');
    const token = generation;
    try {
      const text = await invoke(cloud ? 'claude_voice_finish' : 'voice_finish', { language: language.value });
      if (token !== generation) return;
      // Preserve edits typed while the microphone/transcriber was active.
      chatInput.value += (chatInput.value && !/\s$/.test(chatInput.value) ? '\n' : '') + text;
      message('Transcription added to the draft. Review it, then press Send.');
      chatInput.focus();
    } catch (error) {
      if (token !== generation) return;
      message(String(error), true);
    } finally {
      if (token === generation) idle();
    }
  }
  record.addEventListener('click', async () => {
    if (phase === 'recording') { await finish(); return; }
    if (phase !== 'idle' || chatBusy || authBusy) return;
    phase = 'starting';
    const token = ++generation;
    refresh();
    const cloud = engine.value === 'claude';
    if (cloud) prepareCue();
    message(cloud ? 'Wait before speaking. Start after the beep and “Speak now”… Connecting and warming up Claude audio. Audio goes to Anthropic.' : 'Opening microphone…');
    try {
      const status = cloud ? { ready: true, max_seconds: 60 } : await invoke('voice_status');
      if (!status.ready) throw new Error(status.message);
      await invoke(cloud ? 'claude_voice_start' : 'voice_start');
      phase = 'recording';
      refresh();
      invoke('set_buttons', { mask: 0 }).catch(() => {});
      invoke('set_typing', { typing: true }).catch(() => {});
      message(`${cloud ? 'Speak now · Claude cloud dictation · Italian' : 'Recording'} · maximum ${status.max_seconds} seconds. Press Stop & transcribe when finished.`);
      if (cloud) readyCue();
      timer = setTimeout(finish, status.max_seconds * 1000);
    } catch (error) {
      if (token !== generation) return;
      message(String(error), true);
      idle();
    }
  });
  cancel.addEventListener('click', async () => {
    if (phase === 'idle' || phase === 'starting' || phase === 'cancelling') return;
    ++generation; // Late transcription results must never reappear in the draft.
    clearTimeout(timer);
    phase = 'cancelling';
    refresh();
    message('Cancelling voice input…');
    try {
      await invoke(engine.value === 'claude' ? 'claude_voice_cancel' : 'voice_cancel');
      message('Voice input cancelled. Your draft is unchanged.');
    } catch (error) {
      message(`Cannot cancel voice input: ${error}`, true);
    } finally { idle(); }
  });
  invoke('voice_status').then(status => { if (engine.value !== 'claude') message(status.message, !status.ready); })
    .catch(error => message(`Cannot check local transcription: ${error}`, true));
  engine.addEventListener('change', () => {
    if (engine.value === 'claude') message('Experimental Claude CLI dictation: microphone audio goes to Anthropic using your Claude sign-in. Italian. Prepare the microphone, then wait for the beep and Speak now before speaking. Review before Send.');
    else invoke('voice_status').then(status => message(status.message, !status.ready)).catch(error => message(String(error), true));
    refresh();
  });
  refresh();
})();
