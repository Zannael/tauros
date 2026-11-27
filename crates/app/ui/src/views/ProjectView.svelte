<script>
  // The open project at a glance: build with readable diagnostics, headless tests,
  // the built ROM and the latest recorded changes.
  import { onDestroy } from 'svelte';
  import Icon from '../components/Icon.svelte';
  import { chat, project, history, navigate, viewActions } from '../lib/app.js';
  import { t } from '../lib/i18n.js';
  const { project: path, busy: agentBusy } = chat;
  const { info, operation, build, tests, live, point } = project;
  const { entries } = history;
  let filter = $state('');
  let showPassed = $state(false);

  const name = $derived($path ? $path.split(/[\\/]/).filter(Boolean).pop() : '');
  const blocked = $derived(!!$operation || $agentBusy || !$info);

  $effect(() => {
    viewActions.set([
      { id: 'build', primary: true, icon: 'hammer', label: $t('Build'), busy: $operation === 'build', disabled: blocked,
        title: $t('Validate and build the ROM (a snapshot is recorded first)'), run: project.runBuild },
      { id: 'test', icon: 'flask', label: $t('Run tests'), busy: $operation === 'test', disabled: blocked,
        title: $t('Run the headless test suite'), run: () => project.runTests(filter.trim()) },
      { id: 'provenance', overflow: true, icon: 'download', label: $t('Export provenance labels'),
        hint: $t('Work in progress ({task})', { task: 'PROV-001' }), disabled: true, run: () => {} },
    ]);
  });
  onDestroy(() => viewActions.set([]));
  let loaded = false;
  $effect(() => { if ($info && !loaded) { loaded = true; history.load(); project.refreshLive(); } });

  const time = (d) => d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
  const diagnostics = $derived($build?.result?.diagnostics || []);
  const cases = $derived($tests?.result?.cases || []);
  const failed = $derived(cases.filter((c) => !c.passed));
  const passed = $derived(cases.filter((c) => c.passed));
  const totals = $derived(Object.entries($tests?.result?.totals || {}));
</script>

