<script>
  import Icon from './Icon.svelte';
  import { VIEWS, view, navigate } from '../lib/app.js';
  import { t } from '../lib/i18n.js';
</script>

<nav class="sidebar" aria-label={$t('Views')}>
  {#each VIEWS as item (item.id)}
    <button type="button" class="nav ghost" aria-current={$view === item.id ? 'page' : undefined}
      aria-disabled={item.wip ? 'true' : undefined}
      title={item.wip ? $t('Work in progress ({task})', { task: item.wip }) : $t(item.label)}
      onclick={() => navigate(item.id)}>
      <Icon name={item.icon} size={18} />
      <span class="label">{$t(item.label)}</span>
      {#if item.wip}<span class="wip">WIP</span>{/if}
    </button>
  {/each}
</nav>

<style>
  .sidebar { display: flex; flex-direction: column; gap: 2px; padding: 10px 8px; border-right: 1px solid var(--border);
    background: var(--surface); width: 196px; overflow-y: auto; }
  .nav { justify-content: flex-start; gap: 10px; min-height: 36px; padding: 0 10px; color: var(--muted); font-weight: 500; }
  .nav[aria-current="page"] { background: var(--accent-soft); color: var(--accent-text); font-weight: 650; }
  .nav:hover:not([aria-disabled="true"]) { color: var(--text); }
  .nav[aria-disabled="true"] { opacity: .62; cursor: help; }
  .label { flex: 1; text-align: left; overflow: hidden; text-overflow: ellipsis; }
  .wip { font-size: 10px; font-weight: 700; letter-spacing: .03em; padding: 0 5px; border-radius: 4px;
    background: var(--surface-2); color: var(--muted); border: 1px solid var(--border); }
  @media (max-width: 1180px) {
    .sidebar { width: 56px; padding: 10px 6px; }
    .label, .wip { display: none; }
    .nav { justify-content: center; padding: 0; }
  }
</style>
