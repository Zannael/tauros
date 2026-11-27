<script>
  // The game beside any view, so playing a change never means leaving the editor.
  import Icon from './Icon.svelte';
  import Screen from './Screen.svelte';
  import { miniGame, navigate, emu } from '../lib/app.js';
  import { t } from '../lib/i18n.js';
  const { config, status } = emu;
</script>

<section class="mini card" aria-label={$t('Game')}>
  <header class="row">
    <Icon name="game" size={15} /><strong class="grow ellipsis">{$status.rom_name || $t('Game')}</strong>
    <button type="button" class="ghost icon" title={$status.paused ? $t('Resume') : $t('Pause')} aria-label={$status.paused ? $t('Resume') : $t('Pause')}
      disabled={!$status.loaded || $status.mode === 'agent_held'} onclick={emu.togglePause}>
      <Icon name={$status.paused ? 'play' : 'pause'} size={15} /></button>
    <button type="button" class="ghost icon" title={$t('Open the Emulator view')} aria-label={$t('Open the Emulator view')}
      onclick={() => { miniGame.set(false); navigate('emulator'); }}><Icon name="external" size={15} /></button>
    <button type="button" class="ghost icon" title={$t('Hide the game')} aria-label={$t('Hide the game')}
      onclick={() => miniGame.set(false)}><Icon name="x" size={15} /></button>
  </header>
  <div class="body"><Screen compact aspectLocked={$config.prefs.aspect_locked} /></div>
</section>

<style>
  .mini { position: absolute; right: 14px; bottom: 14px; z-index: 30; width: 500px; max-width: calc(100% - 28px);
    box-shadow: var(--shadow); display: flex; flex-direction: column; overflow: hidden; }
  header { padding: 4px 4px 4px 10px; border-bottom: 1px solid var(--border); font-size: 12.5px; }
  header button { min-height: 26px; width: 26px; }
  .body { height: 330px; display: flex; padding: 4px; background: var(--game-bg); }
</style>