{#if !$info}
  <div class="empty pad">
    <Icon name="folder" size={30} />
    <h2>{$t('No game project is open')}</h2>
    <p>{$t('Start Tauros with --project and a boot point to build, test and edit a game. You can still play a ROM in the Emulator view.')}</p>
    <pre class="mono cmd">cargo run -p tauros-app -- --project path/to/project --map MAP_PETALBURG_CITY --x 15 --y 10</pre>
    <button type="button" class="primary" onclick={() => navigate('emulator')}><Icon name="gamepad" size={16} />{$t('Open the Emulator view')}</button>
  </div>
{:else}
  <div class="grid scroll">
    <section class="card summary">
      <div class="row"><Icon name="folder" /><h2 class="grow ellipsis">{name}</h2></div>
      <p class="muted small mono path">{$path}</p>
      <dl>
        <dt>{$t('Maps')}</dt><dd>{$info.maps.length}</dd>
        <dt>{$t('Boot point')}</dt><dd>{$point.map} ({$point.x}, {$point.y})</dd>
        <dt>{$t('Game in the emulator')}</dt>
        <dd>{#if !$live}—{:else if $live.live_status === 'stale_rom'}<span class="warn-text">{$t('older than the last build')}</span>
          {:else if $live.new_build_pending}<span class="warn-text">{$t('a newer build is ready')}</span>
          {:else if $live.live_status === 'no_rom'}{$t('not started')}{:else}{$t('up to date')}{/if}</dd>
      </dl>
      <button type="button" onclick={() => navigate('emulator')}><Icon name="gamepad" size={16} />{$t('Play in the Emulator view')}</button>
    </section>

    <section class="card">
      <header class="row"><Icon name="hammer" /><h2 class="grow">{$t('Build')}</h2>
        {#if $operation === 'build'}<span class="chip accent"><span class="spin"></span>{$t('Building…')}</span>
        {:else if $build}<span class="chip {$build.ok && !$build.result?.counts?.errors ? 'ok' : 'err'}">{$build.ok ? $t('Succeeded') : $t('Failed')} · {time($build.at)}</span>{/if}
      </header>
      {#if !$build}
        <p class="muted">{$t('No build in this session yet. Build validates the project, records a snapshot and compiles the ROM; the game keeps running until you preview the new build.')}</p>
      {:else if $build.error}
        <pre class="out error-text">{$build.error}</pre>
      {:else}
        <p class="small">{$t('{errors} errors · {warnings} warnings', { errors: $build.result.counts?.errors ?? 0, warnings: $build.result.counts?.warnings ?? 0 })}
          {#if $build.result.counts?.omitted}· {$t('{n} more not listed', { n: $build.result.counts.omitted })}{/if}</p>
        {#if $build.result.rom_sha256}<p class="small muted mono">ROM {$build.result.rom_sha256.slice(0, 16)}…</p>{/if}
        <ul class="diags">
          {#each diagnostics as d, i (i)}
            <li class={d.severity === 'warning' ? 'warn' : 'err'}>
              <Icon name="alert" size={14} />
              <span class="grow">{#if d.file}<code>{d.file}{d.line ? `:${d.line}` : ''}</code> {/if}{d.message}</span>
            </li>
          {/each}
        </ul>
      {/if}
    </section>

    <section class="card">
      <header class="row"><Icon name="flask" /><h2 class="grow">{$t('Tests')}</h2>
        {#if $operation === 'test'}<span class="chip accent"><span class="spin"></span>{$t('Running…')}</span>
        {:else if $tests}<span class="chip {$tests.ok ? 'ok' : 'err'}">{$tests.ok ? $t('Passed') : $t('Failed')} · {time($tests.at)}</span>{/if}
      </header>
      <form class="row" onsubmit={(e) => { e.preventDefault(); project.runTests(filter.trim()); }}>
        <input class="grow" bind:value={filter} placeholder={$t('Optional filter: test name or .c file')} aria-label={$t('Test filter')} />
        <button type="submit" disabled={blocked}>{$t('Run tests')}</button>
      </form>
      {#if $tests?.error}<pre class="out error-text">{$tests.error}</pre>
      {:else if $tests}
        <p class="small">{#each totals as [k, v], i}{i ? ' · ' : ''}{k}: {v}{/each}{#if $tests.result.omitted_cases} · {$t('{n} more not listed', { n: $tests.result.omitted_cases })}{/if}</p>
        {#each failed as c (c.name)}
          <details class="case" open>
            <summary><Icon name="alert" size={14} /><span class="grow">{c.name}</span><span class="error-text small">{c.result}</span></summary>
            {#if c.location}<p class="small mono muted">{c.location[0]}:{c.location[1]}</p>{/if}
            {#if c.output}<pre class="out">{c.output}</pre>{/if}
          </details>
        {/each}
        {#if passed.length}
          <button type="button" class="ghost small" onclick={() => (showPassed = !showPassed)}>
            {showPassed ? $t('Hide {n} passed', { n: passed.length }) : $t('Show {n} passed', { n: passed.length })}</button>
          {#if showPassed}<ul class="passed small">{#each passed as c (c.name)}<li><Icon name="check" size={13} /> {c.name}</li>{/each}</ul>{/if}
        {/if}
      {:else}
        <p class="muted">{$t('Run the project’s headless tests (pokeemerald-expansion test runner). This can take a few minutes.')}</p>
      {/if}
    </section>

    <section class="card">
      <header class="row"><Icon name="history" /><h2 class="grow">{$t('Recent changes')}</h2>
        <button type="button" class="ghost" onclick={() => navigate('history')}>{$t('Open History')}</button></header>
      <ul class="recent">
        {#each $entries.slice(0, 6) as e (e.commit)}
          <li><span class="chip">{e.provenance || e.origin}</span><span class="grow ellipsis">{e.summary}</span><span class="muted small mono">{e.short_commit}</span></li>
        {:else}<li class="muted">{$t('No changes recorded yet.')}</li>{/each}
      </ul>
    </section>
  </div>
{/if}

<style>
  .pad { flex: 1; align-content: center; }
  .pad p { max-width: 520px; }
  .cmd { background: var(--surface-2); padding: 8px 12px; border-radius: var(--radius-sm); white-space: pre-wrap; max-width: 640px; }
  .grid { flex: 1; padding: 14px; display: grid; grid-template-columns: repeat(auto-fit, minmax(380px, 1fr)); gap: 14px; align-content: start; }
  .card { padding: 14px; display: grid; gap: 10px; align-content: start; min-width: 0; }
  .card header { gap: 8px; }
  .path { overflow-wrap: anywhere; }
  dl { display: grid; grid-template-columns: auto 1fr; gap: 6px 14px; margin: 0; }
  dt { color: var(--muted); }
  dd { margin: 0; overflow-wrap: anywhere; }
  .summary button { justify-self: start; }
  .diags { list-style: none; margin: 0; padding: 0; display: grid; gap: 4px; max-height: 320px; overflow-y: auto; }
  .diags li { display: flex; gap: 8px; align-items: flex-start; padding: 6px 8px; border-radius: var(--radius-sm); font-size: 12.5px; overflow-wrap: anywhere; }
  .diags li.err { background: var(--err-soft); color: var(--err); }
  .diags li.warn { background: var(--warn-soft); color: var(--warn); }
  .diags :global(svg) { margin-top: 2px; flex: none; }
  .out { margin: 0; padding: 8px; background: var(--surface-2); border-radius: var(--radius-sm); max-height: 260px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; font-size: 11.5px; }
  .case { border: 1px solid var(--border); border-radius: var(--radius-sm); padding: 6px 8px; }
  .case summary { display: flex; gap: 8px; align-items: center; cursor: pointer; color: var(--err); font-weight: 600; }
  .passed { list-style: none; margin: 0; padding: 0; display: grid; gap: 2px; color: var(--ok); max-height: 220px; overflow-y: auto; }
  .recent { list-style: none; margin: 0; padding: 0; display: grid; gap: 6px; }
  .recent li { display: flex; gap: 8px; align-items: center; min-width: 0; }
</style>
