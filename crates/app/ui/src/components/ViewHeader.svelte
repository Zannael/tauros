<script>
  // The active view's title and the actions it declares: buttons in order, custom
  // controls rendered by the view, and an overflow menu for the rest.
  import Icon from './Icon.svelte';
  import Popover from './Popover.svelte';
  import MenuItem from './MenuItem.svelte';
  import { viewActions } from '../lib/app.js';
  import { t } from '../lib/i18n.js';
  let { title } = $props();
  const visible = $derived($viewActions.filter((a) => !a.overflow));
  const overflow = $derived($viewActions.filter((a) => a.overflow));
</script>

<header class="view-header">
  <h1>{title}</h1>
  <div class="actions">
    {#each visible as action (action.id)}
      {#if action.snippet}
        {@render action.snippet()}
      {:else}
        <button type="button" class:primary={action.primary} class:icon={action.iconOnly}
          disabled={action.disabled} aria-pressed={action.pressed === undefined ? undefined : String(action.pressed)}
          title={action.title || action.label} aria-label={action.label} onclick={action.run}>
          {#if action.busy}<span class="spin"></span>{:else if action.icon}<Icon name={action.icon} />{/if}
          {#if !action.iconOnly}<span>{action.label}</span>{/if}
        </button>
      {/if}
    {/each}
    {#if overflow.length}
      <Popover icon="more" label="" title={$t('More actions')}>
        {#snippet children({ close })}
          {#each overflow as action (action.id)}
            {#if action.separator}<hr />{/if}
            <MenuItem label={action.label} icon={action.icon} disabled={action.disabled} checked={action.checked}
              title={action.title} hint={action.hint} onclick={() => { close(); action.run(); }} />
          {/each}
        {/snippet}
      </Popover>
    {/if}
  </div>
</header>

<style>
  .view-header { display: flex; align-items: center; gap: 12px; padding: 10px 16px; border-bottom: 1px solid var(--border);
    background: var(--surface); min-height: 52px; flex-wrap: wrap; }
  h1 { flex: 1; min-width: 120px; }
  .actions { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; justify-content: flex-end; }
  hr { border: 0; border-top: 1px solid var(--border); margin: 4px 2px; width: 100%; }
</style>
