<script>
  // Every recorded change with its origin; restoring never deletes history: it
  // snapshots pending work and records a new commit matching the chosen point.
  import { onDestroy } from 'svelte';
  import Icon from '../components/Icon.svelte';
  import { history, chat, viewActions } from '../lib/app.js';
  import { t } from '../lib/i18n.js';
  const { entries, state: hs } = history;
  const { busy: agentBusy, project: path } = chat;
  const FILTERS = [['all', 'All'], ['human', 'Manual'], ['agent', 'Agent'], ['safety', 'Snapshots and restores'], ['external', 'Outside Tauros']];
  const group = (e) => {
    const o = e.origin;
    if (o === 'human') return 'human';
    if (o === 'agent-generated' || o === 'agent-assisted') return 'agent';
    if (o === 'snapshot' || o === 'undo') return 'safety';
    return 'external';
  };
  const ORIGIN = { 'agent-generated': 'AI generated', 'agent-assisted': 'AI assisted', human: 'Manual', snapshot: 'Safety snapshot', undo: 'Restore', external: 'Outside Tauros' };
  let filter = $state('all');
  let selected = $state(null);
  let armed = $state(false);
  let armTimer;

  const shown = $derived($entries.filter((e) => filter === 'all' || group(e) === filter));
  const current = $derived($entries.find((e) => e.commit === selected) || shown[0] || null);
  const when = (ts) => { const d = new Date(ts); return Number.isNaN(d.getTime()) ? ts : d.toLocaleString(); };

  $effect(() => {
    viewActions.set([
      { id: 'provenance', overflow: true, icon: 'download', label: $t('Export provenance labels'),
        hint: $t('Work in progress ({task})', { task: 'PROV-001' }), disabled: true, run: () => {} },
    ]);
  });
  onDestroy(() => { viewActions.set([]); clearTimeout(armTimer); });
  let loaded = false;
  $effect(() => { if ($path && !loaded) { loaded = true; history.load(); } });

  function select(entry) { selected = entry.commit; armed = false; }
  function restore() {
    if (!armed) { armed = true; clearTimeout(armTimer); armTimer = setTimeout(() => (armed = false), 6000); return; }
    armed = false;
    history.restore(current);
  }
</script>

{#if !$path}
  <div class="empty pad"><Icon name="history" size={30} /><h2>{$t('No game project is open')}</h2>
    <p>{$t('History lists the changes recorded in a game project.')}</p></div>
{:else}
  <div class="history">
    <section class="list card">
      <div class="filters" role="group" aria-label={$t('Filter changes')}>
        {#each FILTERS as [id, label]}<button type="button" class="chip-btn" aria-pressed={String(filter === id)} onclick={() => (filter = id)}>{$t(label)}</button>{/each}
      </div>
      {#if $hs.error}<p class="error-text small pad-x">{$hs.error}</p>{/if}
      <ul class="scroll" aria-label={$t('Changes')}>
        {#each shown as e (e.commit)}
          <li>
            <button type="button" class="entry ghost" aria-current={current?.commit === e.commit ? 'true' : undefined} onclick={() => select(e)}>
              <span class="dot {group(e)}"></span>
              <span class="grow text"><span class="ellipsis summary">{e.summary}</span>
                <span class="muted small">{$t(ORIGIN[e.origin] || e.origin)} · {when(e.timestamp)}</span></span>
              <span class="muted small mono">{e.short_commit}</span>
            </button>
          </li>
        {:else}
          <li class="empty">{$hs.loading ? $t('Loading…') : $t('No changes match this filter.')}</li>
        {/each}
      </ul>
    </section>

    <section class="detail card">
      {#if current}
        <header><h2>{current.summary}</h2>
          <div class="row"><span class="chip {group(current) === 'agent' ? 'accent' : ''}">{$t(ORIGIN[current.origin] || current.origin)}</span>
            {#if current.provenance}<span class="chip">{current.provenance}</span>{/if}</div></header>
        <dl>
          <dt>{$t('When')}</dt><dd>{when(current.timestamp)}</dd>
          <dt>{$t('Commit')}</dt><dd class="mono">{current.commit}</dd>
          {#if current.snapshot_commit}<dt>{$t('Safety snapshot')}</dt><dd class="mono">{current.snapshot_commit}</dd>{/if}
          {#if current.undo_target}<dt>{$t('Restored point')}</dt><dd class="mono">{current.undo_target}</dd>{/if}
        </dl>
        <div class="restore">
          <h3>{$t('Restore to this point')}</h3>
          <p class="small muted">{$t('The project files go back to how they were right after this change. Nothing is deleted: pending work is saved in a snapshot first and the restore is recorded as a new change you can undo. Build or preview to see it in the game.')}</p>
          <div class="row">
            <button type="button" class={armed ? 'primary' : ''} disabled={!!$hs.restoring || $agentBusy} onclick={restore}>
              {#if $hs.restoring === current.commit}<span class="spin"></span>{:else}<Icon name="history" size={16} />{/if}
              {armed ? $t('Click again to restore {sha}', { sha: current.short_commit }) : $t('Restore to this point')}</button>
            {#if $agentBusy}<span class="small warn-text">{$t('Wait for the agent to finish before restoring.')}</span>{/if}
          </div>
        </div>
      {:else}
        <div class="empty">{$t('Select a change to see its details.')}</div>
      {/if}
    </section>
  </div>
{/if}

<style>
  .pad { flex: 1; align-content: center; }
  .history { flex: 1; min-height: 0; display: grid; grid-template-columns: minmax(320px, 1fr) minmax(320px, 1.1fr); gap: 12px; padding: 12px; }
  .list { display: flex; flex-direction: column; min-height: 0; }
  .filters { display: flex; flex-wrap: wrap; gap: 6px; padding: 10px; border-bottom: 1px solid var(--border); }
  .chip-btn { min-height: 26px; border-radius: 999px; font-size: 12px; }
  .pad-x { padding: 6px 10px; }
  ul { list-style: none; margin: 0; padding: 6px; flex: 1; }
  .entry { width: 100%; justify-content: flex-start; gap: 10px; min-height: 48px; padding: 4px 8px; text-align: left; }
  .entry[aria-current="true"] { background: var(--accent-soft); }
  .text { display: grid; min-width: 0; }
  .summary { font-weight: 600; }
  .dot { width: 9px; height: 9px; border-radius: 50%; flex: none; background: var(--muted); }
  .dot.agent { background: var(--accent); }
  .dot.human { background: var(--ok); }
  .dot.safety { background: var(--border-strong); }
  .detail { padding: 16px; display: grid; gap: 14px; align-content: start; overflow-y: auto; }
  .detail header { display: grid; gap: 8px; }
  dl { display: grid; grid-template-columns: auto 1fr; gap: 6px 14px; margin: 0; font-size: 12.5px; }
  dt { color: var(--muted); }
  dd { margin: 0; overflow-wrap: anywhere; }
  .restore { display: grid; gap: 8px; padding: 12px; border-radius: var(--radius); background: var(--surface-2); }
  @media (max-width: 1100px) { .history { grid-template-columns: 1fr; } }
</style>
