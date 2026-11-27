<script>
  // GFX-009: describe a scene, then review each step (layout, inventory, tileset
  // sheets); every image can be accepted, corrected or regenerated.
  import { onMount, onDestroy } from 'svelte';
  import Icon from '../components/Icon.svelte';
  import AsyncImage from '../components/AsyncImage.svelte';
  import { mapgen, viewActions } from '../lib/app.js';
  import { STEP_STATUS } from '../lib/mapgen.js';
  import { t } from '../lib/i18n.js';
  const { state: mg, shown } = mapgen;
  let creating = $state(false), scene = $state(''), style = $state('');
  let settingsOpen = $state(false), settings = $state(null);
  let layoutCorrection = $state(''), batchCorrection = $state('');
  let selected = $state(null);
  let lightbox = $state(null);
  let armed = $state({});

  const run = $derived($mg.run);
  const busy = $derived($mg.busy);
  const quiet = () => {};

  $effect(() => {
    viewActions.set([
      { id: 'new', primary: true, icon: 'plus', label: $t('New map…'), disabled: busy, run: () => { creating = true; } },
      { id: 'settings', icon: 'settings', label: $t('Run settings'), pressed: settingsOpen, disabled: !run, run: toggleSettings },
    ]);
  });
  onDestroy(() => viewActions.set([]));
  onMount(() => mapgen.refresh());

  function toggleSettings() {
    settingsOpen = !settingsOpen;
    if (settingsOpen && run) settings = { ...run.settings };
  }
  function saveSettings() {
    mapgen.call(['settings', run.id, '--layout-model', settings.layout_model.trim() || run.settings.layout_model,
      '--sheet-model', settings.sheet_model.trim() || run.settings.sheet_model, '--transparent', settings.transparent ? 'on' : 'off',
      '--image-quality', settings.image_quality, '--claude-model', settings.claude_model,
      '--auto-retries', String(Number(settings.auto_retries) || 0), '--verify', settings.verify ? 'on' : 'off'],
      $t('Saving settings…')).then(() => { settingsOpen = false; }, quiet);
  }
  async function create(e) {
    e.preventDefault();
    const args = ['create', '--scene', scene];
    if (style.trim()) args.push('--style', style.trim());
    try { const done = await mapgen.call(args, $t('Creating the map…')); creating = false; scene = ''; selected = null; await mapgen.refresh(done.run); }
    catch { /* shown */ }
  }
  // Destructive steps need a second click within a few seconds.
  function confirmed(key, warning) {
    if (armed[key]) { armed = { ...armed, [key]: false }; mapgen.setError(''); return true; }
    armed = { ...armed, [key]: true }; mapgen.setError(warning);
    setTimeout(() => { armed = { ...armed, [key]: false }; }, 5000);
    return false;
  }
  const idx = (step, key) => { $shown; return Math.min(mapgen.shownIndex(step, key), step.versions.length - 1); };
  const batch = $derived(run?.batches.find((b) => b.n === selected) || null);
  const accepted = $derived(run ? run.batches.filter((b) => b.accepted != null).length : 0);
  const pending = $derived(run ? run.batches.filter((b) => !b.versions.length).length : 0);
  const usage = $derived(run?.usage || {});
  const layoutReady = $derived(run && run.layout.accepted != null);
  const missing = $derived.by(() => {
    if (!batch) return new Set();
    const v = batch.versions[idx(batch, `batch${batch.n}`)];
    return new Set([...(v?.checks?.verify?.missing || []), ...(v?.checks?.verify?.cropped || [])]);
  });
</script>

