// APP-003 Block 1: mGBA-inspired emulator menus, transport controls, savestates,
// saves, screenshots, speed/rewind/frame-advance, key remapping, ROM info,
// emulator log and project build/live-location context.
(() => {
  const { invoke, Channel } = window.__TAURI__.core;

  const DEFAULT_KEYMAP = {
    a: 'x', b: 'z', select: 'backspace', start: 'enter',
    right: 'arrowright', left: 'arrowleft', up: 'arrowup', down: 'arrowdown',
    r: 's', l: 'a',
  };
  const DEFAULT_PREFS = {
    scale: '3x', aspect_locked: true, muted: false, volume: 100,
    speed: 1, pause_on_blur: true, rewind_enabled: true,
  };
  const BUTTON_ORDER = ['a', 'b', 'select', 'start', 'right', 'left', 'up', 'down', 'r', 'l'];
  const BUTTON_LABELS = {
    a: 'A', b: 'B', select: 'Select', start: 'Start',
    right: 'D-Pad Right', left: 'D-Pad Left', up: 'D-Pad Up', down: 'D-Pad Down',
    r: 'R', l: 'L',
  };

  let config = { keymap: { ...DEFAULT_KEYMAP }, prefs: { ...DEFAULT_PREFS } };
  let lastStatus = {
    mode: 'idle', loaded: false, paused: false, held: false,
    speed: 1, fps: 0, frame_count: 0, rewind_count: 0,
    rom_name: '', rom_sha256: null, rom_info: null,
  };
  let slots = Array.from({ length: 9 }, (_, i) => ({ slot: i + 1, available: false }));
  let toastText = '';
  let toastTimer = null;
  let capturingButton = null;
  let draftKeymap = null;
  let busy = false;

  const $ = (id) => document.getElementById(id);
  const menubar = $('emu-menubar');
  const canvas = $('screen');
  const screenStage = $('screen-stage');
  const screenBadge = $('screen-badge');
  const statusEl = $('status');
  const modePill = $('emu-mode-pill');
  const frameReadout = $('emu-frame-readout');
  const audioReadout = $('emu-audio-readout');
  const rewindReadout = $('emu-rewind-readout');
  const toastEl = $('emu-toast');
  const helpEl = $('help');

  const btnPause = $('tb-pause');
  const btnStep = $('tb-step');
  const btnRewind = $('tb-rewind');
  const btnSpeed = $('tb-speed');
  const btnReset = $('tb-reset');
  const btnShot = $('tb-shot');
  const btnMute = $('tb-mute');
  const volSlider = $('menu-volume');
  const volLabel = $('menu-volume-label');

  const saveInput = $('sav-file');
  const stateInput = $('state-file');

  const romModal = $('emu-rom-modal');
  const keysModal = $('emu-keys-modal');
  const logModal = $('emu-log-modal');
  const playBanner = $('play-banner');
  const playLiveChip = $('play-live-chip');

  function normalizeWebKey(event) {
    const code = (event.code || '').toLowerCase();
    if (/^key[a-z]$/.test(code)) return code.slice(3);
    if (/^digit[0-9]$/.test(code)) return code.slice(5);
    if (/^numpad[0-9]$/.test(code)) return code;
    const mapCode = {
      enter: 'enter', numpadenter: 'enter', backspace: 'backspace',
      space: 'space', tab: 'tab', escape: 'escape',
      arrowup: 'arrowup', arrowdown: 'arrowdown', arrowleft: 'arrowleft', arrowright: 'arrowright',
      shiftleft: 'shift', shiftright: 'shift',
      comma: 'comma', period: 'period', slash: 'slash', semicolon: 'semicolon',
      quote: 'quote', bracketleft: 'bracketleft', bracketright: 'bracketright',
      minus: 'minus', equal: 'equal',
    };
    if (code in mapCode) return mapCode[code];
    const key = (event.key || '').toLowerCase();
    if (/^[a-z0-9]$/.test(key)) return key;
    if (/^f([1-9]|1[0-2])$/.test(key)) return key;
    const mapKey = {
      enter: 'enter', backspace: 'backspace', ' ': 'space', tab: 'tab', escape: 'escape',
      arrowup: 'arrowup', arrowdown: 'arrowdown', arrowleft: 'arrowleft', arrowright: 'arrowright',
      shift: 'shift', ',': 'comma', '.': 'period', '/': 'slash', ';': 'semicolon',
      "'": 'quote', '[': 'bracketleft', ']': 'bracketright', '-': 'minus', '=': 'equal',
    };
    return mapKey[key] || null;
  }

  function displayKey(norm) {
    const pretty = {
      arrowup: '↑', arrowdown: '↓', arrowleft: '←', arrowright: '→',
      enter: 'Enter', backspace: 'Backspace', space: 'Space', shift: 'Shift',
      comma: ',', period: '.', slash: '/', semicolon: ';', quote: "'",
      bracketleft: '[', bracketright: ']', minus: '-', equal: '=',
    };
    if (norm in pretty) return pretty[norm];
    return norm.length === 1 ? norm.toUpperCase() : norm;
  }

  function buttonBit(event) {
    const norm = normalizeWebKey(event);
    if (!norm) return undefined;
    const idx = BUTTON_ORDER.findIndex((b) => config.keymap[b] === norm);
    return idx >= 0 ? idx : undefined;
  }

  function showToast(msg, isError = false) {
    toastText = msg;
    toastEl.textContent = msg;
    toastEl.classList.toggle('error', isError);
    if (toastTimer) clearTimeout(toastTimer);
    if (msg) {
      toastTimer = setTimeout(() => {
        toastText = '';
        toastEl.textContent = '';
        toastEl.classList.remove('error');
      }, 4500);
    }
  }

  function closeMenus() {
    for (const item of menubar.querySelectorAll('.menu-item')) {
      item.classList.remove('open');
      const btn = item.querySelector('.menu-trigger');
      if (btn) btn.setAttribute('aria-expanded', 'false');
    }
  }

  function renderHelp() {
    const km = config.keymap;
    helpEl.textContent = '';
    const pairs = [
      [`${displayKey(km.up)}${displayKey(km.down)}${displayKey(km.left)}${displayKey(km.right)}`, 'D-pad'],
      [displayKey(km.a), 'A'], [displayKey(km.b), 'B'],
      [displayKey(km.start), 'Start'], [displayKey(km.select), 'Select'],
      [displayKey(km.l), 'L'], [displayKey(km.r), 'R'],
    ];
    pairs.forEach(([k, label], i) => {
      if (i > 0) helpEl.append(' · ');
      const kbd = document.createElement('kbd');
      kbd.textContent = k;
      helpEl.append(kbd, ` ${label}`);
    });
    helpEl.append(' · ', 'click game for audio');
  }

  function applyScalePrefs() {
    const scale = config.prefs.scale;
    const locked = config.prefs.aspect_locked;
    const widths = { fit: '100%', '1x': '240px', '2x': '480px', '3x': '720px', '4x': '960px' };
    canvas.style.width = widths[scale] || '720px';
    canvas.style.maxWidth = '100%';
    canvas.style.aspectRatio = locked ? '3 / 2' : 'auto';
    canvas.style.height = locked ? 'auto' : '320px';
    screenStage.dataset.scale = scale;

    for (const el of menubar.querySelectorAll('[data-scale]')) {
      el.setAttribute('aria-checked', String(el.dataset.scale === scale));
    }
    $('menu-aspect').setAttribute('aria-checked', String(locked));
  }

  function applyAudioPrefs() {
    const muted = config.prefs.muted;
    const vol = config.prefs.volume;
    btnMute.textContent = muted ? '🔇 Muted' : '🔊 Audio';
    btnMute.setAttribute('aria-pressed', String(muted));
    $('menu-mute').setAttribute('aria-checked', String(muted));
    volSlider.value = String(vol);
    volLabel.textContent = `${vol}%`;
    audioReadout.textContent = muted ? 'Sound: Muted' : `Sound: ${vol}%`;
  }

  function speedLabel(speed) {
    return speed === 0 ? 'Turbo' : `${speed}x`;
  }

  function applyEmulationPrefs() {
    const speed = config.prefs.speed;
    btnSpeed.textContent = `⏩ ${speedLabel(speed)}`;
    btnSpeed.setAttribute('aria-pressed', String(speed !== 1));
    $('menu-ff-toggle').setAttribute('aria-checked', String(speed !== 1));
    for (const el of menubar.querySelectorAll('[data-speed]')) {
      el.setAttribute('aria-checked', String(Number(el.dataset.speed) === speed));
    }
    $('menu-blur-pause').setAttribute('aria-checked', String(config.prefs.pause_on_blur));
    $('menu-rewind-toggle').setAttribute('aria-checked', String(config.prefs.rewind_enabled));
  }

  function renderSlots() {
    for (const kind of ['save', 'load']) {
      const container = $(`menu-slots-${kind}`);
      container.textContent = '';
      for (const s of slots) {
        const btn = document.createElement('button');
        btn.type = 'button';
        btn.role = 'menuitem';
        btn.className = 'menu-entry';
        btn.dataset.action = kind === 'save' ? 'save-slot' : 'load-slot';
        btn.dataset.slot = String(s.slot);
        btn.textContent = `Slot ${s.slot}` + (s.available ? ' ●' : ' (empty)');
        if (kind === 'load' && (!s.available || !lastStatus.loaded || busy)) {
          btn.disabled = true;
        } else if (kind === 'save' && (!lastStatus.loaded || busy)) {
          btn.disabled = true;
        }
        container.appendChild(btn);
      }
    }
  }

  function renderStatus(st) {
    const prevMode = lastStatus.mode;
    lastStatus = st;
    const loaded = !!st.loaded;
    const held = st.mode === 'agent_held';
    const paused = st.mode === 'paused';

    btnPause.textContent = paused ? '▶ Resume' : '⏸ Pause';
    btnPause.setAttribute('aria-pressed', String(paused));
    $('menu-pause').textContent = paused ? 'Resume' : 'Pause';
    btnPause.disabled = !loaded || held || busy;
    btnStep.disabled = !loaded || held || busy;
    btnReset.disabled = !loaded || held || busy;
    btnRewind.disabled = !loaded || held || busy || st.rewind_count === 0;
    btnRewind.textContent = `⏪ Rewind (${st.rewind_count})`;
    btnShot.disabled = !loaded;
    btnSpeed.disabled = !loaded || held;

    for (const id of ['menu-rom-info', 'menu-export-sav', 'menu-import-sav', 'menu-save-state-file',
      'menu-load-state-file', 'menu-pause', 'menu-step', 'menu-reset', 'menu-shot']) {
      const el = $(id);
      if (el) el.disabled = !loaded || (held && id !== 'menu-rom-info' && id !== 'menu-shot');
    }
    $('menu-rewind').disabled = !loaded || held || busy || st.rewind_count === 0;

    // Explicit mode pill & canvas overlay badge (never show 10 Hz held refresh as FPS).
    modePill.dataset.mode = st.mode;
    if (st.mode === 'idle') {
      modePill.textContent = 'Idle';
      if (prevMode !== 'idle') statusEl.textContent = 'Open a ROM to start.';
      screenBadge.hidden = true;
    } else if (st.mode === 'agent_held') {
      modePill.textContent = 'Agent held';
      statusEl.textContent = `${st.rom_name || 'ROM'} · Agent held (preview paused)`;
      screenBadge.textContent = 'Agent held · preview paused';
      screenBadge.dataset.mode = 'agent_held';
      screenBadge.hidden = false;
    } else if (st.mode === 'paused') {
      modePill.textContent = 'Paused';
      statusEl.textContent = `${st.rom_name || 'ROM'} · Paused at frame ${st.frame_count}`;
      screenBadge.textContent = `Paused · Frame ${st.frame_count}`;
      screenBadge.dataset.mode = 'paused';
      screenBadge.hidden = false;
    } else if (st.mode === 'fast_forward') {
      const fpsStr = st.fps > 0 ? `${st.fps.toFixed(0)} fps` : speedLabel(st.speed);
      modePill.textContent = `Fast ${speedLabel(st.speed)} · ${fpsStr}`;
      statusEl.textContent = `${st.rom_name || 'ROM'} · ${speedLabel(st.speed)} (${fpsStr})`;
      screenBadge.textContent = `Fast ${speedLabel(st.speed)}`;
      screenBadge.dataset.mode = 'fast_forward';
      screenBadge.hidden = false;
    } else {
      const fpsStr = st.fps > 0 ? `${st.fps.toFixed(1)} fps` : '59.7 fps';
      modePill.textContent = fpsStr;
      statusEl.textContent = `${st.rom_name || 'ROM'} · ${fpsStr}`;
      screenBadge.hidden = true;
    }
    frameReadout.textContent = `Frame ${st.frame_count}`;
    rewindReadout.textContent = config.prefs.rewind_enabled ? `Rewind: ${st.rewind_count}` : 'Rewind: Off';
    renderSlots();
  }

  async function refreshSlots() {
    try {
      slots = await invoke('emu_savestate_slots');
      renderSlots();
    } catch (err) {
      showToast(`Slots refresh failed: ${err}`, true);
    }
  }

  async function savePrefs(patch) {
    const nextPrefs = { ...config.prefs, ...patch };
    try {
      config = await invoke('emu_set_prefs', { prefs: nextPrefs });
      applyScalePrefs();
      applyAudioPrefs();
      applyEmulationPrefs();
      renderStatus(lastStatus);
    } catch (err) {
      showToast(`Failed to save preference: ${err}`, true);
    }
  }

  async function togglePause() {
    if (!lastStatus.loaded || lastStatus.held) return;
    const nextPaused = !lastStatus.paused;
    await invoke('set_paused', { paused: nextPaused });
    showToast(nextPaused ? 'Emulation paused' : 'Emulation resumed');
  }

  async function stepFrame() {
    try {
      const frame = await invoke('emu_step_frame');
      showToast(`Stepped to frame ${frame}`);
    } catch (err) {
      showToast(`Frame advance failed: ${err}`, true);
    }
  }

  async function resetEmu() {
    try {
      await invoke('emu_reset');
      await refreshSlots();
      showToast('Console reset');
    } catch (err) {
      showToast(`Reset failed: ${err}`, true);
    }
  }

  async function rewindEmu() {
    try {
      const frame = await invoke('emu_rewind');
      showToast(`Rewound to frame ${frame}`);
    } catch (err) {
      showToast(`${err}`, true);
    }
  }

  async function cycleSpeed() {
    const order = [1, 2, 4, 8, 0];
    const idx = order.indexOf(config.prefs.speed);
    const next = order[(idx + 1) % order.length];
    await savePrefs({ speed: next });
    showToast(`Speed: ${speedLabel(next)}`);
  }

  async function takeScreenshot() {
    try {
      const res = await invoke('emu_screenshot');
      showToast(`Screenshot saved: ${res.path.split('/').pop()}`);
    } catch (err) {
      showToast(`Screenshot failed: ${err}`, true);
    }
  }

  async function exportSave() {
    try {
      const res = await invoke('emu_export_save');
      showToast(`Saved .sav (${res.bytes} bytes): ${res.path.split('/').pop()}`);
    } catch (err) {
      showToast(`Save export failed: ${err}`, true);
    }
  }

  async function saveQuickSlot(slot) {
    try {
      const res = await invoke('emu_save_slot', { slot });
      await refreshSlots();
      showToast(`Saved slot ${slot} (frame ${res.frame_count})`);
    } catch (err) {
      showToast(`Quick save failed: ${err}`, true);
    }
  }

  async function loadQuickSlot(slot) {
    try {
      const frame = await invoke('emu_load_slot', { slot });
      showToast(`Loaded slot ${slot} (frame ${frame})`);
    } catch (err) {
      showToast(`Quick load failed: ${err}`, true);
    }
  }

  async function saveStateToFile() {
    try {
      const res = await invoke('emu_save_state_file');
      showToast(`Saved state: ${res.path.split('/').pop()}`);
    } catch (err) {
      showToast(`Save state failed: ${err}`, true);
    }
  }

  function openModal(modal) {
    closeMenus();
    modal.hidden = false;
    invoke('set_typing', { typing: true });
  }

  function closeModal(modal) {
    modal.hidden = true;
    capturingButton = null;
    invoke('set_typing', { typing: false });
    canvas.focus();
  }

  function openRomInfo() {
    const info = lastStatus.rom_info;
    const sha = lastStatus.rom_sha256 || 'Unavailable';
    $('rom-info-body').textContent = '';
    const rows = [
      ['Source / Target', lastStatus.rom_name || 'None'],
      ['Header Title', info ? (info.title || '(empty)') : '—'],
      ['Game Code', info ? (info.code || '—') : '—'],
      ['ROM Size', info ? `${(info.rom_size / 1024 / 1024).toFixed(2)} MiB (${info.rom_size} bytes)` : '—'],
      ['Save Memory', info ? (info.save_size ? `${info.save_size} bytes` : 'Inactive / None') : '—'],
      ['Frame Counter', String(lastStatus.frame_count)],
      ['SHA-256', sha],
    ];
    for (const [k, v] of rows) {
      const dt = document.createElement('dt');
      dt.textContent = k;
      const dd = document.createElement('dd');
      dd.textContent = v;
      $('rom-info-body').append(dt, dd);
    }
    openModal(romModal);
  }

  function renderKeyModal() {
    const grid = $('keys-grid');
    grid.textContent = '';
    for (const btn of BUTTON_ORDER) {
      const row = document.createElement('div');
      row.className = 'key-row';
      const label = document.createElement('span');
      label.textContent = BUTTON_LABELS[btn];
      const bindBtn = document.createElement('button');
      bindBtn.type = 'button';
      bindBtn.className = 'key-capture-btn';
      bindBtn.textContent = capturingButton === btn ? 'Press any key…' : displayKey(draftKeymap[btn]);
      bindBtn.setAttribute('aria-pressed', String(capturingButton === btn));
      bindBtn.addEventListener('click', () => {
        capturingButton = btn;
        $('keys-error').textContent = '';
        renderKeyModal();
      });
      row.append(label, bindBtn);
      grid.appendChild(row);
    }
  }

  function openKeyConfig() {
    draftKeymap = { ...config.keymap };
    capturingButton = null;
    $('keys-error').textContent = '';
    renderKeyModal();
    openModal(keysModal);
  }

  async function loadLogs() {
    try {
      const entries = await invoke('emu_logs');
      const list = $('emu-log-list');
      if (!entries.length && !list.children.length) {
        const li = document.createElement('li');
        li.className = 'muted';
        li.textContent = 'No mGBA core log messages yet.';
        list.appendChild(li);
      } else if (entries.length) {
        if (list.children.length === 1 && list.firstChild.classList.contains('muted')) {
          list.textContent = '';
        }
        for (const e of entries) {
          const li = document.createElement('li');
          li.textContent = `[${e.level}] ${e.message}`;
          list.appendChild(li);
        }
      }
    } catch (err) {
      $('emu-log-list').textContent = `Log read failed: ${err}`;
    }
  }

  async function refreshProjectContext() {
    const playMap = $('play-map');
    const playX = $('play-x');
    const playY = $('play-y');
    if (!playMap || !playMap.value) return;
    try {
      const ctx = await invoke('project_context', {
        selectedMap: playMap.value || null,
        selectedX: playX.value !== '' ? Number(playX.value) : null,
        selectedY: playY.value !== '' ? Number(playY.value) : null,
      });
      if (!ctx) return;
      if (ctx.live_status === 'ok' && ctx.live_map) {
        playLiveChip.textContent = `Live: ${ctx.live_map} (${ctx.live_x}, ${ctx.live_y})`;
        playLiveChip.dataset.state = 'ok';
      } else if (ctx.live_status === 'title_or_uninitialized') {
        playLiveChip.textContent = 'Live: Title / not in world';
        playLiveChip.dataset.state = 'muted';
      } else if (ctx.live_status === 'stale_rom') {
        playLiveChip.textContent = 'ROM stale (rebuild or preview)';
        playLiveChip.dataset.state = 'warn';
      } else {
        playLiveChip.textContent = `Live: ${ctx.live_status.replace(/_/g, ' ')}`;
        playLiveChip.dataset.state = 'muted';
      }
      playLiveChip.hidden = false;

      if (ctx.new_build_pending) {
        $('play-banner-text').textContent =
          `Agent built a new ROM (${(ctx.loaded_rom_sha256 || '').slice(0, 8)}). Click to boot directly at ${playMap.value} (${playX.value}, ${playY.value}).`;
        playBanner.hidden = false;
      } else {
        playBanner.hidden = true;
      }
    } catch (_) {
      // No project open or context unavailable.
    }
  }

  // Menu dropdown open/close
  for (const item of menubar.querySelectorAll('.menu-item')) {
    const trigger = item.querySelector('.menu-trigger');
    trigger.addEventListener('click', (e) => {
      e.stopPropagation();
      const wasOpen = item.classList.contains('open');
      closeMenus();
      if (!wasOpen) {
        item.classList.add('open');
        trigger.setAttribute('aria-expanded', 'true');
        if (item.dataset.menu === 'file') refreshSlots();
      }
    });
  }
  document.addEventListener('click', (e) => {
    if (!menubar.contains(e.target)) closeMenus();
  });

  menubar.addEventListener('click', (e) => {
    const entry = e.target.closest('[data-action], [data-scale], [data-speed]');
    if (!entry || entry.disabled) return;
    if (entry.dataset.scale) {
      closeMenus();
      savePrefs({ scale: entry.dataset.scale });
      canvas.focus();
      return;
    }
    if (entry.dataset.speed !== undefined) {
      closeMenus();
      savePrefs({ speed: Number(entry.dataset.speed) });
      canvas.focus();
      return;
    }
    const action = entry.dataset.action;
    if (action !== 'volume') closeMenus();
    switch (action) {
      case 'open-rom': $('file').click(); break;
      case 'rom-info': openRomInfo(); break;
      case 'import-sav': saveInput.click(); break;
      case 'export-sav': exportSave(); break;
      case 'save-slot': saveQuickSlot(Number(entry.dataset.slot)); break;
      case 'load-slot': loadQuickSlot(Number(entry.dataset.slot)); break;
      case 'save-state-file': saveStateToFile(); break;
      case 'load-state-file': stateInput.click(); break;
      case 'pause': togglePause(); break;
      case 'step': stepFrame(); break;
      case 'reset': resetEmu(); break;
      case 'rewind': rewindEmu(); break;
      case 'ff-toggle': savePrefs({ speed: config.prefs.speed === 1 ? 4 : 1 }); break;
      case 'blur-pause': savePrefs({ pause_on_blur: !config.prefs.pause_on_blur }); break;
      case 'rewind-toggle': savePrefs({ rewind_enabled: !config.prefs.rewind_enabled }); break;
      case 'aspect': savePrefs({ aspect_locked: !config.prefs.aspect_locked }); break;
      case 'mute': savePrefs({ muted: !config.prefs.muted }); break;
      case 'shot': takeScreenshot(); break;
      case 'keys': openKeyConfig(); break;
      case 'logs': openModal(logModal); loadLogs(); break;
      case 'view-palettes': window.emuInspector?.open('palettes'); break;
      case 'view-tiles': window.emuInspector?.open('tiles'); break;
      case 'view-oam': window.emuInspector?.open('oam'); break;
      case 'view-memory': window.emuInspector?.open('memory'); break;
    }
    if (!['rom-info', 'keys', 'logs', 'view-palettes', 'view-tiles', 'view-oam', 'view-memory'].includes(action)) canvas.focus();
  });

  volSlider.addEventListener('input', () => {
    config.prefs.volume = Number(volSlider.value);
    applyAudioPrefs();
  });
  volSlider.addEventListener('change', () => savePrefs({ volume: Number(volSlider.value), muted: false }));

  // Transport buttons
  btnPause.addEventListener('click', (e) => { e.target.blur(); togglePause(); canvas.focus(); });
  btnStep.addEventListener('click', (e) => { e.target.blur(); stepFrame(); canvas.focus(); });
  btnRewind.addEventListener('click', (e) => { e.target.blur(); rewindEmu(); canvas.focus(); });
  btnSpeed.addEventListener('click', (e) => { e.target.blur(); cycleSpeed(); canvas.focus(); });
  btnReset.addEventListener('click', (e) => { e.target.blur(); resetEmu(); canvas.focus(); });
  btnShot.addEventListener('click', (e) => { e.target.blur(); takeScreenshot(); canvas.focus(); });
  btnMute.addEventListener('click', (e) => { e.target.blur(); savePrefs({ muted: !config.prefs.muted }); canvas.focus(); });

  $('play-banner-boot').addEventListener('click', () => {
    playBanner.hidden = true;
    $('play-preview').click();
  });

  saveInput.addEventListener('change', async (e) => {
    const input = e.target;
    const file = input.files[0];
    if (!file) return;
    try {
      const bytes = new Uint8Array(await file.arrayBuffer());
      const frame = await invoke('emu_import_save', bytes);
      showToast(`Imported ${file.name} (frame ${frame})`);
    } catch (err) {
      showToast(`Import .sav failed: ${err}`, true);
    } finally {
      input.value = '';
    }
  });

  stateInput.addEventListener('change', async (e) => {
    const input = e.target;
    const file = input.files[0];
    if (!file) return;
    try {
      const bytes = new Uint8Array(await file.arrayBuffer());
      const frame = await invoke('emu_load_state_file', bytes);
      showToast(`Loaded state ${file.name} (frame ${frame})`);
    } catch (err) {
      showToast(`Load state failed: ${err}`, true);
    } finally {
      input.value = '';
    }
  });

  // Modal controls
  $('rom-info-close').addEventListener('click', () => closeModal(romModal));
  $('keys-cancel').addEventListener('click', () => closeModal(keysModal));
  $('keys-reset').addEventListener('click', () => {
    draftKeymap = { ...DEFAULT_KEYMAP };
    capturingButton = null;
    $('keys-error').textContent = '';
    renderKeyModal();
  });
  $('keys-save').addEventListener('click', async () => {
    try {
      config = await invoke('emu_set_keymap', { keymap: draftKeymap });
      renderHelp();
      closeModal(keysModal);
      showToast('Key bindings saved');
    } catch (err) {
      $('keys-error').textContent = String(err);
    }
  });
  $('emu-log-refresh').addEventListener('click', loadLogs);
  $('emu-log-clear').addEventListener('click', () => { $('emu-log-list').textContent = ''; });
  $('emu-log-close').addEventListener('click', () => closeModal(logModal));

  // Keyboard capture & shortcuts (Ctrl+O/P/N/R/B, Shift+Tab, F12, Escape)
  window.addEventListener('keydown', (e) => {
    if (capturingButton) {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === 'Escape') {
        capturingButton = null;
        renderKeyModal();
        return;
      }
      const norm = normalizeWebKey(e);
      if (!norm) {
        $('keys-error').textContent = `Unsupported key: ${e.key}`;
        return;
      }
      draftKeymap[capturingButton] = norm;
      capturingButton = null;
      $('keys-error').textContent = '';
      renderKeyModal();
      return;
    }
    if (e.key === 'Escape') {
      closeMenus();
      if (!romModal.hidden) closeModal(romModal);
      if (!keysModal.hidden) closeModal(keysModal);
      if (!logModal.hidden) closeModal(logModal);
      return;
    }
    const mod = e.ctrlKey || e.metaKey;
    if (mod && !e.altKey) {
      const k = e.key.toLowerCase();
      if (k === 'o') { e.preventDefault(); $('file').click(); }
      else if (k === 'p') { e.preventDefault(); togglePause(); }
      else if (k === 'n') { e.preventDefault(); stepFrame(); }
      else if (k === 'r') { e.preventDefault(); resetEmu(); }
      else if (k === 'b') { e.preventDefault(); rewindEmu(); }
      return;
    }
    if (e.shiftKey && e.key === 'Tab') {
      e.preventDefault();
      savePrefs({ speed: config.prefs.speed === 1 ? 4 : 1 });
      return;
    }
    if (e.key === 'F12') {
      e.preventDefault();
      takeScreenshot();
    }
  }, true);

  document.addEventListener('visibilitychange', () => {
    if (config.prefs.pause_on_blur) {
      invoke('set_paused', { paused: document.hidden });
    }
  });

  async function init() {
    try {
      config = await invoke('emu_get_config');
    } catch (_) {
      config = { keymap: { ...DEFAULT_KEYMAP }, prefs: { ...DEFAULT_PREFS } };
    }
    applyScalePrefs();
    applyAudioPrefs();
    applyEmulationPrefs();
    renderHelp();

    const statusChannel = new Channel();
    statusChannel.onmessage = (st) => renderStatus(st);
    invoke('attach_status', { onStatus: statusChannel }).catch(() => {});
    refreshSlots();
    refreshProjectContext();
  }

  window.emuControls = {
    init,
    buttonBit,
    isMuted: () => config.prefs.muted,
    getVolume: () => config.prefs.volume / 100,
    shouldPauseOnBlur: () => config.prefs.pause_on_blur,
    refreshProjectContext,
    refreshSlots,
    showError: (msg) => showToast(msg, true),
    setBusy: (nextBusy) => {
      busy = !!nextBusy;
      renderStatus(lastStatus);
    },
  };

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }
})();
