<script>
  // The game at full size with its controls in the view header, the boot point for
  // Preview and Build & play, and a side panel with slots, viewers, layers, log
  // and ROM details.
  import { onDestroy, untrack } from 'svelte';
  import Icon from '../components/Icon.svelte';
  import Popover from '../components/Popover.svelte';
  import MenuItem from '../components/MenuItem.svelte';
  import Screen from '../components/Screen.svelte';
  import Inspector from '../components/Inspector.svelte';
  import { emu, project, chat, viewActions } from '../lib/app.js';
  import { SPEED_ORDER, speedBadge } from '../lib/emu.js';
  import { pickFile, ACCEPT } from '../lib/files.js';
  import { emuLog } from '../lib/emulog.js';
  import { t } from '../lib/i18n.js';
  const { status, config, slots, busy } = emu;
  const { info, point, operation, live, suggestion } = project;
  const { busy: agentBusy } = chat;

  let panel = $state('slots');
  // The tools panel starts hidden when the game would otherwise shrink below 2×;
  // the header toggle overrides that choice.
  let chosenPanel = $state(null);
  let width = $state(1200);
  const showPanel = $derived(chosenPanel ?? width >= 1000);
  let layers = $state(null);
  let logLevel = $state('important');

  const s = $derived($status);
  const prefs = $derived($config.prefs);
  const held = $derived(s.mode === 'agent_held');
  const canControl = $derived(s.loaded && !held && !$busy);
  const speedName = (v) => (v === 0 ? $t('Unlimited') : v === 1 ? $t('Normal (1×)') : speedBadge(v));
  const SCALES = [['auto', 'Fit (largest integer)'], ['1x', '1× (240×160)'], ['2x', '2× (480×320)'], ['3x', '3× (720×480)'], ['4x', '4× (960×640)']];

  const pickAnd = (accept, fn) => async () => { const file = await pickFile(accept); if (file) fn(file); };

  $effect(() => {
    viewActions.set([
      { id: 'pause', primary: true, icon: s.paused ? 'play' : 'pause', label: s.paused ? $t('Resume') : $t('Pause'),
        hint: 'Ctrl+P', disabled: !s.loaded || held || $busy, run: emu.togglePause },
      { id: 'step', icon: 'step', iconOnly: true, label: $t('Advance one frame'), hint: 'Ctrl+N', title: $t('Advance one frame (Ctrl+N)'), disabled: !canControl, run: emu.step },
      { id: 'rewind', icon: 'rewind', label: `${s.rewind_count}`, title: $t('Rewind (Ctrl+B)'), hint: 'Ctrl+B',
        disabled: !canControl || s.rewind_count === 0 || !prefs.rewind_enabled, run: emu.rewind,
        commands: [{ id: 'rewind', icon: 'rewind', label: $t('Rewind'), hint: 'Ctrl+B', disabled: !canControl || s.rewind_count === 0, run: emu.rewind }],
        snippet: rewindButton },
      { id: 'speed', snippet: speedControl, commands: SPEED_ORDER.map((v) => ({ id: `speed-${v}`, icon: 'forward',
        label: $t('Speed: {speed}', { speed: speedName(v) }), disabled: !s.loaded || held, run: () => emu.setSpeed(v) })) },
      { id: 'reset', icon: 'reset', iconOnly: true, label: $t('Reset the console'), title: $t('Reset the console (Ctrl+R)'), hint: 'Ctrl+R', disabled: !canControl, run: emu.reset },
      { id: 'shot', icon: 'camera', iconOnly: true, label: $t('Take a screenshot'), title: $t('Take a screenshot (F12)'), hint: 'F12', disabled: !s.loaded, run: emu.screenshot },
      { id: 'scale', snippet: scaleControl, commands: SCALES.map(([v, l]) => ({ id: `scale-${v}`, icon: 'zoom', label: $t('Scale: {scale}', { scale: $t(l) }), run: () => emu.savePrefs({ scale: v }) })) },
      { id: 'audio', snippet: audioControl, commands: [{ id: 'mute', icon: prefs.muted ? 'volume' : 'mute', label: prefs.muted ? $t('Unmute the game') : $t('Mute the game'), run: () => emu.savePrefs({ muted: !prefs.muted }) }] },
      { id: 'panel', icon: 'panel', iconOnly: true, label: $t('Tools panel'), pressed: showPanel, run: () => (chosenPanel = !showPanel) },
      { id: 'open-rom', overflow: true, icon: 'folder', label: $t('Open ROM…'), hint: 'Ctrl+O', run: pickAnd(ACCEPT.rom, emu.loadRom) },
      { id: 'import-sav', overflow: true, separator: true, icon: 'upload', label: $t('Import save (.sav)…'), disabled: !s.loaded || held, run: pickAnd(ACCEPT.save, emu.importSave) },
      { id: 'export-sav', overflow: true, icon: 'download', label: $t('Export save (.sav)'), disabled: !s.loaded || held, run: emu.exportSave },
      { id: 'save-state', overflow: true, separator: true, icon: 'save', label: $t('Save state to file'), disabled: !s.loaded || held, run: emu.saveStateFile },
      { id: 'load-state', overflow: true, icon: 'folder', label: $t('Load state from file…'), disabled: !s.loaded || held, run: pickAnd(ACCEPT.state, emu.loadStateFile) },
    ]);
  });
  onDestroy(() => viewActions.set([]));

  async function loadLayers() { try { layers = await emu.layers(); } catch (err) { layers = { error: String(err), video: [], audio: [] }; } }
  async function toggleLayer(kind, item) {
    try { await emu.setLayer(kind, item.id, !item.enabled); item.enabled = !item.enabled; layers = { ...layers }; }
    catch (err) { layers = { ...layers, error: String(err) }; }
  }
  const loaded = $derived(s.loaded);
  $effect(() => {
    const current = panel, isLoaded = loaded;
    untrack(() => {
      if (current === 'layers' && isLoaded) loadLayers();
      if (current === 'log') emuLog.refresh(emu);
      if (current === 'slots') emu.refreshSlots();
    });
  });
  const logEntries = $derived($emuLog.filter((e) => logLevel === 'all' || !['INFO', 'DEBUG', 'STUB'].includes(e.level)));
  const bounds = $derived($point.map ? project.bounds($point.map) : null);
  const refused = $derived($info ? $info.maps.filter((m) => project.refusal(m)) : []);
  const pointOk = $derived(!!bounds && $point.x !== '' && $point.y !== '' && Number.isInteger(Number($point.x)) && Number.isInteger(Number($point.y))
    && Number($point.x) >= 0 && Number($point.y) >= 0 && Number($point.x) < bounds.width && Number($point.y) < bounds.height);
  $effect(() => { $point.map; untrack(() => project.refreshLive()); });
