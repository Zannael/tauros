<script>
  // App preferences, applied as soon as they change. Agent provider and model are
  // chosen in the agent panel, where they are used.
  import { onDestroy } from 'svelte';
  import KeyConfig from '../components/KeyConfig.svelte';
  import { emu, chat, voice, projects, viewActions } from '../lib/app.js';
  import { uiPrefs, setPref } from '../lib/prefs.js';
  import { LANGUAGES, t } from '../lib/i18n.js';
  const { config } = emu;
  const { project: path } = chat;
  const { phase } = voice;
  viewActions.set([]);
  onDestroy(() => viewActions.set([]));
  const prefs = $derived($config.prefs);
  const { defaultDir } = projects;
  const folder = $derived($uiPrefs.projectsDir || $defaultDir);
  async function chooseFolder() {
    const dir = await projects.chooseFolder(folder, $t('Projects folder'));
    if (dir) projects.setFolder(dir);
  }
  function setEngine(engine) { setPref('voiceEngine', engine); voice.status(); }
</script>

<div class="settings scroll">
  <section class="card">
    <h2>{$t('Appearance')}</h2>
    <div class="field-row">
      <span class="label">{$t('Theme')}</span>
      <div class="seg" role="radiogroup" aria-label={$t('Theme')}>
        {#each [['system', 'Follow the system'], ['light', 'Light'], ['dark', 'Dark']] as [id, label]}
          <button type="button" role="radio" aria-checked={$uiPrefs.theme === id} onclick={() => setPref('theme', id)}>{$t(label)}</button>
        {/each}
      </div>
    </div>
    <label class="field-row"><span class="label">{$t('Interface language')}</span>
      <select value={$uiPrefs.language} onchange={(e) => setPref('language', e.currentTarget.value)}>
        {#each Object.entries(LANGUAGES) as [id, name]}<option value={id}>{name}</option>{/each}
      </select></label>
  </section>

  <section class="card">
    <h2>{$t('Projects')}</h2>
    <div class="field-row">
      <span class="label">{$t('Projects folder')}</span>
      <code class="folder grow" title={folder}>{folder}</code>
    </div>
    <div class="row">
      <button type="button" onclick={chooseFolder}>{$t('Choose…')}</button>
      <button type="button" disabled={!$uiPrefs.projectsDir} onclick={() => projects.setFolder($defaultDir)}>{$t('Use the default folder')}</button>
    </div>
    <p class="small muted">{$t('The Project view lists the game projects in this folder. The default is the projects folder in the Tauros data folder.')}</p>
  </section>

  <section class="card">
    <h2>{$t('Dictation')}</h2>
    <label class="field-row"><span class="label">{$t('Engine')}</span>
      <select value={$uiPrefs.voiceEngine} disabled={$phase !== 'idle'} onchange={(e) => setEngine(e.currentTarget.value)}>
        <option value="offline">{$t('Offline Whisper (on this computer)')}</option>
        <option value="claude">{$t('Claude CLI · experimental cloud')}</option>
      </select></label>
    <label class="field-row"><span class="label">{$t('Speech language')}</span>
      <select value={$uiPrefs.voiceLanguage} disabled={$phase !== 'idle' || $uiPrefs.voiceEngine === 'claude'}
        onchange={(e) => setPref('voiceLanguage', e.currentTarget.value)}>
        <option value="it">Italiano</option><option value="en">English</option><option value="auto">{$t('Automatic')}</option>
      </select></label>
    <p class="small muted">{$uiPrefs.voiceEngine === 'claude'
      ? $t('Microphone audio goes to Anthropic using your Claude sign-in; Italian only. Wait for the beep and “Speak now” before speaking.')
      : $t('Audio is transcribed on this computer and never leaves it.')} {$t('Dictation only fills the draft: you always review it and press Send.')}</p>
  </section>

  <section class="card">
    <h2>{$t('Emulator')}</h2>
    <label class="check"><input type="checkbox" checked={prefs.pause_on_blur} onchange={() => emu.savePrefs({ pause_on_blur: !prefs.pause_on_blur })} />
      {$t('Pause the game when Tauros is hidden')}</label>
    <label class="check"><input type="checkbox" checked={prefs.rewind_enabled} onchange={() => emu.savePrefs({ rewind_enabled: !prefs.rewind_enabled })} />
      {$t('Keep a rewind buffer')}</label>
    <h3>{$t('Game keys')}</h3>
    <KeyConfig />
  </section>

  <section class="card">
    <h2>{$t('About')}</h2>
    <dl>
      <dt>{$t('Project')}</dt><dd class="mono">{$path || $t('No project open')}</dd>
      <dt>{$t('Agent provider and model')}</dt><dd>{$t('Chosen in the agent panel, where sign-in happens too.')}</dd>
    </dl>
  </section>
</div>

<style>
  .settings { flex: 1; padding: 14px; display: grid; gap: 14px; align-content: start; grid-template-columns: minmax(0, 760px); }
  .card { padding: 16px; display: grid; gap: 12px; }
  .field-row { display: flex; align-items: center; gap: 14px; flex-wrap: wrap; }
  .label { width: 170px; color: var(--muted); }
  .field-row select { min-width: 260px; }
  .seg { display: inline-flex; border: 1px solid var(--border-strong); border-radius: var(--radius-sm); overflow: hidden; }
  .seg button { border: 0; border-radius: 0; }
  .seg button[aria-checked="true"] { background: var(--accent); color: var(--on-accent); font-weight: 600; }
  .folder { overflow-wrap: anywhere; padding: 5px 9px; background: var(--surface-2); border-radius: var(--radius-sm); }
  dl { display: grid; grid-template-columns: auto 1fr; gap: 6px 14px; margin: 0; }
  dt { color: var(--muted); }
  dd { margin: 0; overflow-wrap: anywhere; }
</style>
