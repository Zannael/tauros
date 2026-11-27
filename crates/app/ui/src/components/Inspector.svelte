<script>
  // Game state viewers beside the game: palettes, tiles (VRAM), sprites (OAM) and
  // memory with .sym symbols, with an optional 2 Hz refresh.
  import { onMount, untrack } from 'svelte';
  import { ipc } from '../lib/app.js';
  import { t } from '../lib/i18n.js';
  let tab = $state('palettes');
  let auto = $state(false);
  let status = $state({ text: '', error: false });
  let palettes = $state(null), selectedColor = $state('');
  let tile = $state({ base: 100663296, bpp: '4', bank: 'bg', pal: 0 });
  let tileCanvas = $state(null);
  let oam = $state([]), activeOnly = $state(true);
  let regions = $state([]), addr = $state('0x02000000'), deref = $state(false), memory = $state(null);
  let query = $state(''), symbols = $state([]);
  const BASES = [[100663296, '0x06000000 BG0'], [100679680, '0x06004000 BG1'], [100696064, '0x06008000 BG2'],
    [100712448, '0x0600C000 BG3'], [100728832, '0x06010000 OBJ0'], [100745216, '0x06014000 OBJ1']];
  const hex = (n, w = 8) => `0x${n.toString(16).padStart(w, '0').toUpperCase()}`;

  async function refresh() {
    try {
      if (tab === 'palettes') { palettes = await ipc.invoke('emu_inspect_palettes'); status = { text: $t('Palettes updated'), error: false }; }
      else if (tab === 'tiles') await refreshTiles();
      else if (tab === 'oam') { oam = await ipc.invoke('emu_inspect_oam'); status = { text: $t('{shown} of {total} sprites shown', { shown: shownOam().length, total: oam.length }), error: false }; }
      else if (tab === 'memory') await readMemory();
    } catch (err) { status = { text: String(err), error: true }; }
  }
  async function refreshTiles() {
    const sheet = await ipc.invoke('emu_inspect_tiles', { baseAddr: Number(tile.base), bpp8: tile.bpp === '8', paletteIdx: Number(tile.pal), isObj: tile.bank === 'obj' });
    tileCanvas.width = sheet.width; tileCanvas.height = sheet.height;
    const ctx = tileCanvas.getContext('2d'), img = ctx.createImageData(sheet.width, sheet.height);
    img.data.set(sheet.rgba); ctx.putImageData(img, 0, 0);
    status = { text: $t('{n} tiles', { n: sheet.tile_count }) + ` (${sheet.bpp8 ? '8bpp' : '4bpp'}, ${sheet.is_obj ? 'OBJ' : 'BG'} pal ${sheet.palette_idx})`, error: false };
  }
  const shownOam = () => oam.filter((e) => !activeOnly || e.obj_mode !== 'Hidden');
  async function readMemory() {
    const raw = addr.trim();
    const value = /^0x/i.test(raw) ? parseInt(raw.slice(2), 16) : parseInt(raw, 16);
    if (!Number.isFinite(value) || value < 0) { status = { text: $t('Enter a valid hex address (e.g. 0x02000000)'), error: true }; return; }
    memory = await ipc.invoke('emu_read_memory', { addr: value, len: 256, deref });
    status = { text: $t('Read {n} bytes at {addr}', { n: memory.bytes.length, addr: hex(memory.resolved_addr) }), error: false };
  }
  function dump(win) {
    const lines = [];
    for (let i = 0; i < win.bytes.length; i += 16) {
      const slice = win.bytes.slice(i, i + 16);
      const hexes = Array.from(slice, (b) => b.toString(16).padStart(2, '0').toUpperCase()).join(' ');
      const ascii = Array.from(slice, (b) => (b >= 32 && b < 127 ? String.fromCharCode(b) : '.')).join('');
      lines.push(`${(win.resolved_addr + i).toString(16).padStart(8, '0').toUpperCase()}  ${hexes.padEnd(47, ' ')}  ${ascii}`);
    }
    return lines.join('\n');
  }
  async function search() {
    try { symbols = await ipc.invoke('emu_search_symbols', { query, limit: 50 }); } catch { symbols = []; }
  }
  function go(address, ptr = false) { addr = hex(address); deref = ptr; tab = 'memory'; readMemory().catch((err) => (status = { text: String(err), error: true })); }

  onMount(() => {
    ipc.invoke('emu_memory_regions').then((r) => (regions = r)).catch(() => {});
    search();
  });
  $effect(() => { tab; tile.base; tile.bpp; tile.bank; tile.pal; untrack(refresh); });
  $effect(() => { if (!auto) return; const id = setInterval(refresh, 500); return () => clearInterval(id); });
