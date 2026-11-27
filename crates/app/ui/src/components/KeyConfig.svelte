<script>
  // Game key bindings, saved as soon as a key is captured. While capturing, the
  // app stops reading game keys so the pressed key does not reach the game.
  import { onDestroy } from 'svelte';
  import { emu, ipc } from '../lib/app.js';
  import { BUTTON_ORDER, BUTTON_LABELS, DEFAULT_KEYMAP, normalizeWebKey, displayKey } from '../lib/keys.js';
  import { t } from '../lib/i18n.js';
  const { config } = emu;
  let capturing = $state(null);
  let error = $state('');

  function start(button) {
    capturing = button; error = '';
    ipc.invoke('set_typing', { typing: true }).catch(() => {});
  }
  function stop() {
    capturing = null;
    ipc.invoke('set_typing', { typing: false }).catch(() => {});
  }
  async function save(keymap) {
    try { await emu.setKeymap(keymap); error = ''; } catch (err) { error = String(err); }
  }
  function onKey(e) {
    if (!capturing) return;
    e.preventDefault(); e.stopPropagation();
    if (e.key === 'Escape') { stop(); return; }
    const norm = normalizeWebKey(e);
    if (!norm) { error = $t('Unsupported key: {key}', { key: e.key }); return; }
    const button = capturing;
    stop();
    save({ ...$config.keymap, [button]: norm });
  }
  onDestroy(() => { if (capturing) stop(); });
</script>

<svelte:window onkeydowncapture={onKey} />
<div class="keys">
  {#each BUTTON_ORDER as button}
    <div class="key">
      <span>{$t(BUTTON_LABELS[button])}</span>
      <button type="button" class="mono" aria-pressed={String(capturing === button)} onclick={() => (capturing === button ? stop() : start(button))}>
        {capturing === button ? $t('Press a key…') : displayKey($config.keymap[button])}</button>
    </div>
  {/each}
</div>
{#if error}<p class="small error-text" role="alert">{error}</p>{/if}
<div class="row"><button type="button" onclick={() => save({ ...DEFAULT_KEYMAP })}>{$t('Restore default keys')}</button>
  <span class="small muted">{$t('Click a button, then press the new key. Escape cancels.')}</span></div>

<style>
  .keys { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 6px 18px; max-width: 560px; }
  .key { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .key button { min-width: 110px; }
</style>
