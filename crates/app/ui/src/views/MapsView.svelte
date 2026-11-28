<script>
  // Read-only map viewer (APP-001): the project's maps drawn from their tilesets,
  // with collision, elevation, events and the game's live position. Choosing a tile
  // sets the boot point; Preview and Build & play start there and the game plays
  // beside the view.
  import { onDestroy, onMount, untrack } from 'svelte';
  import { get } from 'svelte/store';
  import Icon from '../components/Icon.svelte';
  import Popover from '../components/Popover.svelte';
  import MenuItem from '../components/MenuItem.svelte';
  import { maps, project, emu, chat, viewActions, miniGame } from '../lib/app.js';
  import { TILE, ZOOMS, tileClass, eventLabel, kindIndex } from '../lib/maps.js';
  import { t } from '../lib/i18n.js';
  const { current, loading, error, tile, tileCheck, selectedEvent, settings } = maps;
  const { info, live, operation } = project;
  const { busy: agentBusy } = chat;
  const { status } = emu;

  const DIRECTIONS = { up: 'North', down: 'South', left: 'West', right: 'East', dive: 'Dive', emerge: 'Emerge' };
  const KINDS = { npc: ['N', 'NPC'], warp: ['W', 'Warp'], trigger: ['T', 'Trigger'], background: ['S', 'Sign or hidden item'] };
  let query = $state('');
  let panel = $state('tile');
  let hover = $state(null);
  let overlayCanvas = $state(null);
  // The list and the details panel give way to the map when the view is narrow;
  // the header toggles override that choice.
  let width = $state(1400);
  let chosenList = $state(null), chosenSide = $state(null);
  const showSide = $derived(chosenSide ?? width >= 700);
  const showList = $derived(chosenList ?? width >= 1100);

  const m = $derived($current);
  const s = $derived($settings);
  const px = $derived(TILE * s.zoom);
  const refusal = $derived(m ? m.refusal || project.refusal(m.id) : null);
  const liveTile = $derived(maps.liveOnMap($live));
  const hovered = $derived(hover && m ? maps.block(hover.x, hover.y) : null);
  const picked = $derived($tile && m ? maps.block($tile.x, $tile.y) : null);
  const event = $derived(m && $selectedEvent != null ? m.events[$selectedEvent] : null);
  const canPlay = $derived(!!m && !!$tile && !refusal && !$operation && !$agentBusy);

  // Groups of map_groups.json, in name order; maps outside every group last.
  const groups = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const by = new Map();
    for (const b of $info?.map_bounds || []) {
      if (q && !b.id.toLowerCase().includes(q) && !(b.name || '').toLowerCase().includes(q)) continue;
      const g = b.group ? b.group.replace(/^gMapGroup_/, '') : '';
      if (!by.has(g)) by.set(g, []);
      by.get(g).push(b);
    }
    return [...by.entries()].sort(([a], [b]) => (a === '' ? 1 : b === '' ? -1 : a.localeCompare(b)));
  });
  const shownCount = $derived(groups.reduce((n, [, list]) => n + list.length, 0));

  const hex = (n, width = 3) => `0x${Number(n).toString(16).toUpperCase().padStart(width, '0')}`;
  const kindText = (k) => ({ solid: $t('Solid'), water: $t('Deep water (needs Surf)'), walkable: $t('Walkable') })[k];
  const describe = (b) => `(${b.x}, ${b.y}) · ${$t('metatile')} ${hex(b.metatile)} · ${kindText(b.kind)} · ${$t('elevation')} ${b.elevation}`
    + (b.behaviorName ? ` · ${b.behaviorName}` : b.behavior != null ? ` · ${hex(b.behavior, 2)}` : '');
  const value = (v) => (v == null ? '—' : typeof v === 'object' ? JSON.stringify(v) : String(v));
  const fields = (e) => Object.entries(e).filter(([k]) => !['kind', 'position', 'extensions'].includes(k))
    .concat(Object.entries(e.extensions || {}));

  async function play(withBuild) {
    if (await maps.play(withBuild)) miniGame.set(true);
  }

  $effect(() => {
    viewActions.set([
      { id: 'zoom', snippet: zoomControl, commands: ZOOMS.map((z) => ({ id: `map-zoom-${z}`, icon: 'zoom', label: $t('Map zoom: {zoom}×', { zoom: z }), run: () => maps.setSetting('zoom', z) })) },
      { id: 'overlays', snippet: overlayControl, commands: [
        { id: 'map-collision', icon: 'layers', label: $t('Show collision'), run: () => maps.setSetting('overlay', s.overlay === 'collision' ? 'none' : 'collision') },
        { id: 'map-elevation', icon: 'layers', label: $t('Show elevation'), run: () => maps.setSetting('overlay', s.overlay === 'elevation' ? 'none' : 'elevation') },
        { id: 'map-events', icon: 'layers', label: $t('Show events'), run: () => maps.setSetting('events', !s.events) },
        { id: 'map-grid', icon: 'layers', label: $t('Show the grid'), run: () => maps.setSetting('grid', !s.grid) }] },
      { id: 'list', icon: 'panel', iconOnly: true, label: $t('Maps list'), pressed: showList, run: () => (chosenList = !showList) },
      { id: 'side', icon: 'info', iconOnly: true, label: $t('Details panel'), pressed: showSide, run: () => (chosenSide = !showSide) },
      { id: 'follow', icon: 'gamepad', label: $t('Follow the game'), pressed: s.follow,
        title: $t('Open the map the game is on whenever it changes'), run: () => maps.setFollow(!s.follow, $live) },
      { id: 'live', icon: 'map', iconOnly: true, label: $t("Show the game's map"), disabled: $live?.live_status !== 'ok' || !$live?.live_map,
        run: () => maps.showLive($live) },
      { id: 'preview', icon: 'play', label: $t('Preview here'), busy: $operation === 'preview',
        title: $info?.built ? $t('Start a fresh preview of the last build at the selected tile') : $t('No build yet: use Build & play'),
        disabled: !canPlay || !$info?.built, run: () => play(false) },
      { id: 'build-play', primary: true, icon: 'hammer', label: $t('Build & play here'), busy: $operation === 'build_and_play',
        title: $t('Build the project and boot it at the selected tile'), disabled: !canPlay, run: () => play(true) },
    ]);
  });
  onDestroy(() => viewActions.set([]));

  // The live position is read while the view is open and a game runs.
  let timer;
  onMount(() => {
    if (!get(current)) {
      const l = get(live);
      if (l?.live_status === 'ok' && l.live_map) maps.showLive(l);
      else maps.open(get(project.point).map);
    }
    timer = setInterval(() => { if (get(status).loaded) project.refreshLive(); }, 1000);
  });
  onDestroy(() => clearInterval(timer));
  $effect(() => { const l = $live; untrack(() => maps.onLive(l)); });

  // Collision or elevation as one pixel per block, scaled up without smoothing.
  $effect(() => {
    const canvas = overlayCanvas, map = m, mode = s.overlay;
    if (!canvas || !map || mode === 'none') return;
    const ctx = canvas.getContext('2d');
    const image = ctx.createImageData(map.width, map.height);
    for (let i = 0; i < map.width * map.height; i++) {
      let rgba = [0, 0, 0, 0];
      if (mode === 'collision') {
        const k = tileClass(map.collision[i], map.elevation[i]);
        rgba = k === 'solid' ? [220, 38, 52, 120] : k === 'water' ? [36, 110, 235, 110] : [0, 0, 0, 0];
      } else {
        const e = map.elevation[i];
        rgba = e === 0 || e === 15 ? [255, 255, 255, 60] : hsl(e * 47 % 360, 120);
      }
      image.data.set(rgba, i * 4);
    }
    ctx.putImageData(image, 0, 0);
  });
  function hsl(h, alpha) {
    const f = (n) => { const k = (n + h / 30) % 12; return Math.round(255 * (0.5 - 0.45 * Math.max(-1, Math.min(k - 3, 9 - k, 1)))); };
    return [f(0), f(8), f(4), alpha];
  }

  // Keeps the open map visible in the list, and the player while following.
  function reveal(node, active) {
    const show = (on) => { if (on) node.scrollIntoView({ block: 'nearest', inline: 'nearest' }); };
    show(active);
    return { update: show };
  }

  function at(e) {
    const r = e.currentTarget.getBoundingClientRect();
    const x = Math.floor((e.clientX - r.left) / px), y = Math.floor((e.clientY - r.top) / px);
    return m && x >= 0 && y >= 0 && x < m.width && y < m.height ? { x, y } : null;
  }