</script>

<div class="inspector">
  <div class="row controls">
    <div class="seg" role="tablist" aria-label={$t('Viewer')}>
      {#each [['palettes', 'Palettes'], ['tiles', 'Tiles'], ['oam', 'Sprites'], ['memory', 'Memory']] as [id, label]}
        <button type="button" role="tab" aria-selected={tab === id} onclick={() => (tab = id)}>{$t(label)}</button>
      {/each}
    </div>
    <span class="grow"></span>
    <label class="check small"><input type="checkbox" bind:checked={auto} /> {$t('Live')}</label>
    <button type="button" class="ghost" onclick={refresh}>{$t('Refresh')}</button>
  </div>
  <p class="small status" class:error-text={status.error} role="status">{status.text}</p>

  {#if tab === 'palettes'}
    <p class="small mono sel">{selectedColor || $t('Select a colour to see its values.')}</p>
    {#each [['bg', 'BG palettes'], ['obj', 'OBJ palettes']] as [kind, label]}
      <h3 class="small">{$t(label)}</h3>
      <div class="pal">
        {#each palettes?.[kind] || [] as c (c.index)}
          {@const info = `${kind.toUpperCase()} ${Math.floor(c.index / 16)}[${c.index % 16}] · ${c.hex} · RGB(${c.r},${c.g},${c.b}) · ${hex(c.bgr555, 4)}`}
          <button type="button" class="sw" style:background={c.hex} title={info} aria-label={info} onclick={() => (selectedColor = info)}></button>
        {/each}
      </div>
    {/each}
  {:else if tab === 'tiles'}
    <div class="row wrap">
      <label class="field">{$t('Base')}<select bind:value={tile.base}>{#each BASES as [v, l]}<option value={v}>{l}</option>{/each}</select></label>
      <label class="field">BPP<select bind:value={tile.bpp}><option value="4">4bpp</option><option value="8">8bpp</option></select></label>
      <label class="field">{$t('Bank')}<select bind:value={tile.bank}><option value="bg">BG</option><option value="obj">OBJ</option></select></label>
      <label class="field">{$t('Palette')}<select bind:value={tile.pal}>{#each Array.from({ length: 16 }, (_, i) => i) as i}<option value={i}>{i}</option>{/each}</select></label>
    </div>
    <canvas bind:this={tileCanvas} class="tiles pixel checker" width="128" height="256"></canvas>
  {:else if tab === 'oam'}
    <label class="check small"><input type="checkbox" bind:checked={activeOnly} onchange={refresh} /> {$t('Hide disabled sprites')}</label>
    <div class="table">
      <table><thead><tr><th>#</th><th>{$t('Mode')}</th><th>{$t('Position')}</th><th>{$t('Size')}</th><th>Tile</th><th>Pal</th><th>Prio</th><th>Flags</th></tr></thead>
        <tbody>{#each shownOam() as e (e.id)}<tr><td>{e.id}</td><td>{e.obj_mode}</td><td>({e.x}, {e.y})</td><td>{e.width}×{e.height}</td>
          <td>{e.tile} ({e.bpp}bpp)</td><td>{e.palette}</td><td>{e.priority}</td>
          <td>{[e.mosaic && 'Mosaic', e.hflip && 'HFlip', e.vflip && 'VFlip', e.affine_idx != null && `Aff#${e.affine_idx}`].filter(Boolean).join(' ') || '—'}</td></tr>{/each}</tbody></table>
    </div>
  {:else if tab === 'memory'}
    <div class="row wrap">{#each regions as r (r.name)}<button type="button" class="small" title="{hex(r.start)} ({r.size} bytes)" onclick={() => go(r.start)}>{r.name}</button>{/each}</div>
    <form class="row" onsubmit={(e) => { e.preventDefault(); readMemory().catch((err) => (status = { text: String(err), error: true })); }}>
      <input class="mono grow" bind:value={addr} aria-label={$t('Address')} />
      <label class="check small"><input type="checkbox" bind:checked={deref} /> {$t('Follow pointer')}</label>
      <button type="submit">{$t('Read')}</button>
    </form>
    {#if memory}
      <p class="mono small accent">{memory.region_name}{memory.deref_ptr != null ? ` → ptr ${hex(memory.deref_ptr)}` : ''} · u8={memory.u8_val ?? '—'} · u16={memory.u16_val ?? '—'} · u32={memory.u32_val != null ? hex(memory.u32_val) : '—'}</p>
      <pre class="dump">{dump(memory)}</pre>
    {/if}
    <input type="search" bind:value={query} oninput={search} placeholder={$t('Search .sym symbols…')} aria-label={$t('Search .sym symbols…')} />
    <ul class="symbols">
      {#each symbols as s (s.name)}<li><button type="button" class="ghost mono" onclick={() => go(s.addr, s.name.endsWith('Ptr'))}>{s.name} <span class="muted">{hex(s.addr)}</span></button></li>
      {:else}<li class="muted small">{$t('No matching symbols (open a built project to load .sym).')}</li>{/each}
    </ul>
  {/if}
</div>

<style>
  .inspector { display: grid; grid-template-columns: minmax(0, 1fr); gap: 8px; align-content: start; }
  .controls { gap: 6px; flex-wrap: wrap; }
  .seg { display: inline-flex; border: 1px solid var(--border); border-radius: var(--radius-sm); overflow: hidden; }
  .seg button { border: 0; border-radius: 0; min-height: 28px; padding: 0 8px; font-size: 12px; }
  .seg button[aria-selected="true"] { background: var(--accent-soft); color: var(--accent-text); font-weight: 600; }
  .status { color: var(--muted); min-height: 18px; }
  .wrap { flex-wrap: wrap; }
  .sel { color: var(--text); min-height: 18px; overflow-wrap: anywhere; }
  .pal { display: grid; grid-template-columns: repeat(16, 1fr); gap: 2px; padding: 4px; background: var(--surface-2); border-radius: var(--radius-sm); }
  .sw { min-height: 0; aspect-ratio: 1; padding: 0; border-radius: 2px; border: 1px solid rgba(127, 127, 127, .35); }
  .tiles { width: 256px; max-width: 100%; border: 1px solid var(--border); border-radius: 4px; }
  .table { max-height: 360px; overflow: auto; border: 1px solid var(--border); border-radius: var(--radius-sm); }
  table { width: 100%; border-collapse: collapse; font-family: var(--mono); font-size: 11px; }
  th, td { padding: 4px 6px; border-bottom: 1px solid var(--border); text-align: left; white-space: nowrap; }
  th { position: sticky; top: 0; background: var(--surface-2); color: var(--muted); font-family: var(--font); }
  .accent { color: var(--accent-text); overflow-wrap: anywhere; }
  .dump { margin: 0; padding: 8px; background: var(--surface-2); border-radius: var(--radius-sm); font-size: 10.5px; max-height: 260px; overflow: auto; }
  .symbols { list-style: none; margin: 0; padding: 0; max-height: 220px; overflow-y: auto; }
  .symbols button { width: 100%; justify-content: space-between; font-size: 11px; min-height: 26px; }
</style>
