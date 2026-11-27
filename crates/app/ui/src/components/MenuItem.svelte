<script>
  import Icon from './Icon.svelte';
  let { label, icon = null, onclick, disabled = false, checked = undefined, hint = '', title = '' } = $props();
</script>

<button type="button" class="item" role={checked === undefined ? 'menuitem' : 'menuitemcheckbox'}
  aria-checked={checked === undefined ? undefined : String(checked)} {disabled} {title} {onclick}>
  <span class="mark">{#if checked}<Icon name="check" size={15} />{:else if icon}<Icon name={icon} size={15} />{/if}</span>
  <span class="label">{label}</span>
  {#if hint}<span class="hint">{hint}</span>{/if}
</button>

<style>
  .item { width: 100%; justify-content: flex-start; border: 0; background: transparent; min-height: 30px; padding: 0 8px; gap: 8px; text-align: left; }
  .item:hover:not(:disabled) { background: var(--surface-2); border: 0; }
  .mark { width: 16px; display: inline-flex; color: var(--accent-text); flex: none; }
  .label { flex: 1; white-space: normal; }
  .hint { color: var(--muted); font-size: 11.5px; }
</style>
