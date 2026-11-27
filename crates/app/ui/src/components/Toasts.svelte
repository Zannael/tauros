<script>
  import Icon from './Icon.svelte';
  import { toasts, dismissToast } from '../lib/notify.js';
  import { t } from '../lib/i18n.js';
</script>

<div class="toasts" role="status" aria-live="polite">
  {#each $toasts as toast (toast.id)}
    <div class="toast {toast.kind}" role={toast.kind === 'error' ? 'alert' : undefined}>
      <Icon name={toast.kind === 'error' ? 'alert' : toast.kind === 'success' ? 'check' : 'info'} size={16} />
      <span class="grow">{toast.text}</span>
      <button type="button" class="ghost icon" aria-label={$t('Dismiss')} title={$t('Dismiss')} onclick={() => dismissToast(toast.id)}>
        <Icon name="x" size={14} /></button>
    </div>
  {/each}
</div>

<style>
  .toasts { position: fixed; right: 16px; bottom: 40px; z-index: 60; display: grid; gap: 8px; width: min(420px, calc(100vw - 32px)); }
  .toast { display: flex; align-items: flex-start; gap: 8px; padding: 9px 8px 9px 12px; border-radius: var(--radius);
    background: var(--surface); border: 1px solid var(--border-strong); box-shadow: var(--shadow); overflow-wrap: anywhere; font-size: 12.5px; }
  .toast :global(svg) { margin-top: 2px; flex: none; }
  .toast.error { border-color: var(--err); color: var(--err); background: var(--err-soft); }
  .toast.success { color: var(--ok); }
  .toast button { min-height: 22px; width: 22px; }
</style>
