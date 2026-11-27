<script>
  // The agent beside every view: provider and model (chosen only here), sign-in
  // for providers that need it, the streamed conversation and the composer with
  // dictation. The agent's context follows what is open.
  import Icon from './Icon.svelte';
  import { chat, voice, project, navigate, toggleAgent } from '../lib/app.js';
  import { PROVIDERS, AUTH } from '../lib/chat.js';
  import { uiPrefs } from '../lib/prefs.js';
  import { t } from '../lib/i18n.js';
  const { provider, models, entries, busy, auth, draft, project: path } = chat;
  const { phase, feedback } = voice;
  const { point } = project;
  let log, input;
  let voiceTouched = $state(false);

  const authSpec = $derived(AUTH[$provider]);
  const voiceActive = $derived($phase !== 'idle');
  const locked = $derived($busy || $auth.busy || voiceActive);
  const projectName = $derived($path ? $path.split(/[\\/]/).filter(Boolean).pop() : '');
  const recordLabel = $derived($phase === 'recording' ? $t('Stop & transcribe') : $phase === 'transcribing' ? $t('Transcribing…')
    : $phase === 'starting' ? $t('Wait — preparing…') : $uiPrefs.voiceEngine === 'claude' ? $t('Prepare microphone') : $t('Record'));

  $effect(() => { $entries; queueMicrotask(() => { if (log) log.scrollTop = log.scrollHeight; }); });

  async function submit(e) {
    e?.preventDefault();
    const text = $draft;
    if (await chat.send(text)) draft.set('');
  }
  function onKey(e) {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); submit(); }
  }
  function record() { voiceTouched = true; voice.toggle(); }
  const examples = ['Add a trainer with three Pokémon on Route 104.', 'Why does the build fail?', 'Boot the game at Petalburg City and take a screenshot.'];
</script>

