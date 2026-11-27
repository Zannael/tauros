<script>
  // A button that opens a floating panel; closes on outside click or Escape.
  let { label, icon = null, title = '', align = 'right', disabled = false, buttonClass = 'ghost', badge = '',
    trigger = null, children, onprimary = null } = $props();
  import Icon from './Icon.svelte';
  let open = $state(false);
  let root;
  function onWindowClick(e) { if (open && root && !root.contains(e.target)) open = false; }
  function onKey(e) { if (open && e.key === 'Escape') { open = false; e.stopPropagation(); } }
</script>

<svelte:window onclick={onWindowClick} onkeydown={onKey} />
<div class="pop" bind:this={root}>
  <button type="button" class={buttonClass || undefined} class:icon={icon && !label && !badge} {disabled} title={title || label}
    aria-label={title || label} aria-haspopup="true" aria-expanded={open}
    onclick={() => { if (onprimary) { open = false; onprimary(); } else open = !open; }}
    oncontextmenu={(e) => { if (onprimary) { e.preventDefault(); open = true; } }}>
    {#if trigger}{@render trigger()}{:else}
      {#if icon}<Icon name={icon} />{/if}{#if label}<span>{label}</span>{/if}{#if badge}<span class="badge">{badge}</span>{/if}
    {/if}
  </button>
  {#if open}
    <div class="panel {align}" role="menu">
      {@render children({ close: () => (open = false) })}
    </div>
  {/if}
</div>

<style>
  .pop { position: relative; display: inline-flex; }
  .panel { position: absolute; top: calc(100% + 6px); z-index: 40; min-width: 270px; max-width: 360px;
    background: var(--surface); border: 1px solid var(--border-strong); border-radius: var(--radius);
    box-shadow: var(--shadow); padding: 6px; display: grid; gap: 2px; }
  .panel.right { right: 0; }
  .panel.left { left: 0; }
  :global(button.on) { background: var(--accent-soft); border-color: var(--accent); color: var(--accent-text); }
  .badge { font-size: 11px; font-weight: 700; color: var(--accent-text); }
</style>