{#snippet versions(step, key)}
  {#if step.versions.length}
    {@const i = idx(step, key)}
    {@const v = step.versions[i]}
    {@const cost = v.usage ? ` · ${v.usage.model} · $${v.usage.cost_usd.toFixed(3)}` : ''}
    <div class="figure checker">
      <AsyncImage path={mapgen.path(v.file)} alt={$t('Version {n}', { n: i + 1 })} class="main pixel" title={`${$t('Version {n}', { n: i + 1 })}${cost}`}
        onclick={() => (lightbox = { path: mapgen.path(v.file), caption: `${$t('Version {n}', { n: i + 1 })}${cost}${v.correction ? ` · ${$t('correction')}: ${v.correction}` : ''}` })} />
    </div>
    {#if step.versions.length > 1}
      <div class="strip">
        {#each step.versions as ver, j}
          <button type="button" aria-pressed={String(j === i)} title="v{j + 1} · {ver.kind}{j === step.accepted ? ` · ${$t('accepted')}` : ''}" onclick={() => mapgen.show(key, j)}>
            <AsyncImage path={mapgen.path(ver.thumb)} class="pixel" /><span class="tag">v{j + 1}{j === step.accepted ? ' ✓' : ''}</span></button>
        {/each}
      </div>
    {/if}
    {#if v.checks}
      <div class="checks small">
        <p class={v.checks.pass ? 'ok-text' : 'error-text'}>{v.checks.pass ? $t('Automatic checks passed.') : `${$t('Check issues')}: ${v.checks.reasons.join('; ')}`}</p>
        {#if v.checks.verify_error}<p class="muted">{$t('Claude check unavailable')}: {v.checks.verify_error}</p>{/if}
        {#if v.checks.verify?.extra?.length}<p class="muted">{$t('Not requested')}: {v.checks.verify.extra.join('; ')}</p>{/if}
        {#if v.checks.verify?.notes}<p class="muted">{v.checks.verify.notes}</p>{/if}
      </div>
    {/if}
  {/if}
{/snippet}

{#snippet status(s)}{#if s && STEP_STATUS[s]}<span class="chip {s === 'accepted' ? 'ok' : s === 'failed' ? 'err' : s === 'review' ? 'warn' : 'accent'}">{$t(STEP_STATUS[s])}</span>{/if}{/snippet}

<div class="mapgen scroll">
  <div class="top row">
    <select class="grow" value={run?.id || ''} disabled={busy || !$mg.runs.length} aria-label={$t('Map')}
      onchange={(e) => { selected = null; mapgen.refresh(e.currentTarget.value); }}>
      {#each $mg.runs as r (r.id)}<option value={r.id}>{r.scene || r.id} · {r.accepted}/{r.batches} {$t('sheets')}</option>
      {:else}<option value="">{$t('No map yet')}</option>{/each}
    </select>
    {#if run}<span class="small muted usage" title={$t('Claude runs through the signed-in CLI, so it uses your plan limits, not API billing.')}>
      {$t('Images: {n} · ${cost} billed by OpenAI · Claude: {calls} calls on your plan', { n: usage.image_calls || 0, cost: (usage.image_cost_usd || 0).toFixed(2), calls: usage.claude_calls || 0 })}</span>{/if}
  </div>

  {#if !$mg.keyReady}<p class="alert" role="alert">{$t('OpenAI key file missing: put OPENAI_API_KEY in mapgen/openai.env under the Tauros data folder.')}</p>{/if}
  {#if $mg.error}<p class="alert" role="alert">{$mg.error}</p>{/if}

  {#if creating}
    <form class="card step" onsubmit={create}>
      <h2>{$t('New map')}</h2>
      <label class="field">{$t('Scene description (notes or a voice draft are fine; English works best)')}
        <textarea bind:value={scene} rows="6" placeholder={$t('A treetop city with a crystal castle to the north, a plaza, a market…')}></textarea></label>
      <label class="field">{$t('Style reference PNG (camera angle and general look only; empty = default)')}
        <input bind:value={style} placeholder={$t('Default style reference')} /></label>
      <div class="row"><button type="submit" class="primary" disabled={busy || !scene.trim()}>{$t('Create')}</button>
        <button type="button" onclick={() => (creating = false)}>{$t('Cancel')}</button></div>
    </form>
  {/if}

  {#if settingsOpen && settings}
    <section class="card step">
      <h2>{$t('Settings for this map')}</h2>
      <p class="small muted">{$t('Flare is faster for layout drafts; Sunburst is more precise when redrawing and correcting. Corrections use low planning complexity, new images high.')}</p>
      <div class="settings">
        <label class="field">{$t('Layout image model')}<input bind:value={settings.layout_model} list="mg-models" /></label>
        <label class="field">{$t('Sheet image model')}<input bind:value={settings.sheet_model} list="mg-models" /></label>
        <datalist id="mg-models"><option value="gpt-image-2.6-flare"></option><option value="gpt-image-2.6-sunburst"></option><option value="gpt-image-2.5-flare"></option><option value="gpt-image-2.5-sunburst"></option></datalist>
        <label class="field">{$t('Image quality')}<select bind:value={settings.image_quality}>{#each ['high', 'medium', 'low', 'xhigh', 'max'] as q}<option>{q}</option>{/each}</select></label>
        <label class="field">{$t('Claude model')}<select bind:value={settings.claude_model}><option>opus</option><option>sonnet</option></select></label>
        <label class="field">{$t('Automatic retries per sheet')}<input type="number" min="0" max="3" bind:value={settings.auto_retries} /></label>
        <label class="check"><input type="checkbox" bind:checked={settings.verify} /> {$t('Claude checks each sheet')}</label>
        <label class="check"><input type="checkbox" bind:checked={settings.transparent} /> {$t('Transparent sheet background')}</label>
      </div>
      <div class="row"><button type="button" class="primary" disabled={busy} onclick={saveSettings}>{$t('Save settings')}</button>
        <button type="button" onclick={() => (settingsOpen = false)}>{$t('Cancel')}</button></div>
    </section>
  {/if}

  {#if !run && !creating}
    <div class="empty card"><Icon name="sparkles" size={28} /><p>{$t('No map yet. Choose New map…, describe the scene, then run the steps below.')}</p></div>
  {:else if run}
    <article class="card step">
      <header class="row"><span class="n">1</span><h2 class="grow">{$t('Layout')}</h2>{@render status(run.layout.status)}</header>
      <p class="small muted desc">{run.layout.plan ? `${run.layout.plan.width} × ${run.layout.plan.height} metatiles, ${run.layout.plan.orientation}. ${run.layout.plan.layout}`
        : $t('Claude plans the full map from the scene description, then the image model draws it at plan scale.')}</p>
      {@render versions(run.layout, 'layout')}
      {#if run.layout.versions.length}
        <details><summary class="small">{$t('Correct the layout…')}</summary>
          <textarea bind:value={layoutCorrection} rows="3" placeholder={$t('What should change, e.g. move the market to the east platform')}></textarea>
          <button type="button" disabled={busy || !layoutCorrection.trim()} onclick={() => { mapgen.forget('layout');
            mapgen.call(['layout', run.id, '--correction', layoutCorrection.trim()], $t('Applying the correction…')).then(() => (layoutCorrection = ''), quiet); }}>{$t('Apply correction')}</button></details>
      {/if}
      <div class="row">
        <button type="button" class="primary" disabled={busy} onclick={() => { mapgen.forget('layout'); mapgen.call(['layout', run.id], $t('Generating the layout…')).catch(quiet); }}>
          {run.layout.versions.length ? $t('Regenerate') : $t('Generate layout')}</button>
        <button type="button" disabled={busy || !run.layout.plan} title={$t('Ask Claude for a new plan, then draw it')}
          onclick={() => { mapgen.forget('layout'); mapgen.call(['layout', run.id, '--replan'], $t('Planning a new layout…')).catch(quiet); }}>{$t('New plan')}</button>
        <button type="button" disabled={busy || !run.layout.versions.length || idx(run.layout, 'layout') === run.layout.accepted}
          onclick={() => { if (run.batches.length && !confirmed('layout', $t('Accepting another layout clears the inventory and all batches.'))) return;
            mapgen.call(['accept', run.id, 'layout', '--version', String(idx(run.layout, 'layout') + 1)], $t('Accepting…')).catch(quiet); }}>
          {armed.layout ? $t('Click again to confirm') : $t('Accept this version')}</button>
      </div>
    </article>

    <article class="card step" class:locked={!layoutReady}>
      <header class="row"><span class="n">2</span><h2 class="grow">{$t('Inventory and batches')}</h2>{@render status(run.inventory.status === 'pending' ? '' : run.inventory.status)}</header>
      <p class="small muted desc">
        {#if run.inventory.problems?.length}{$t('Rejected')}: {run.inventory.problems.slice(0, 3).join('; ')}
        {:else if run.inventory.status === 'accepted'}{$t('{n} assets in {b} batches.', { n: run.inventory.count, b: run.batches.length })}{#if run.inventory.uncertainties?.length} {$t('Uncertain')}: {run.inventory.uncertainties.slice(0, 3).join('; ')}{/if}
        {:else if layoutReady}{$t('Claude lists every distinct asset of the accepted layout; batches are planned automatically.')}
        {:else}{$t('Accept a layout first.')}{/if}</p>
      <div class="row"><button type="button" class="primary" disabled={busy || !layoutReady}
        onclick={() => { if (run.batches.some((b) => b.versions.length) && !confirmed('inventory', $t('Rebuilding the inventory replaces all batches and their sheets.'))) return;
          selected = null; mapgen.call(['inventory', run.id], $t('Building the inventory…')).catch(quiet); }}>
        {armed.inventory ? $t('Click again to confirm') : run.inventory.status === 'accepted' ? $t('Rebuild inventory') : $t('Build inventory')}</button></div>
    </article>

    <article class="card step" class:locked={!run.batches.length}>
      <header class="row"><span class="n">3</span><h2 class="grow">{$t('Tileset sheets')}</h2>{@render status(run.batches.length && accepted === run.batches.length ? 'accepted' : '')}</header>
      <p class="small muted desc">{run.batches.length ? $t('{a} of {n} sheets accepted · {p} not generated yet. Select a batch to review it.', { a: accepted, n: run.batches.length, p: pending }) : $t('Build the inventory first.')}</p>
      <div class="row"><button type="button" class="primary" disabled={busy || !pending} onclick={() => mapgen.call(['sheets', run.id], $t('Generating sheets…')).catch(quiet)}>
        {pending ? $t('Generate {n} pending', { n: pending }) : $t('All generated')}</button></div>
      <div class="batches">
        {#each run.batches as b (b.n)}
          {@const v = b.versions[b.accepted ?? b.versions.length - 1]}
          <button type="button" class="batch" aria-pressed={String(selected === b.n)} onclick={() => { selected = selected === b.n ? null : b.n; }}>
            <span class="thumb checker">{#if v}<AsyncImage path={mapgen.path(v.thumb)} class="pixel" />{/if}</span>
            <strong class="ellipsis">{b.n}. {b.title}</strong>
            <span class="row small"><span class="grow muted">{$t('{n} assets', { n: b.lines.length })}{v?.checks ? (v.checks.pass ? ` · ${$t('checks ok')}` : ` · ${$t('check issues')}`) : ''}</span>{@render status(b.status)}</span>
          </button>
        {/each}
      </div>
      {#if batch}
        <div class="batch-detail">
          <header class="row"><h3 class="grow">{$t('Batch {n} · {title}', { n: batch.n, title: batch.title })}</h3>{@render status(batch.status)}</header>
          {@render versions(batch, `batch${batch.n}`)}
          <ol class="small">{#each batch.lines as line, i}<li class:error-text={missing.has(i + 1)}>{line}</li>{/each}</ol>
          {#if batch.versions.length}
            <details><summary class="small">{$t('Correct this sheet…')}</summary>
              <textarea bind:value={batchCorrection} rows="3" placeholder={$t('What should change, e.g. the trees must have complete crowns')}></textarea>
              <button type="button" disabled={busy || !batchCorrection.trim()} onclick={() => { mapgen.forget(`batch${batch.n}`);
                mapgen.call(['sheet', run.id, String(batch.n), '--correction', batchCorrection.trim()], $t('Applying the correction…')).then(() => (batchCorrection = ''), quiet); }}>{$t('Apply correction')}</button></details>
          {/if}
          <div class="row">
            <button type="button" disabled={busy} onclick={() => { mapgen.forget(`batch${batch.n}`); mapgen.call(['sheet', run.id, String(batch.n)], $t('Generating batch {n}…', { n: batch.n })).catch(quiet); }}>
              {batch.versions.length ? $t('Regenerate') : $t('Generate')}</button>
            <button type="button" class="primary" disabled={busy || !batch.versions.length || idx(batch, `batch${batch.n}`) === batch.accepted}
              onclick={() => mapgen.call(['accept', run.id, 'batch', '--batch', String(batch.n), '--version', String(idx(batch, `batch${batch.n}`) + 1)], $t('Accepting…')).catch(quiet)}>{$t('Accept this version')}</button>
          </div>
        </div>
      {/if}
    </article>
  {/if}
  {#if $mg.log}<details class="card log"><summary class="small">{$t('Activity log')}</summary><pre>{$mg.log}</pre></details>{/if}
  {#if busy}
    <div class="activity" role="status" aria-live="polite"><span class="spin"></span><span class="grow">{$mg.activity}</span>
      <button type="button" onclick={mapgen.stop}>{$t('Stop')}</button></div>
  {/if}
</div>

{#if lightbox}
  <div class="lightbox" role="dialog" aria-modal="true" aria-label={$t('Image preview')} tabindex="-1"
    onclick={(e) => { if (e.target === e.currentTarget) lightbox = null; }} onkeydown={(e) => { if (e.key === 'Escape') lightbox = null; }}>
    <div class="lb"><AsyncImage path={lightbox.path} class="pixel big" />
      <div class="row"><span class="grow small">{lightbox.caption}</span><button type="button" onclick={() => (lightbox = null)}>{$t('Close')}</button></div></div>
  </div>
{/if}

<style>
  .mapgen { flex: 1; padding: 12px 14px; display: flex; flex-direction: column; gap: 12px; }
  .top { gap: 10px; flex-wrap: wrap; }
  .top select { max-width: 520px; }
  .alert { background: var(--err-soft); color: var(--err); border-radius: var(--radius); padding: 8px 12px; white-space: pre-wrap; font-size: 12.5px; }
  .step { padding: 14px; display: grid; gap: 10px; }
  .step.locked { opacity: .6; }
  .n { width: 24px; height: 24px; border-radius: 50%; display: grid; place-items: center; background: var(--accent-soft); color: var(--accent-text); font-weight: 700; font-size: 12px; }
  .desc { line-height: 1.5; }
  .figure { border-radius: var(--radius-sm); padding: 6px; display: grid; place-items: center; }
  .figure :global(img.main) { max-width: 100%; max-height: 340px; object-fit: contain; display: block; }
  .strip { display: flex; gap: 6px; overflow-x: auto; }
  .strip button { padding: 2px; position: relative; height: auto; }
  .strip :global(img) { width: 72px; display: block; }
  .tag { position: absolute; left: 4px; top: 3px; font-size: 10px; background: rgba(0, 0, 0, .7); color: #fff; padding: 0 4px; border-radius: 3px; }
  .checks { display: grid; gap: 2px; }
  details textarea { width: 100%; margin: 6px 0; }
  .settings { display: grid; grid-template-columns: repeat(auto-fill, minmax(190px, 1fr)); gap: 10px; align-items: end; }
  .batches { display: grid; grid-template-columns: repeat(auto-fill, minmax(176px, 1fr)); gap: 8px; }
  .batch { display: grid; gap: 6px; padding: 8px; height: auto; text-align: left; justify-items: stretch; white-space: normal; }
  .thumb { aspect-ratio: 20 / 11; border-radius: 4px; overflow: hidden; display: block; }
  .thumb :global(img) { width: 100%; height: 100%; object-fit: contain; display: block; }
  .batch-detail { border-top: 1px solid var(--border); padding-top: 12px; display: grid; gap: 10px; }
  ol { margin: 0; padding-left: 20px; color: var(--muted); display: grid; gap: 2px; }
  .log { padding: 8px 12px; }
  .log pre { max-height: 180px; overflow: auto; white-space: pre-wrap; margin: 6px 0 0; font-size: 11px; color: var(--muted); }
  .activity { position: sticky; bottom: 0; display: flex; align-items: center; gap: 10px; padding: 8px 12px; border-radius: var(--radius);
    background: var(--accent-soft); color: var(--accent-text); font-size: 12.5px; box-shadow: var(--shadow); }
  .lightbox { position: fixed; inset: 0; z-index: 80; background: rgba(10, 8, 16, .9); display: grid; place-items: center; padding: 20px; }
  .lb { display: grid; gap: 8px; max-width: 100%; color: #ece8f7; }
  .lb :global(img.big) { max-width: 100%; max-height: calc(100vh - 100px); }
</style>
