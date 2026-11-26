// APP-003 Block 2: Video/Audio layer toggles and Game State Viewers
// (Palette Viewer, Tile/VRAM Viewer, Sprite/OAM Viewer, Memory & Symbol Inspector).
(() => {
  const { invoke } = window.__TAURI__.core;

  const $ = (id) => document.getElementById(id);
  const modal = $('emu-inspector-modal');
  const videoMenu = $('menu-video-layers');
  const audioMenu = $('menu-audio-channels');
  const autoCheck = $('insp-auto');
  const refreshBtn = $('insp-refresh');
  const closeBtn = $('insp-close');
  const statusEl = $('insp-status');

  let activeTab = 'palettes';
  let autoTimer = null;

  function setStatus(msg, isError = false) {
    statusEl.textContent = msg;
    statusEl.classList.toggle('error', isError);
  }

  async function refreshLayers() {
    if (!videoMenu || !audioMenu) return;
    try {
      const data = await invoke('emu_layers');
      const renderList = (container, items, cmd) => {
        container.textContent = '';
        if (!items.length) {
          const span = document.createElement('span');
          span.className = 'menu-note';
          span.textContent = 'Unavailable';
          container.appendChild(span);
          return;
        }
        for (const item of items) {
          const btn = document.createElement('button');
          btn.type = 'button';
          btn.role = 'menuitemcheckbox';
          btn.className = 'menu-entry';
          btn.setAttribute('aria-checked', String(item.enabled));
          btn.textContent = item.name;
          btn.addEventListener('click', async (e) => {
            e.stopPropagation();
            const next = btn.getAttribute('aria-checked') !== 'true';
            try {
              await invoke(cmd, { id: item.id, enabled: next });
              btn.setAttribute('aria-checked', String(next));
            } catch (err) {
              window.emuControls?.showError(`Layer toggle failed: ${err}`);
            }
          });
          container.appendChild(btn);
        }
      };
      renderList(videoMenu, data.video || [], 'emu_set_video_layer');
      renderList(audioMenu, data.audio || [], 'emu_set_audio_channel');
    } catch (_) {
      // Ignore when core is not ready.
    }
  }

  function switchTab(tab) {
    activeTab = tab;
    for (const btn of modal.querySelectorAll('.insp-tab')) {
      btn.setAttribute('aria-selected', String(btn.dataset.tab === tab));
    }
    for (const pane of modal.querySelectorAll('.insp-pane')) {
      pane.hidden = pane.dataset.pane !== tab;
    }
    refreshActiveTab();
  }

  function renderPaletteGrid(container, list, kind) {
    container.textContent = '';
    for (const c of list) {
      const sw = document.createElement('button');
      sw.type = 'button';
      sw.className = 'pal-swatch';
      sw.style.background = c.hex;
      const bank = Math.floor(c.index / 16);
      const idx = c.index % 16;
      sw.title = `${kind} Pal ${bank}[${idx}] · ${c.hex} · RGB(${c.r},${c.g},${c.b}) · 0x${c.bgr555.toString(16).padStart(4, '0').toUpperCase()}`;
      sw.addEventListener('click', () => {
        $('pal-selected').textContent = sw.title;
      });
      container.appendChild(sw);
    }
  }

  async function refreshPalettes() {
    const dump = await invoke('emu_inspect_palettes');
    renderPaletteGrid($('pal-bg-grid'), dump.bg, 'BG');
    renderPaletteGrid($('pal-obj-grid'), dump.obj, 'OBJ');
    setStatus('Palettes updated');
  }

  async function refreshTiles() {
    const baseAddr = Number($('tile-base').value);
    const bpp8 = $('tile-bpp').value === '8';
    const paletteIdx = Number($('tile-pal').value);
    const isObj = $('tile-bank').value === 'obj';
    const sheet = await invoke('emu_inspect_tiles', { baseAddr, bpp8, paletteIdx, isObj });
    const canvas = $('tile-canvas');
    canvas.width = sheet.width;
    canvas.height = sheet.height;
    const ctx = canvas.getContext('2d');
    const img = ctx.createImageData(sheet.width, sheet.height);
    img.data.set(sheet.rgba);
    ctx.putImageData(img, 0, 0);
    setStatus(`${sheet.tile_count} tiles (${sheet.bpp8 ? '8bpp' : '4bpp'}, ${sheet.is_obj ? 'OBJ' : 'BG'} pal ${sheet.palette_idx})`);
  }

  async function refreshOam() {
    const entries = await invoke('emu_inspect_oam');
    const activeOnly = $('oam-active-only').checked;
    const tbody = $('oam-tbody');
    tbody.textContent = '';
    let shown = 0;
    for (const e of entries) {
      if (activeOnly && e.obj_mode === 'Hidden') continue;
      shown++;
      const tr = document.createElement('tr');
      const flags = [
        e.mosaic ? 'Mosaic' : '',
        e.hflip ? 'HFlip' : '',
        e.vflip ? 'VFlip' : '',
        e.affine_idx !== null && e.affine_idx !== undefined ? `Aff#${e.affine_idx}` : '',
      ].filter(Boolean).join(' ') || '—';
      const cells = [
        String(e.id),
        e.obj_mode,
        `(${e.x}, ${e.y})`,
        `${e.width}×${e.height}`,
        `${e.tile} (${e.bpp}bpp)`,
        String(e.palette),
        String(e.priority),
        flags,
      ];
      for (const text of cells) {
        const td = document.createElement('td');
        td.textContent = text;
        tr.appendChild(td);
      }
      tbody.appendChild(tr);
    }
    setStatus(`OAM: ${shown} / ${entries.length} sprites shown`);
  }

  async function loadMemoryRegions() {
    const regions = await invoke('emu_memory_regions');
    const bar = $('mem-regions');
    bar.textContent = '';
    for (const r of regions) {
      const btn = document.createElement('button');
      btn.type = 'button';
      btn.textContent = r.name;
      btn.title = `0x${r.start.toString(16).padStart(8, '0').toUpperCase()} (${r.size} bytes)`;
      btn.addEventListener('click', () => {
        $('mem-addr').value = `0x${r.start.toString(16).padStart(8, '0')}`;
        $('mem-deref').checked = false;
        refreshMemory();
      });
      bar.appendChild(btn);
    }
  }

  function formatHexDump(startAddr, bytes) {
    const lines = [];
    for (let i = 0; i < bytes.length; i += 16) {
      const addr = (startAddr + i).toString(16).padStart(8, '0').toUpperCase();
      const slice = bytes.slice(i, i + 16);
      const hex = Array.from(slice, (b) => b.toString(16).padStart(2, '0').toUpperCase()).join(' ');
      const ascii = Array.from(slice, (b) => (b >= 32 && b < 127 ? String.fromCharCode(b) : '.')).join('');
      lines.push(`${addr}  ${hex.padEnd(47, ' ')}  ${ascii}`);
    }
    return lines.join('\n');
  }

  async function refreshMemory() {
    const raw = $('mem-addr').value.trim();
    const addr = raw.startsWith('0x') || raw.startsWith('0X') ? parseInt(raw.slice(2), 16) : parseInt(raw, 16);
    if (!Number.isFinite(addr) || addr < 0) {
      setStatus('Enter a valid hex address (e.g. 0x02000000)', true);
      return;
    }
    const deref = $('mem-deref').checked;
    const win = await invoke('emu_read_memory', { addr, len: 256, deref });
    const ptrText = win.deref_ptr !== null && win.deref_ptr !== undefined
      ? ` -> ptr 0x${win.deref_ptr.toString(16).padStart(8, '0').toUpperCase()}`
      : '';
    $('mem-decoded').textContent =
      `${win.region_name}${ptrText} · u8=${win.u8_val ?? '—'} · u16=${win.u16_val ?? '—'} · u32=${win.u32_val !== null && win.u32_val !== undefined ? '0x' + win.u32_val.toString(16).padStart(8, '0').toUpperCase() : '—'}`;
    $('mem-dump').textContent = formatHexDump(win.resolved_addr, win.bytes);
    setStatus(`Read ${win.bytes.length} bytes at 0x${win.resolved_addr.toString(16).padStart(8, '0').toUpperCase()}`);
  }

  async function searchSymbols() {
    const query = $('sym-search').value;
    const matches = await invoke('emu_search_symbols', { query, limit: 50 });
    const list = $('sym-results');
    list.textContent = '';
    if (!matches.length) {
      const li = document.createElement('li');
      li.className = 'muted';
      li.textContent = 'No matching symbols (open a built project to load .sym).';
      list.appendChild(li);
      return;
    }
    for (const m of matches) {
      const li = document.createElement('li');
      const btn = document.createElement('button');
      btn.type = 'button';
      btn.className = 'sym-row';
      const hex = `0x${m.addr.toString(16).padStart(8, '0').toUpperCase()}`;
      btn.textContent = `${m.name}  (${hex})`;
      btn.addEventListener('click', () => {
        $('mem-addr').value = hex;
        $('mem-deref').checked = m.name.endsWith('Ptr');
        refreshMemory();
      });
      li.appendChild(btn);
      list.appendChild(li);
    }
  }

  async function refreshActiveTab() {
    if (modal.hidden) return;
    try {
      if (activeTab === 'palettes') await refreshPalettes();
      else if (activeTab === 'tiles') await refreshTiles();
      else if (activeTab === 'oam') await refreshOam();
      else if (activeTab === 'memory') await refreshMemory();
    } catch (err) {
      setStatus(String(err), true);
    }
  }

  function stopAuto() {
    if (autoTimer) {
      clearInterval(autoTimer);
      autoTimer = null;
    }
  }

  function syncAuto() {
    stopAuto();
    if (!modal.hidden && autoCheck.checked) {
      autoTimer = setInterval(refreshActiveTab, 500);
    }
  }

  function open(tab = 'palettes') {
    modal.hidden = false;
    invoke('set_typing', { typing: true });
    loadMemoryRegions().catch(() => {});
    searchSymbols().catch(() => {});
    switchTab(tab);
    syncAuto();
  }

  function close() {
    stopAuto();
    modal.hidden = true;
    invoke('set_typing', { typing: false });
    $('screen').focus();
  }

  for (const btn of modal.querySelectorAll('.insp-tab')) {
    btn.addEventListener('click', () => switchTab(btn.dataset.tab));
  }
  refreshBtn.addEventListener('click', refreshActiveTab);
  closeBtn.addEventListener('click', close);
  autoCheck.addEventListener('change', syncAuto);
  for (const id of ['tile-base', 'tile-bpp', 'tile-pal', 'tile-bank']) {
    $(id).addEventListener('change', refreshActiveTab);
  }
  $('oam-active-only').addEventListener('change', refreshActiveTab);
  $('mem-read').addEventListener('click', refreshActiveTab);
  $('mem-deref').addEventListener('change', refreshActiveTab);
  $('sym-search').addEventListener('input', () => searchSymbols().catch(() => {}));

  // Populate Audio / Video layers when the AV menu is opened.
  const avMenuBtn = document.querySelector('.menu-item[data-menu="av"] > .menu-trigger');
  if (avMenuBtn) {
    avMenuBtn.addEventListener('click', () => refreshLayers());
  }

  window.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && !modal.hidden) {
      close();
    }
  }, true);

  window.emuInspector = { open, close, refreshLayers };
})();