</script>

{#snippet rewindButton()}
  <button type="button" title={$t('Rewind (Ctrl+B)')} aria-label={$t('Rewind (Ctrl+B)')}
    disabled={!canControl || s.rewind_count === 0 || !prefs.rewind_enabled} onclick={emu.rewind}>
    <Icon name="rewind" /><span class="count">{prefs.rewind_enabled ? s.rewind_count : $t('off')}</span></button>
{/snippet}

{#snippet speedControl()}
  <Popover label="" title={$t('Speed {speed}: click for the next speed, right-click to choose (Shift+Tab toggles)', { speed: speedName(prefs.speed) })}
    disabled={!s.loaded || held} buttonClass={prefs.speed !== 1 ? 'on' : ''} onprimary={emu.cycleSpeed}>
    {#snippet trigger()}<Icon name="forward" /><span class="badge">{speedBadge(prefs.speed)}</span>{/snippet}
    {#snippet children({ close })}
      {#each SPEED_ORDER as v}
        <MenuItem label={speedName(v)} checked={prefs.speed === v} onclick={() => { close(); emu.setSpeed(v); }} />
      {/each}
    {/snippet}
  </Popover>
{/snippet}

{#snippet scaleControl()}
  <Popover icon="zoom" label="" title={$t('Scale')}>
    {#snippet children({ close })}
      {#each SCALES as [v, l]}<MenuItem label={$t(l)} checked={prefs.scale === v} onclick={() => { close(); emu.savePrefs({ scale: v }); }} />{/each}
      <hr />
      <MenuItem label={$t('Keep the 3:2 shape')} checked={prefs.aspect_locked} onclick={() => emu.savePrefs({ aspect_locked: !prefs.aspect_locked })} />
    {/snippet}
  </Popover>
{/snippet}

{#snippet audioControl()}
  <Popover icon={prefs.muted ? 'mute' : 'volume'} label="" title={$t('Sound')}>
    {#snippet children()}
      <MenuItem label={$t('Mute the game')} checked={prefs.muted} onclick={() => emu.savePrefs({ muted: !prefs.muted })} />
      <label class="volume small"><span>{$t('Volume')}</span>
        <input type="range" min="0" max="100" value={prefs.volume} aria-label={$t('Volume')}
          oninput={(e) => config.update((c) => ({ ...c, prefs: { ...c.prefs, volume: Number(e.currentTarget.value) } }))}
          onchange={(e) => emu.savePrefs({ volume: Number(e.currentTarget.value), muted: false })} />
        <span class="mono">{prefs.volume}%</span></label>
    {/snippet}
  </Popover>
{/snippet}

<div class="emulator" bind:clientWidth={width}>
  <section class="stage">
    {#if $info}
      <form class="boot card" onsubmit={(e) => e.preventDefault()} aria-label={$t('Boot point')}>
        <label class="field map"><span>{$t('Map')}</span>
          <select value={$point.map} disabled={!!$operation} title={bounds ? $t('{w} × {h} tiles; choose a walkable point', { w: bounds.width, h: bounds.height }) : ''}
            onchange={(e) => project.chooseMap(e.currentTarget.value)}>
            {#each $info.maps.filter((m) => !project.refusal(m)) as m (m)}<option value={m}>{m}</option>{/each}
            {#if refused.length}
              <optgroup label={$t('Not bootable in this build')}>
                {#each refused as m (m)}<option value={m} disabled title={project.refusal(m)}>{m}</option>{/each}
              </optgroup>
            {/if}
          </select></label>
        <label class="field coord"><span>X</span><input type="number" min="0" max={bounds ? bounds.width - 1 : 0} value={$point.x}
          disabled={!!$operation} oninput={(e) => project.setCoordinate('x', e.currentTarget.value)} /></label>
        <label class="field coord"><span>Y</span><input type="number" min="0" max={bounds ? bounds.height - 1 : 0} value={$point.y}
          disabled={!!$operation} oninput={(e) => project.setCoordinate('y', e.currentTarget.value)} /></label>
        <div class="row buttons">
          <button type="button" disabled={!!$operation || $agentBusy || !pointOk || !$info.built} onclick={() => project.boot(false)}
            title={$info.built ? $t('Start a fresh preview from the last built ROM without rebuilding') : $t('No build yet: use Build & play')}>
            {#if $operation === 'preview'}<span class="spin"></span>{:else}<Icon name="play" size={15} />{/if}{$t('Preview')}</button>
          <button type="button" class="primary" disabled={!!$operation || $agentBusy || !pointOk} onclick={() => project.boot(true)}
            title={$t('Build the project and boot it at this map and position')}>
            {#if $operation === 'build_and_play'}<span class="spin"></span>{:else}<Icon name="hammer" size={15} />{/if}{$t('Build & play')}</button>
        </div>
        {#if $live && ['ok', 'stale_rom', 'title_or_uninitialized'].includes($live.live_status)}
          <span class="live chip {$live.live_status === 'ok' ? 'ok' : $live.live_status === 'stale_rom' ? 'warn' : ''}">
            {#if $live.live_status === 'ok' && $live.live_map}{$t('Now at {map} ({x}, {y})', { map: $live.live_map, x: $live.live_x, y: $live.live_y })}
            {:else if $live.live_status === 'stale_rom'}{$t('The game is older than the last build')}
            {:else if $live.live_status === 'title_or_uninitialized'}{$t('Title screen / not in the world')}
            {:else}{$live.live_status.replace(/_/g, ' ')}{/if}</span>
        {/if}
        {#if !pointOk && $point.map}<p class="small error-text hint">{$t('Choose integer coordinates inside this map.')}</p>{/if}
        {#if $suggestion}
          <p class="small hint">{$t('This tile cannot be walked on.')}
            <button type="button" disabled={!!$operation} onclick={() => { const { x, y } = $suggestion; project.setCoordinate('x', x); project.setCoordinate('y', y); }}>
              {$t('Use the nearest walkable tile ({x}, {y})', $suggestion)}</button></p>
        {/if}
      </form>
      {#if $live?.new_build_pending}
        <div class="banner" role="status">
          <Icon name="info" size={16} />
          <span class="grow">{$t('A new build is ready ({sha}). Preview it at {map} ({x}, {y}).', { sha: ($live.loaded_rom_sha256 || '').slice(0, 8), map: $point.map, x: $point.x, y: $point.y })}</span>
          <button type="button" class="primary" disabled={!!$operation || $agentBusy || !pointOk} onclick={() => project.boot(false)}>{$t('Preview new build')}</button>
        </div>
      {/if}
    {/if}
    <Screen scale={prefs.scale} aspectLocked={prefs.aspect_locked} />
  </section>

  {#if showPanel}
    <aside class="side card" aria-label={$t('Tools panel')}>
      <div class="tabs" role="tablist">
        {#each [['slots', 'Slots'], ['inspect', 'Inspect'], ['layers', 'Layers'], ['log', 'Log'], ['rom', 'ROM']] as [id, label]}
          <button type="button" role="tab" aria-selected={panel === id} onclick={() => (panel = id)}>{$t(label)}</button>
        {/each}
      </div>
      <div class="side-body scroll">
        {#if panel === 'slots'}
          <p class="small muted">{$t('Quick savestates for this game.')}</p>
          <ul class="slots">
            {#each $slots as slot (slot.slot)}
              <li class="slot">
                <span class="n">{slot.slot}</span>
                <span class="grow" class:muted={!slot.available}>{slot.available ? $t('Saved') : $t('Empty')}</span>
                <button type="button" disabled={!s.loaded || $busy || held} onclick={() => emu.saveSlot(slot.slot)}>{$t('Save')}</button>
                <button type="button" disabled={!slot.available || !s.loaded || $busy || held} onclick={() => emu.loadSlot(slot.slot)}>{$t('Load')}</button>
              </li>
            {/each}
          </ul>
        {:else if panel === 'inspect'}
          {#if s.loaded}<Inspector />{:else}<div class="empty">{$t('Start a game to inspect it.')}</div>{/if}
        {:else if panel === 'layers'}
          {#if !s.loaded}<div class="empty">{$t('Start a game to toggle its layers.')}</div>
          {:else if layers}
            {#if layers.error}<p class="small error-text">{layers.error}</p>{/if}
            {#each [['video', 'Video layers'], ['audio', 'Audio channels']] as [kind, label]}
              <h3 class="small">{$t(label)}</h3>
              <div class="toggles">
                {#each layers[kind] || [] as item (item.id)}
                  <label class="check"><input type="checkbox" checked={item.enabled} onchange={() => toggleLayer(kind, item)} /> {item.name}</label>
                {:else}<span class="muted small">{$t('Unavailable')}</span>{/each}
              </div>
            {/each}
          {/if}
        {:else if panel === 'log'}
          <div class="row">
            <select bind:value={logLevel} aria-label={$t('Messages to show')}>
              <option value="important">{$t('Warnings and errors')}</option>
              <option value="all">{$t('All messages')}</option>
            </select>
            <span class="grow"></span>
            <button type="button" class="ghost" onclick={() => emuLog.refresh(emu)}>{$t('Refresh')}</button>
            <button type="button" class="ghost" onclick={() => emuLog.clear()}>{$t('Clear')}</button>
          </div>
          <ul class="log mono">
            {#each logEntries as e, i (i)}<li><span class="lvl {e.level}">{e.level}</span> {e.message}</li>
            {:else}<li class="muted">{$t('No messages at this level.')}</li>{/each}
          </ul>
        {:else if panel === 'rom'}
          {#if s.loaded}
            <dl class="rom">
              <dt>{$t('Loaded as')}</dt><dd>{s.rom_name || '—'}</dd>
              <dt>{$t('Header title')}</dt><dd>{s.rom_info?.title || '—'}</dd>
              <dt>{$t('Game code')}</dt><dd>{s.rom_info?.code || '—'}</dd>
              <dt>{$t('ROM size')}</dt><dd>{s.rom_info ? `${(s.rom_info.rom_size / 1048576).toFixed(2)} MiB (${s.rom_info.rom_size} bytes)` : '—'}</dd>
              <dt>{$t('Save memory')}</dt><dd>{s.rom_info?.save_size ? `${s.rom_info.save_size} bytes` : $t('None')}</dd>
              <dt>SHA-256</dt><dd class="mono">{s.rom_sha256 || '—'}</dd>
            </dl>
          {:else}<div class="empty">{$t('No game running')}</div>{/if}
        {/if}
      </div>
    </aside>
  {/if}
</div>

<style>
  .emulator { flex: 1; min-height: 0; display: flex; gap: 12px; padding: 12px; }
  .stage { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 10px; }
  .boot { display: flex; flex-wrap: wrap; align-items: flex-end; gap: 8px; padding: 8px 10px; }
  .boot .map { flex: 1; min-width: 180px; }
  .boot .coord input { width: 72px; }
  .buttons { gap: 6px; }
  .live { align-self: center; }
  .hint { width: 100%; }
  .banner { display: flex; align-items: center; gap: 10px; padding: 8px 10px; border-radius: var(--radius);
    background: var(--accent-soft); color: var(--accent-text); font-size: 12.5px; }
  .side { width: 360px; flex: none; display: flex; flex-direction: column; min-height: 0; }
  .side .tabs { padding: 0 4px; flex-wrap: nowrap; }
  .side .tabs button { padding: 0 8px; }
  .side-body { padding: 10px 12px; display: grid; grid-template-columns: minmax(0, 1fr); gap: 10px; align-content: start; flex: 1; }
  .side-body > :global(.row) { flex-wrap: wrap; }
  .slots { list-style: none; margin: 0; padding: 0; display: grid; gap: 4px; }
  .slot { display: flex; align-items: center; gap: 6px; padding: 4px 6px; border-radius: var(--radius-sm); background: var(--surface-2); }
  .slot .n { width: 22px; height: 22px; border-radius: 50%; display: grid; place-items: center; background: var(--surface); font-weight: 650; font-size: 12px; }
  .slot button { min-height: 26px; padding: 0 9px; }
  .toggles { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 6px; }
  .log { list-style: none; margin: 0; padding: 8px; background: var(--surface-2); border-radius: var(--radius-sm);
    font-size: 11px; display: grid; gap: 3px; max-height: 60vh; overflow-y: auto; overflow-wrap: anywhere; }
  .lvl { font-weight: 700; color: var(--muted); }
  .lvl.WARN, .lvl.GAME_ERROR { color: var(--warn); }
  .lvl.ERROR, .lvl.FATAL { color: var(--err); }
  .rom { display: grid; grid-template-columns: auto 1fr; gap: 6px 12px; margin: 0; font-size: 12.5px; }
  .rom dt { color: var(--muted); }
  .rom dd { margin: 0; overflow-wrap: anywhere; }
  .badge { font-weight: 700; font-size: 12px; min-width: 22px; }
  .count { font-variant-numeric: tabular-nums; min-width: 16px; }
  .volume { display: flex; align-items: center; gap: 8px; padding: 6px 8px; color: var(--muted); }
  .volume input { flex: 1; }
  hr { border: 0; border-top: 1px solid var(--border); margin: 4px 2px; }
  @media (max-width: 1360px) { .side { width: 310px; } }
</style>
