// GFX-009 map generator view: drives scripts/mapgen.py through the backend and
// renders each run's steps (layout, inventory, tileset batches) with review actions.
(() => {
  const { invoke, Channel } = window.__TAURI__.core;
  const $ = (id) => document.getElementById(id);
  const STATUS = { pending: 'To do', planning: 'Planning…', generating: 'Generating…', checking: 'Checking…',
    running: 'Running…', review: 'To review', accepted: 'Accepted', failed: 'Failed' };
  let run = null, runs = [], busy = false, selected = null, shown = {}, keyReady = true, pausedByView = false;
  const images = new Map();

  // ---- view switch -------------------------------------------------------
  for (const button of document.querySelectorAll('#views button')) {
    button.addEventListener('click', () => showView(button.dataset.view));
  }
  function showView(view) {
    const mapgen = view === 'mapgen';
    document.body.classList.toggle('view-mapgen', mapgen);
    for (const b of document.querySelectorAll('#views button')) b.setAttribute('aria-pressed', String(b.dataset.view === view));
    if (mapgen && !pausedByView) { pausedByView = true; invoke('set_paused', { paused: true }).catch(() => {}); }
    if (!mapgen && pausedByView) { pausedByView = false; invoke('set_paused', { paused: false }).catch(() => {}); }
    if (mapgen) refreshList();
  }

  // ---- backend -----------------------------------------------------------
  function call(args, label) {
    setError('');
    busy = true; activity(label); render();
    const channel = new Channel();
    channel.onmessage = (event) => {
      if (event.event === 'progress') { activity(event.message); log(event.message); }
      if (event.event === 'state' && (!run || event.state.id === run.id)) { run = event.state; render(); }
    };
    return invoke('mapgen_run', { args, onEvent: channel })
      .catch((error) => { setError(String(error)); log(`Error: ${error}`); throw error; })
      .finally(() => { busy = false; activity(''); render(); refreshList(); });
  }
  const read = (args) => invoke('mapgen_run', { args, onEvent: new Channel() });

  async function refreshList(select) {
    try {
      const result = await read(['list']);
      runs = result.runs; keyReady = result.key_ready;
      const want = select || run?.id || runs[0]?.id;
      if (want) { run = (await read(['state', want])).state; } else run = null;
    } catch (error) { setError(String(error)); }
    render();
  }

  async function image(path) {
    if (!images.has(path)) {
      images.set(path, invoke('mapgen_image', { path }).then((bytes) => new Promise((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => resolve(reader.result);
        reader.onerror = () => reject(reader.error);
        reader.readAsDataURL(new Blob([bytes instanceof ArrayBuffer ? bytes : new Uint8Array(bytes)], { type: 'image/png' }));
      })).catch((error) => { images.delete(path); log(`Image not shown: ${error}`); return ''; }));
    }
    return images.get(path);
  }

  // ---- feedback ----------------------------------------------------------
  function activity(text) {
    $('mg-activity').hidden = !busy;
    if (text) $('mg-activity').querySelector('span').textContent = text;
  }
  function log(text) {
    const pre = $('mg-log').querySelector('pre');
    pre.textContent = `${new Date().toLocaleTimeString()} ${text}\n${pre.textContent}`.slice(0, 20000);
  }
  function setError(text) { $('mg-error').hidden = !text; $('mg-error').textContent = text; }
  function chip(el, status) { el.dataset.s = status || ''; el.textContent = STATUS[status] || ''; }
  const path = (file) => `${run.dir}/${file}`;

  // ---- rendering ---------------------------------------------------------
  function render() {
    const select = $('mg-run');
    select.replaceChildren(...runs.map((r) => new Option(`${r.scene || r.id} · ${r.accepted}/${r.batches} sheets`, r.id)));
    if (run) select.value = run.id;
    select.disabled = busy || !runs.length;
    $('mg-empty').hidden = !!run || !$('mg-create').hidden;
    $('mg-steps').hidden = !run;
    $('mg-settings-toggle').disabled = !run;
    $('mg-new').disabled = busy;
    if (!keyReady) setError('OpenAI key file missing: put OPENAI_API_KEY in mapgen/openai.env under the Tauros data folder.');
    if (!run) { $('mg-usage').textContent = ''; return; }
    const u = run.usage || {};
    $('mg-usage').textContent = `Images: ${u.image_calls || 0} · $${(u.image_cost_usd || 0).toFixed(2)} billed by OpenAI` +
      ` · Claude: ${u.claude_calls || 0} calls on your plan, no charge`;
    $('mg-usage').title = 'Claude runs through the signed-in CLI, so it uses your plan limits, not API billing.' +
      (u.claude_cost_usd ? ` The CLI estimates these calls at $${u.claude_cost_usd.toFixed(2)} at API prices.` : '');
    renderLayout(); renderInventory(); renderSheets();
  }

  function renderLayout() {
    const step = run.layout, card = $('mg-layout');
    chip(card.querySelector('.chip'), step.status);
    const plan = step.plan;
    card.querySelector('.mg-desc').textContent = plan
      ? `${plan.width} × ${plan.height} metatiles, ${plan.orientation}. ${plan.layout}`
      : 'Claude plans the full map from the scene description, then the image model draws it at plan scale.';
    renderVersions(card, step, 'layout');
    const has = step.versions.length > 0;
    $('mg-layout-generate').textContent = has ? 'Regenerate' : 'Generate layout';
    $('mg-layout-generate').disabled = busy;
    $('mg-layout-replan').disabled = busy || !plan;
    $('mg-layout-accept').disabled = busy || !has || shownIndex(step, 'layout') === step.accepted;
    $('mg-layout-apply').disabled = busy || !has;
  }

  function renderInventory() {
    const step = run.inventory, card = $('mg-inventory');
    const ready = run.layout.accepted !== null && run.layout.accepted !== undefined;
    card.dataset.locked = String(!ready);
    chip(card.querySelector('.chip'), step.status === 'pending' ? '' : step.status);
    let text = ready ? 'Claude lists every distinct asset of the accepted layout; batches are planned automatically.'
      : 'Accept a layout first.';
    if (step.status === 'accepted') {
      text = `${step.count} assets in ${run.batches.length} batches.`;
      if (step.uncertainties?.length) text += ` Uncertain: ${step.uncertainties.slice(0, 3).join('; ')}`;
    }
    if (step.problems?.length) text = `Rejected: ${step.problems.slice(0, 3).join('; ')}`;
    card.querySelector('.mg-desc').textContent = text;
    $('mg-inventory-run').textContent = step.status === 'accepted' ? 'Rebuild inventory' : 'Build inventory';
    $('mg-inventory-run').disabled = busy || !ready;
  }

  function renderSheets() {
    const card = $('mg-sheets'), batches = run.batches;
    card.dataset.locked = String(!batches.length);
    const accepted = batches.filter((b) => b.accepted !== null && b.accepted !== undefined).length;
    const pending = batches.filter((b) => !b.versions.length).length;
    chip(card.querySelector('.chip'), batches.length && accepted === batches.length ? 'accepted' : '');
    card.querySelector('.mg-desc').textContent = batches.length
      ? `${accepted} of ${batches.length} sheets accepted · ${pending} not generated yet. Select a batch to review it.`
      : 'Build the inventory first.';
    $('mg-sheets-all').disabled = busy || !pending;
    $('mg-sheets-all').textContent = pending ? `Generate ${pending} pending` : 'All generated';
    const grid = $('mg-batches');
    grid.replaceChildren(...batches.map((b) => {
      const button = document.createElement('button');
      button.type = 'button'; button.className = 'mg-batch';
      button.setAttribute('aria-pressed', String(selected === b.n));
      const thumb = document.createElement('img'); thumb.className = 'thumb'; thumb.alt = '';
      const version = b.versions[b.accepted ?? b.versions.length - 1];
      if (version) image(path(version.thumb)).then((url) => { if (url) thumb.src = url; });
      const title = document.createElement('div'); title.className = 'row';
      title.append(Object.assign(document.createElement('b'), { textContent: `${b.n}. ${b.title}` }));
      const meta = document.createElement('div'); meta.className = 'row';
      const status = document.createElement('span'); status.className = 'chip'; chip(status, b.status);
      const checks = version?.checks;
      meta.append(Object.assign(document.createElement('span'), { textContent: `${b.lines.length} assets` +
        (checks ? (checks.pass ? ' · checks ok' : ' · check issues') : '') }), status);
      button.append(thumb, title, meta);
      button.addEventListener('click', () => { selected = selected === b.n ? null : b.n; render(); });
      return button;
    }));
    renderBatchDetail();
  }

  function renderBatchDetail() {
    const detail = $('mg-batch-detail'), batch = run.batches.find((b) => b.n === selected);
    detail.hidden = !batch;
    if (!batch) return;
    $('mg-batch-title').textContent = `Batch ${batch.n} · ${batch.title}`;
    chip(detail.querySelector('.chip'), batch.status);
    const key = `batch${batch.n}`;
    renderVersions(detail, batch, key);
    const version = batch.versions[shownIndex(batch, key)];
    const missing = new Set([...(version?.checks?.verify?.missing || []), ...(version?.checks?.verify?.cropped || [])]);
    $('mg-batch-assets').replaceChildren(...batch.lines.map((line, i) => {
      const li = document.createElement('li'); li.textContent = line;
      if (missing.has(i + 1)) li.className = 'missing';
      return li;
    }));
    $('mg-batch-generate').textContent = batch.versions.length ? 'Regenerate' : 'Generate';
    $('mg-batch-generate').disabled = busy;
    $('mg-batch-apply').disabled = busy || !batch.versions.length;
    $('mg-batch-accept').disabled = busy || !batch.versions.length || shownIndex(batch, key) === batch.accepted;
  }

  function shownIndex(step, key) {
    const last = step.versions.length - 1;
    return shown[`${run.id}:${key}`] ?? step.accepted ?? last;
  }

  function renderVersions(container, step, key) {
    const figure = container.querySelector('.mg-figure'), checks = container.querySelector('.mg-checks');
    figure.replaceChildren(); checks.replaceChildren();
    if (!step.versions.length) return;
    const index = Math.min(shownIndex(step, key), step.versions.length - 1);
    const version = step.versions[index];
    const main = document.createElement('img');
    main.alt = `Version ${index + 1}`;
    image(path(version.file)).then((url) => { main.src = url; });
    const cost = version.usage ? ` · ${version.usage.model} · $${version.usage.cost_usd.toFixed(3)}` : '';
    main.title = `Version ${index + 1}${cost}`;
    main.addEventListener('click', () => openLightbox(path(version.file), `Version ${index + 1}${cost}${version.correction ? ` · correction: ${version.correction}` : ''}`));
    figure.append(main);
    if (step.versions.length > 1) {
      const strip = document.createElement('div'); strip.className = 'mg-versions';
      step.versions.forEach((v, i) => {
        const b = document.createElement('button'); b.type = 'button';
        b.setAttribute('aria-pressed', String(i === index));
        b.title = `Version ${i + 1} · ${v.kind}${i === step.accepted ? ' · accepted' : ''}`;
        const img = document.createElement('img'); image(path(v.thumb)).then((url) => { img.src = url; });
        const tag = Object.assign(document.createElement('span'), { className: 'tag', textContent: i === step.accepted ? `v${i + 1} ✓` : `v${i + 1}` });
        b.append(img, tag);
        b.addEventListener('click', () => { shown[`${run.id}:${key}`] = i; render(); });
        strip.append(b);
      });
      figure.append(strip);
    }
    const c = version.checks;
    if (c) {
      const line = document.createElement('div');
      line.className = c.pass ? 'good' : 'bad';
      line.textContent = c.pass ? 'Automatic checks passed.' : `Check issues: ${c.reasons.join('; ')}`;
      checks.append(line);
      if (c.verify_error) checks.append(Object.assign(document.createElement('div'), { textContent: `Claude check unavailable: ${c.verify_error}` }));
      if (c.verify?.extra?.length) checks.append(Object.assign(document.createElement('div'), { textContent: `Not requested: ${c.verify.extra.join('; ')}` }));
      if (c.verify?.notes) checks.append(Object.assign(document.createElement('div'), { textContent: c.verify.notes }));
    }
  }

  function openLightbox(file, caption) {
    const box = $('mg-lightbox');
    image(file).then((url) => { box.querySelector('img').src = url; });
    $('mg-lightbox-caption').textContent = caption;
    box.hidden = false; $('mg-lightbox-close').focus();
  }
  $('mg-lightbox-close').addEventListener('click', () => { $('mg-lightbox').hidden = true; });
  $('mg-lightbox').addEventListener('click', (e) => { if (e.target.id === 'mg-lightbox') e.target.hidden = true; });
  document.addEventListener('keydown', (e) => { if (e.key === 'Escape') $('mg-lightbox').hidden = true; });

  // ---- actions -----------------------------------------------------------
  const quiet = () => {};
  // Destructive actions need a second click within a few seconds (no native dialogs).
  const armed = new Map();
  function confirmed(button, warning) {
    if (armed.get(button)) { clearTimeout(armed.get(button)); armed.delete(button); return true; }
    const label = button.textContent;
    button.textContent = 'Click again to confirm'; setError(warning);
    armed.set(button, setTimeout(() => { armed.delete(button); button.textContent = label; setError(''); }, 5000));
    return false;
  }
  $('mg-run').addEventListener('change', (e) => { selected = null; refreshList(e.target.value); });
  $('mg-new').addEventListener('click', () => { $('mg-create').hidden = false; $('mg-scene').focus(); render(); });
  $('mg-create-cancel').addEventListener('click', () => { $('mg-create').hidden = true; render(); });
  $('mg-create').addEventListener('submit', async (e) => {
    e.preventDefault();
    const args = ['create', '--scene', $('mg-scene').value];
    if ($('mg-style').value.trim()) args.push('--style', $('mg-style').value.trim());
    try {
      const done = await call(args, 'Creating the map…');
      $('mg-create').hidden = true; $('mg-scene').value = ''; selected = null;
      await refreshList(done.run);
    } catch { /* shown */ }
  });
  $('mg-settings-toggle').addEventListener('click', () => {
    const panel = $('mg-settings'), open = panel.hidden;
    panel.hidden = !open; $('mg-settings-toggle').setAttribute('aria-expanded', String(open));
    if (open && run) {
      const s = run.settings;
      $('mg-layout-model').value = s.layout_model; $('mg-sheet-model').value = s.sheet_model;
      $('mg-image-quality').value = s.image_quality; $('mg-transparent').checked = s.transparent;
      $('mg-claude-model').value = s.claude_model; $('mg-retries').value = s.auto_retries; $('mg-verify').checked = s.verify;
    }
  });
  $('mg-settings-save').addEventListener('click', () => call(['settings', run.id,
    '--layout-model', $('mg-layout-model').value.trim() || run.settings.layout_model,
    '--sheet-model', $('mg-sheet-model').value.trim() || run.settings.sheet_model,
    '--transparent', $('mg-transparent').checked ? 'on' : 'off',
    '--image-quality', $('mg-image-quality').value, '--claude-model', $('mg-claude-model').value,
    '--auto-retries', String(Number($('mg-retries').value) || 0), '--verify', $('mg-verify').checked ? 'on' : 'off'],
    'Saving settings…').then(() => { $('mg-settings').hidden = true; }, quiet));

  $('mg-layout-generate').addEventListener('click', () => {
    delete shown[`${run.id}:layout`];
    call(['layout', run.id], 'Generating the layout…').catch(quiet);
  });
  $('mg-layout-replan').addEventListener('click', () => {
    delete shown[`${run.id}:layout`];
    call(['layout', run.id, '--replan'], 'Planning a new layout…').catch(quiet);
  });
  $('mg-layout-apply').addEventListener('click', () => {
    const text = $('mg-layout-correction').value.trim();
    if (!text) return $('mg-layout-correction').focus();
    delete shown[`${run.id}:layout`];
    call(['layout', run.id, '--correction', text], 'Applying the correction…')
      .then(() => { $('mg-layout-correction').value = ''; }, quiet);
  });
  $('mg-layout-accept').addEventListener('click', () => {
    const index = shownIndex(run.layout, 'layout');
    if (run.batches.length && !confirmed($('mg-layout-accept'), 'Accepting another layout clears the inventory and all batches.')) return;
    call(['accept', run.id, 'layout', '--version', String(index + 1)], 'Accepting…').catch(quiet);
  });
  $('mg-inventory-run').addEventListener('click', () => {
    if (run.batches.some((b) => b.versions.length) && !confirmed($('mg-inventory-run'), 'Rebuilding the inventory replaces all batches and their sheets.')) return;
    selected = null;
    call(['inventory', run.id], 'Building the inventory…').catch(quiet);
  });
  $('mg-sheets-all').addEventListener('click', () => call(['sheets', run.id], 'Generating sheets…').catch(quiet));
  $('mg-batch-generate').addEventListener('click', () => {
    delete shown[`${run.id}:batch${selected}`];
    call(['sheet', run.id, String(selected)], `Generating batch ${selected}…`).catch(quiet);
  });
  $('mg-batch-apply').addEventListener('click', () => {
    const text = $('mg-batch-correction').value.trim();
    if (!text) return $('mg-batch-correction').focus();
    delete shown[`${run.id}:batch${selected}`];
    call(['sheet', run.id, String(selected), '--correction', text], 'Applying the correction…')
      .then(() => { $('mg-batch-correction').value = ''; }, quiet);
  });
  $('mg-batch-accept').addEventListener('click', () => {
    const batch = run.batches.find((b) => b.n === selected);
    call(['accept', run.id, 'batch', '--batch', String(selected), '--version', String(shownIndex(batch, `batch${selected}`) + 1)],
      'Accepting…').catch(quiet);
  });
  $('mg-stop').addEventListener('click', () => { activity('Stopping…'); invoke('mapgen_cancel').catch(() => {}); });

  render();
})();