</script>

{#snippet zoomControl()}
  <Popover icon="zoom" label="" title={$t('Map zoom: {zoom}×', { zoom: s.zoom })}>
    {#snippet children({ close })}
      {#each ZOOMS as z}<MenuItem label={`${z}×`} checked={s.zoom === z} onclick={() => { close(); maps.setSetting('zoom', z); }} />{/each}
    {/snippet}
  </Popover>
{/snippet}

{#snippet overlayControl()}
  <Popover icon="layers" label="" title={$t('Overlays')}>
    {#snippet children()}
      <MenuItem label={$t('Collision')} checked={s.overlay === 'collision'} onclick={() => maps.setSetting('overlay', s.overlay === 'collision' ? 'none' : 'collision')} />
      <MenuItem label={$t('Elevation')} checked={s.overlay === 'elevation'} onclick={() => maps.setSetting('overlay', s.overlay === 'elevation' ? 'none' : 'elevation')} />
      <hr />
      <MenuItem label={$t('Events')} checked={s.events} onclick={() => maps.setSetting('events', !s.events)} />
      <MenuItem label={$t('Grid')} checked={s.grid} onclick={() => maps.setSetting('grid', !s.grid)} />
    {/snippet}
  </Popover>
{/snippet}

{#if !$info}
  <div class="empty"><Icon name="map" size={28} />{$t('Open a game project to view its maps.')}</div>
{:else}
<div class="maps" bind:clientWidth={width}>
  {#if showList}
  <aside class="list card" aria-label={$t('Maps')}>
    <input type="search" placeholder={$t('Search maps')} aria-label={$t('Search maps')} bind:value={query} />
    <p class="small muted">{$t('{n} maps', { n: shownCount })}</p>
    <div class="scroll groups">
      {#each groups as [group, list] (group)}
        <h3 class="small">{group || $t('In no group')}</h3>
        <ul>
          {#each list as b (b.id)}
            <li><button type="button" class="ghost item" use:reveal={m?.id === b.id} aria-current={m?.id === b.id ? 'true' : undefined}
              title={b.refusal ? $t('Not bootable in this build: {reason}', { reason: b.refusal }) : `${b.id} · ${b.width} × ${b.height}`}
              onclick={() => maps.open(b.id)}>
              <span class="ellipsis grow" class:muted={!!b.refusal}>{b.name || b.id}</span>
              {#if $live?.live_status === 'ok' && $live.live_map === b.id}<span class="dot" title={$t('The game is here')}></span>{/if}
            </button></li>
          {/each}
        </ul>
      {:else}<p class="small muted">{$t('No map matches.')}</p>{/each}
    </div>
  </aside>
  {/if}

  <section class="stage card" aria-label={$t('Map')}>
    <div class="bar row small">
      {#if m}
        <strong class="mono">{m.id}</strong>
        <span class="muted">{m.width} × {m.height} · {m.primary_tileset} + {m.secondary_tileset}</span>
        {#if refusal}<span class="chip warn" title={refusal}>{$t('Not bootable')}</span>{/if}
        {#if m.problems.length}<span class="chip err" title={m.problems.join('\n')}>{$t('{n} drawing problems', { n: m.problems.length })}</span>{/if}
      {/if}
      <span class="grow"></span>
      {#if $loading}<span class="spin"></span><span class="muted">{$t('Loading {map}…', { map: $loading })}</span>{/if}
    </div>
    {#if $error}
      <p class="small error-text err-line">{$t('Could not open {map}: {cause}', { map: $error.map, cause: $error.message })}
        <button type="button" onclick={() => maps.open($error.map)}>{$t('Retry')}</button></p>
    {/if}
    <div class="scroll canvas">
      {#if m}
        <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
        <div class="mapbox" style:width="{m.width * px}px" style:height="{m.height * px}px"
          onmousemove={(e) => (hover = at(e))} onmouseleave={() => (hover = null)}
          onclick={(e) => { const p = at(e); if (p) { maps.selectTile(p.x, p.y); panel = 'tile'; } }}>
          <img src={m.image} alt={$t('{map} as drawn from its tilesets', { map: m.id })} width={m.width * px} height={m.height * px} draggable="false" />
          {#if s.overlay !== 'none'}<canvas class="overlay" bind:this={overlayCanvas} width={m.width} height={m.height}></canvas>{/if}
          {#if s.grid}<div class="grid" style:background-size="{px}px {px}px"></div>{/if}
          {#if s.events}
            {#each m.events as e, i (i)}
              <button type="button" class="event {e.kind}" class:selected={$selectedEvent === i}
                style:left="{e.position.x * px}px" style:top="{e.position.y * px}px" style:width="{px}px" style:height="{px}px"
                title="{$t(KINDS[e.kind][1])}: {eventLabel(e, kindIndex(m.events, i))}{e.kind === 'warp' ? ` (${$t('double-click to open')})` : ''}"
                aria-label="{$t(KINDS[e.kind][1])}: {eventLabel(e, kindIndex(m.events, i))}"
                onclick={(ev) => { ev.stopPropagation(); maps.selectEvent(i); panel = 'events'; }}
                ondblclick={(ev) => { ev.stopPropagation(); maps.followWarp(e); }}>{#if s.zoom > 1}{KINDS[e.kind][0]}{/if}</button>
            {/each}
          {/if}
          {#if hover}<div class="box hover" style:left="{hover.x * px}px" style:top="{hover.y * px}px" style:width="{px}px" style:height="{px}px"></div>{/if}
          {#if $tile}<div class="box picked" style:left="{$tile.x * px}px" style:top="{$tile.y * px}px" style:width="{px}px" style:height="{px}px"></div>{/if}
          {#if liveTile}<div class="player" use:reveal={s.follow ? `${m.id}:${liveTile.x},${liveTile.y}` : false} title={$t('The game is here ({x}, {y})', liveTile)}
            style:left="{liveTile.x * px}px" style:top="{liveTile.y * px}px" style:width="{px}px" style:height="{px}px"></div>{/if}
        </div>
      {:else if !$loading}
        <div class="empty">{$t('Choose a map from the list.')}</div>
      {/if}
    </div>
    <div class="foot small mono muted">{hovered ? describe(hovered) : m ? $t('Point at a tile to read it; click to choose it.') : ''}</div>
  </section>

  {#if showSide}
  <aside class="side card" aria-label={$t('Details')}>
    <div class="tabs" role="tablist">
      {#each [['tile', 'Tile'], ['events', 'Events'], ['map', 'Map']] as [id, label]}
        <button type="button" role="tab" aria-selected={panel === id} onclick={() => (panel = id)}>{$t(label)}</button>
      {/each}
    </div>
    <div class="side-body scroll">
      {#if !m}<div class="empty">{$t('No map open')}</div>
      {:else if panel === 'tile'}
        {#if picked}
          <dl class="props">
            <dt>{$t('Position')}</dt><dd class="mono">({picked.x}, {picked.y})</dd>
            <dt>{$t('Metatile')}</dt><dd class="mono">{hex(picked.metatile)} ({picked.metatile})</dd>
            <dt>{$t('Collision')}</dt><dd>{picked.collision} · {kindText(picked.kind)}</dd>
            <dt>{$t('Elevation')}</dt><dd>{picked.elevation}</dd>
            <dt>{$t('Behaviour')}</dt><dd class="mono">{picked.behaviorName || (picked.behavior == null ? '—' : hex(picked.behavior, 2))}</dd>
          </dl>
          {#if refusal}<p class="small warn-text">{$t('This map cannot be booted: {reason}', { reason: refusal })}</p>
          {:else if $tileCheck?.ok}<p class="small ok-text">{$t('Boot point set: the player can start here.')}</p>
          {:else if $tileCheck}
            <p class="small warn-text">{$t('Boot point set, but the player cannot start here: {problem}.', { problem: $tileCheck.problem })}</p>
            {#if $tileCheck.nearest}<button type="button" onclick={() => maps.selectTile($tileCheck.nearest[0], $tileCheck.nearest[1])}>
              {$t('Use the nearest walkable tile ({x}, {y})', { x: $tileCheck.nearest[0], y: $tileCheck.nearest[1] })}</button>{/if}
          {/if}
          {#each maps.eventsAt(picked.x, picked.y) as [e, i] (i)}
            <button type="button" class="ghost row-item" onclick={() => { maps.selectEvent(i); panel = 'events'; }}>
              <span class="badge-kind {e.kind}">{KINDS[e.kind][0]}</span><span class="ellipsis">{eventLabel(e, kindIndex(m.events, i))}</span></button>
          {/each}
        {:else}<p class="small muted">{$t('Click a tile to read it and make it the boot point.')}</p>{/if}
      {:else if panel === 'events'}
        {#if event}
          <div class="row event-head"><span class="badge-kind {event.kind}">{KINDS[event.kind][0]}</span>
            <strong class="grow ellipsis">{$t(KINDS[event.kind][1])} #{kindIndex(m.events, $selectedEvent)}</strong>
            {#if event.kind === 'warp'}<button type="button" onclick={() => maps.followWarp(event)}>{$t('Open destination')}</button>{/if}</div>
          <dl class="props">
            <dt>{$t('Position')}</dt><dd class="mono">({event.position.x}, {event.position.y}){event.position.elevation != null ? ` · ${$t('elevation')} ${event.position.elevation}` : ''}</dd>
            {#each fields(event) as [k, v] (k)}<dt>{k}</dt><dd class="mono">{value(v)}</dd>{/each}
          </dl>
          <hr />
        {/if}
        <ul class="events">
          {#each m.events as e, i (i)}
            <li><button type="button" class="ghost row-item" aria-current={$selectedEvent === i ? 'true' : undefined} onclick={() => maps.selectEvent(i)}>
              <span class="badge-kind {e.kind}">{KINDS[e.kind][0]}</span><span class="ellipsis grow">{eventLabel(e, kindIndex(m.events, i))}</span>
              <span class="mono muted">{e.position.x},{e.position.y}</span></button></li>
          {:else}<li class="small muted">{$t('This map has no events.')}</li>{/each}
        </ul>
      {:else}
        <dl class="props">
          <dt>{$t('Name')}</dt><dd>{m.name}</dd>
          <dt>{$t('Layout')}</dt><dd class="mono">{m.layout}</dd>
          <dt>{$t('Size')}</dt><dd>{m.width} × {m.height}</dd>
          <dt>{$t('Tilesets')}</dt><dd class="mono">{m.primary_tileset}<br />{m.secondary_tileset}</dd>
          {#each Object.entries(m.properties || {}) as [k, v] (k)}<dt>{k}</dt><dd class="mono">{value(v)}</dd>{/each}
        </dl>
        <h3 class="small">{$t('Connections')}</h3>
        {#each m.connections as c, i (i)}
          <button type="button" class="ghost row-item" onclick={() => maps.open(c.map)}>
            <span class="grow ellipsis">{$t(DIRECTIONS[c.direction] || c.direction)} · {c.map}</span><span class="mono muted">{c.offset}</span></button>
        {:else}<p class="small muted">{$t('None')}</p>{/each}
        {#if refusal}<p class="small warn-text">{$t('This map cannot be booted: {reason}', { reason: refusal })}</p>{/if}
        {#if m.problems.length}
          <h3 class="small">{$t('Drawing problems')}</h3>
          <ul class="small problems">{#each m.problems as p}<li>{p}</li>{/each}</ul>
        {/if}
      {/if}
    </div>
  </aside>
  {/if}
</div>
{/if}

<style>
  .maps { flex: 1; min-height: 0; display: flex; gap: 12px; padding: 12px; }
  .list { width: 230px; flex: none; display: flex; flex-direction: column; gap: 6px; padding: 8px; min-height: 0; }
  .list p { margin: 0 2px; }
  .groups { flex: 1; }
  .groups h3 { margin: 8px 4px 2px; color: var(--muted); }
  .groups ul, .events { list-style: none; margin: 0; padding: 0; display: grid; gap: 1px; }
  .item, .row-item { width: 100%; min-width: 0; justify-content: flex-start; gap: 6px; min-height: 28px; padding: 0 8px; font-weight: 450; text-align: left; }
  .row-item .mono { flex: none; }
  .row-item .ellipsis, .item .ellipsis { min-width: 0; }
  .events, .groups ul { grid-template-columns: minmax(0, 1fr); }
  .event-head { flex-wrap: wrap; gap: 6px; min-width: 0; }
  .event-head strong { min-width: 0; }
  .item[aria-current="true"], .row-item[aria-current="true"] { background: var(--accent-soft); color: var(--accent-text); font-weight: 600; }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--ok); flex: none; }
  .stage { flex: 1; min-width: 0; display: flex; flex-direction: column; min-height: 0; }
  .bar { padding: 6px 10px; gap: 8px; border-bottom: 1px solid var(--border); flex-wrap: nowrap; min-height: 36px; white-space: nowrap; overflow: hidden; }
  .bar > * { flex: none; }
  .bar .muted { flex: 0 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .err-line { margin: 6px 10px; display: flex; gap: 8px; align-items: center; }
  .canvas { flex: 1; background: var(--game-bg); padding: 12px; }
  .mapbox { position: relative; cursor: crosshair; margin: auto; }
  .mapbox img, .overlay, .grid { position: absolute; inset: 0; width: 100%; height: 100%; image-rendering: pixelated; user-select: none; }
  .overlay { pointer-events: none; }
  .grid { pointer-events: none; background-image: linear-gradient(to right, rgba(255,255,255,.22) 1px, transparent 1px),
    linear-gradient(to bottom, rgba(255,255,255,.22) 1px, transparent 1px); }
  .box { position: absolute; pointer-events: none; box-sizing: border-box; }
  .box.hover { border: 1px solid rgba(255, 255, 255, .9); }
  .box.picked { border: 2px solid #ffd84a; box-shadow: 0 0 0 1px #000; }
  .player { position: absolute; pointer-events: none; box-sizing: border-box; border-radius: 50%; border: 3px solid #2ee37a;
    box-shadow: 0 0 0 1px #000, 0 0 8px #2ee37a; }
  .event { position: absolute; min-height: 0; padding: 0; border-radius: 3px; border: 1px solid #000; color: #fff;
    font: 700 11px/1 var(--font); opacity: .9; display: grid; place-items: center; box-sizing: border-box; }
  .event:hover { opacity: 1; }
  .event.selected { outline: 2px solid #ffd84a; outline-offset: 1px; opacity: 1; }
  .event.npc, .badge-kind.npc { background: #6a46dc; }
  .event.warp, .badge-kind.warp { background: #c0262f; }
  .event.trigger, .badge-kind.trigger { background: #17733a; }
  .event.background, .badge-kind.background { background: #855400; }
  .badge-kind { flex: none; width: 18px; height: 18px; border-radius: 4px; color: #fff; font-size: 11px; font-weight: 700; display: grid; place-items: center; }
  .foot { padding: 5px 10px; border-top: 1px solid var(--border); min-height: 26px; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .side { width: 300px; flex: none; display: flex; flex-direction: column; min-height: 0; }
  .side-body { padding: 10px 12px; display: grid; grid-template-columns: minmax(0, 1fr); gap: 8px; align-content: start; flex: 1; overflow-x: hidden; }
  .props { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: 5px 12px; margin: 0; font-size: 12.5px; }
  .props dt { color: var(--muted); }
  .props dd { margin: 0; overflow-wrap: anywhere; }
  .problems { margin: 0; padding-left: 18px; overflow-wrap: anywhere; }
  h3 { margin: 6px 0 0; }
  hr { border: 0; border-top: 1px solid var(--border); margin: 4px 0; width: 100%; }
  p { margin: 0; }
  @media (max-width: 1360px) { .side { width: 260px; } .list { width: 200px; } }
</style>