<aside class="agent" aria-label={$t('Agent')}>
  <header class="head">
    <div class="row">
      <label class="field grow"><span>{$t('Agent')}</span>
        <select value={$provider} disabled={locked} onchange={(e) => chat.selectProvider(e.currentTarget.value)}>
          {#each PROVIDERS as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
        </select>
      </label>
      <label class="field grow"><span>{$t('Model')}</span>
        <select value={$models.current} disabled={locked || !$models.ready} title={$t('Applies from the next message')}
          onchange={(e) => chat.setModel(e.currentTarget.value)}>
          {#if $models.ready}
            {#each $models.list as m (m.id)}<option value={m.id} title={m.description || ''}>{m.name}</option>{/each}
          {:else}<option value="">{$t($models.placeholder)}</option>{/if}
        </select>
      </label>
    </div>
    <div class="row tools">
      <span class="context muted small ellipsis grow" title={$t('What the agent works on')}>
        {#if projectName}{projectName + ($point.map ? ` · ${$point.map} (${$point.x}, ${$point.y})` : '')}
        {:else}{$t('No project: start Tauros with --project to give the agent its tools')}{/if}
      </span>
      <button type="button" class="ghost" disabled={$auth.busy || voiceActive} onclick={chat.reset} title={$t('Start a new conversation')}>
        <Icon name="plus" size={15} />{$t('New chat')}</button>
      <button type="button" class="ghost icon" onclick={toggleAgent} title={$t('Hide the agent panel (Ctrl+J)')}
        aria-label={$t('Hide the agent panel (Ctrl+J)')}><Icon name="x" size={16} /></button>
    </div>
    {#if authSpec}
      <div class="auth" data-state={$auth.state}>
        <div class="row">
          <strong class="grow">{$t(authSpec.title)}</strong>
          {#if $auth.busy}
            <button type="button" onclick={chat.cancelLogin}>{$t('Cancel sign-in')}</button>
          {:else}
            <button type="button" class="primary" disabled={$busy || voiceActive} aria-label={$t(authSpec.label)} onclick={chat.login}>
              {$auth.state === 'signed_in' ? $t('Check connection') : $t('Sign in')}</button>
          {/if}
        </div>
        <p class="small" role="status" aria-live="polite">{$auth.error || $t($auth.message)}</p>
      </div>
    {/if}
  </header>

  <div class="log" bind:this={log} aria-live="polite">
    {#each $entries as entry (entry.id)}
      {#if entry.kind === 'user'}<div class="msg user">{entry.text}</div>
      {:else if entry.kind === 'agent'}<div class="msg agent" class:streaming={entry.streaming}>{entry.text}</div>
      {:else if entry.kind === 'error'}<div class="msg error" role="alert">{entry.text}</div>
      {:else if entry.kind === 'meta'}<div class="meta">{entry.text}</div>
      {:else if entry.kind === 'tool'}
        <details class="tool {entry.state}" open={entry.state === 'failed'}>
          <summary><span class="name">{entry.name}</span>
            <span class="state">{#if entry.state === 'running'}<span class="spin"></span>{$t('running…')}{:else}{$t(entry.state)}{/if}</span></summary>
          <pre>{$t('arguments')}: {JSON.stringify(entry.args, null, 2)}</pre>
          {#if entry.output !== null}<pre>{$t('result')}: {entry.output}</pre>{/if}
        </details>
      {:else if entry.kind === 'changes'}
        <div class="changes card">
          <strong>{$t('Changes recorded in this reply')}</strong>
          <ul>{#each entry.changes as c (c.commit)}<li><span class="chip">{c.provenance || c.origin}</span> <span class="ellipsis">{c.summary}</span></li>{/each}</ul>
          <button type="button" onclick={() => navigate('history')}><Icon name="history" size={15} />{$t('Review or restore in History')}</button>
        </div>
      {/if}
    {:else}
      <div class="empty">
        <Icon name="sparkles" size={26} />
        <strong>{$t('Ask the agent to change the game')}</strong>
        <p class="small">{$t('It reads and edits the project, builds, boots the game and checks the result. Every change is recorded in History and can be restored.')}</p>
        <div class="examples">
          {#each examples as ex}<button type="button" class="ghost small" onclick={() => { draft.set($t(ex)); input?.focus(); }}>{$t(ex)}</button>{/each}
        </div>
      </div>
    {/each}
  </div>

  <form class="composer" onsubmit={submit}>
    <textarea bind:this={input} bind:value={$draft} onkeydown={onKey} rows="3"
      placeholder={$t('Ask the agent… (Enter sends, Shift+Enter new line)')} aria-label={$t('Message to the agent')}></textarea>
    {#if voiceTouched || voiceActive || $feedback.error}
      <p class="voice small" class:error-text={$feedback.error} role="status" aria-live="polite">
        {$feedback.raw ? $feedback.text : $t($feedback.text, $feedback.params)}</p>
    {/if}
    <div class="row">
      <button type="button" class:primary={$phase === 'recording'} disabled={$busy || $auth.busy || ['starting', 'transcribing', 'cancelling'].includes($phase)}
        onclick={record} title={$feedback.raw && !voiceTouched ? $feedback.text : recordLabel}>
        {#if $phase === 'starting' || $phase === 'transcribing'}<span class="spin"></span>{:else}<Icon name={$phase === 'recording' ? 'stop' : 'mic'} size={16} />{/if}
        <span>{recordLabel}</span></button>
      {#if voiceActive}
        <button type="button" disabled={['starting', 'cancelling'].includes($phase)} onclick={voice.cancel}>
          {$phase === 'transcribing' ? $t('Cancel transcription') : $t('Cancel recording')}</button>
      {/if}
      <span class="grow"></span>
      {#if $busy}
        <button type="button" onclick={chat.stop}><Icon name="stop" size={15} />{$t('Stop')}</button>
      {:else}
        <button type="submit" class="primary" disabled={locked || !$draft.trim()}><Icon name="send" size={15} />{$t('Send')}</button>
      {/if}
    </div>
  </form>
</aside>

<style>
  .agent { width: 380px; flex: none; display: flex; flex-direction: column; min-height: 0; background: var(--surface); border-left: 1px solid var(--border); }
  .head { padding: 10px 12px; display: grid; grid-template-columns: minmax(0, 1fr); gap: 8px; border-bottom: 1px solid var(--border); }
  .head select { width: 100%; }
  .tools { gap: 4px; }
  .tools button { min-height: 28px; }
  .auth { padding: 9px 10px; border-radius: var(--radius); background: var(--surface-2); display: grid; gap: 6px; }
  .auth[data-state="failed"] p { color: var(--err); }
  .auth[data-state="signed_in"] p { color: var(--ok); }
  .auth[data-state="pending"] p, .auth[data-state="signed_out"] p { color: var(--warn); }
  .log { flex: 1; overflow-y: auto; padding: 12px; display: flex; flex-direction: column; gap: 8px; min-height: 0; }
  .msg { padding: 8px 10px; border-radius: 10px; white-space: pre-wrap; overflow-wrap: anywhere; }
  .msg.user { align-self: flex-end; max-width: 88%; background: var(--accent-soft); color: var(--text); }
  .msg.agent { background: var(--surface-2); }
  .msg.agent.streaming::after { content: '▍'; color: var(--accent-text); }
  .msg.error { background: var(--err-soft); color: var(--err); }
  .meta { color: var(--muted); font-size: 11.5px; padding: 0 2px; }
  .tool { border: 1px solid var(--border); border-radius: var(--radius-sm); font-size: 12px; background: var(--surface); }
  .tool summary { padding: 6px 9px; cursor: pointer; display: flex; gap: 8px; align-items: center; }
  .tool .name { font-family: var(--mono); color: var(--accent-text); flex: 1; overflow-wrap: anywhere; }
  .tool .state { display: inline-flex; gap: 5px; align-items: center; color: var(--muted); font-size: 11.5px; }
  .tool.done .state { color: var(--ok); }
  .tool.failed .state { color: var(--err); }
  .tool pre { margin: 0; padding: 7px 9px; border-top: 1px solid var(--border); max-height: 200px; overflow: auto;
    white-space: pre-wrap; overflow-wrap: anywhere; font-size: 11px; color: var(--muted); }
  .changes { padding: 9px 10px; display: grid; gap: 6px; font-size: 12px; }
  .changes ul { margin: 0; padding: 0; list-style: none; display: grid; gap: 4px; }
  .changes li { display: flex; gap: 6px; align-items: center; min-width: 0; }
  .changes button { justify-self: start; }
  .examples { display: grid; gap: 4px; width: 100%; }
  .examples button { white-space: normal; text-align: left; justify-content: flex-start; height: auto; padding: 6px 8px; color: var(--accent-text); }
  .composer { border-top: 1px solid var(--border); padding: 10px 12px; display: grid; gap: 8px; }
  .composer textarea { width: 100%; min-height: 64px; max-height: 220px; }
  .voice { color: var(--muted); white-space: pre-wrap; }
  @media (max-width: 1280px) { .agent { width: 340px; } }
</style>
