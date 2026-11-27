<script>
  import Icon from './Icon.svelte';
  import Popover from './Popover.svelte';
  import { emu, project, chat } from '../lib/app.js';
  import { modeSummary } from '../lib/emu.js';
  import { notifications, clearNotifications } from '../lib/notify.js';
  import { t } from '../lib/i18n.js';
  const { status, config } = emu;
  const { operation, build, tests } = project;
  const { busy: agentBusy } = chat;
  const OPERATIONS = { build: 'Building…', test: 'Running tests…', preview: 'Starting preview…', build_and_play: 'Building and booting…' };
  const errors = $derived($notifications.filter((n) => n.kind === 'error').length);
  const time = (d) => d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
</script>

<footer class="statusbar">
  <span class="item mode" data-mode={$status.mode}><span class="dot"></span>{modeSummary($status, $t)}</span>
  {#if $status.loaded}
    <span class="item muted">{$t('Frame {n}', { n: $status.frame_count })}</span>
    <span class="item muted">{$config.prefs.rewind_enabled ? $t('Rewind {n}', { n: $status.rewind_count }) : $t('Rewind off')}</span>
  {/if}
  {#if $operation}<span class="item"><span class="spin"></span>{$t(OPERATIONS[$operation])}</span>{/if}
  {#if $agentBusy}<span class="item"><span class="spin"></span>{$t('Agent working…')}</span>{/if}
  <span class="spacer"></span>
  {#if $build}
    <span class="item" class:error-text={!$build.ok}>
      <Icon name="hammer" size={14} />{$build.ok ? $t('Last build OK ({time})', { time: time($build.at) }) : $t('Last build failed ({time})', { time: time($build.at) })}
    </span>
  {/if}
  {#if $tests?.result}
    <span class="item" class:error-text={!$tests.ok}><Icon name="flask" size={14} />{$tests.result.totals?.TOTAL !== undefined
      ? $t('Tests: {passed}/{total}', { passed: $tests.result.totals.PASSED ?? 0, total: $tests.result.totals.TOTAL })
      : $tests.ok ? $t('Tests passed') : $t('Tests failed')}</span>
  {/if}
  <Popover icon="bell" label="" align="right" badge={errors ? String(errors) : ''} title={$t('Notifications')} buttonClass="ghost bell">
    {#snippet children()}
      <div class="notes">
        <div class="row"><strong class="grow">{$t('Notifications')}</strong>
          {#if $notifications.length}<button type="button" class="ghost" onclick={clearNotifications}>{$t('Clear')}</button>{/if}</div>
        {#each $notifications as note (note.id)}
          <div class="note {note.kind}"><span class="muted small">{time(note.at)}</span> {note.text}</div>
        {:else}
          <p class="muted small">{$t('Nothing to report.')}</p>
        {/each}
      </div>
    {/snippet}
  </Popover>
</footer>

<style>
  .statusbar { display: flex; align-items: center; gap: 14px; height: 30px; padding: 0 8px 0 12px; font-size: 12px;
    background: var(--surface); border-top: 1px solid var(--border); }
  .item { display: inline-flex; align-items: center; gap: 6px; white-space: nowrap; }
  .spacer { flex: 1; }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--muted); }
  .mode[data-mode="playing"] .dot { background: var(--ok); }
  .mode[data-mode="fast_forward"] .dot { background: var(--accent); }
  .mode[data-mode="paused"] .dot { background: var(--warn); }
  .mode[data-mode="agent_held"] .dot { background: var(--warn); }
  .mode[data-mode="agent_held"] { color: var(--warn); font-weight: 600; }
  .statusbar :global(.bell) { min-height: 24px; }
  .notes { display: grid; gap: 6px; max-height: 360px; overflow-y: auto; width: 320px; padding: 4px; }
  .note { font-size: 12px; padding: 6px 8px; border-radius: var(--radius-sm); background: var(--surface-2); overflow-wrap: anywhere; }
  .note.error { background: var(--err-soft); color: var(--err); }
  .note.success { background: var(--ok-soft); color: var(--ok); }
</style>
