<script>
  // Ctrl+K: searches the navigation, the panel toggles and the actions the active
  // view declares. It runs them; it never defines its own.
  import Icon from './Icon.svelte';
  import { VIEWS, navigate, paletteOpen, viewActions, toggleAgent, miniGame } from '../lib/app.js';
  import { t } from '../lib/i18n.js';
  let query = $state('');
  let index = $state(0);
  let input = $state(null);

  const commands = $derived([
    ...$viewActions.flatMap((a) => a.commands || (a.snippet || a.separatorOnly ? [] : [a]))
      .map((a) => ({ id: `a:${a.id}`, label: a.label, hint: a.hint || '', icon: a.icon, disabled: a.disabled, run: a.run })),
    ...VIEWS.map((v) => ({ id: `v:${v.id}`, label: $t('Go to {view}', { view: $t(v.label) }), icon: v.icon,
      hint: v.wip ? $t('Work in progress ({task})', { task: v.wip }) : '', disabled: !!v.wip, run: () => navigate(v.id) })),
    { id: 'g:agent', label: $t('Show or hide the agent panel'), icon: 'panel', hint: 'Ctrl+J', run: toggleAgent },
    { id: 'g:game', label: $t('Show or hide the game here'), icon: 'game', run: () => miniGame.update((v) => !v) },
  ]);
  const matches = $derived(commands.filter((c) => c.label.toLowerCase().includes(query.trim().toLowerCase())));

  $effect(() => { if ($paletteOpen) { query = ''; index = 0; queueMicrotask(() => input?.focus()); } });
  $effect(() => { if (index >= matches.length) index = Math.max(0, matches.length - 1); });

  function run(cmd) {
    if (!cmd || cmd.disabled) return;
    paletteOpen.set(false);
    cmd.run();
  }
  function onKey(e) {
    if (e.key === 'ArrowDown') { e.preventDefault(); index = Math.min(index + 1, matches.length - 1); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); index = Math.max(index - 1, 0); }
    else if (e.key === 'Enter') { e.preventDefault(); run(matches[index]); }
    else if (e.key === 'Escape') { e.preventDefault(); paletteOpen.set(false); }
  }
</script>

{#if $paletteOpen}
  <div class="backdrop" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) paletteOpen.set(false); }}>
    <div class="palette" role="dialog" aria-modal="true" aria-label={$t('Search actions')}>
      <div class="search"><Icon name="command" size={16} />
        <input bind:this={input} bind:value={query} onkeydown={onKey} placeholder={$t('Type an action or a view…')}
          aria-label={$t('Search actions')} role="combobox" aria-expanded="true" aria-controls="palette-list"
          aria-activedescendant={matches[index] ? `pal-${index}` : undefined} />
      </div>
      <ul id="palette-list" role="listbox">
        {#each matches as cmd, i (cmd.id)}
          <!-- Options are chosen with the arrow keys and Enter in the search field (combobox pattern). -->
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <li id="pal-{i}" role="option" aria-selected={i === index} aria-disabled={cmd.disabled ? 'true' : undefined}
            class:active={i === index} onmousemove={() => (index = i)} onclick={() => run(cmd)}>
            {#if cmd.icon}<Icon name={cmd.icon} size={16} />{/if}
            <span class="grow">{cmd.label}</span>{#if cmd.hint}<span class="muted small">{cmd.hint}</span>{/if}
          </li>
        {:else}
          <li class="muted none">{$t('No matching action.')}</li>
        {/each}
      </ul>
    </div>
  </div>
{/if}

<style>
  .backdrop { position: fixed; inset: 0; z-index: 70; background: rgba(15, 10, 30, .35); display: grid; justify-items: center; align-items: start; padding-top: 12vh; }
  .palette { width: min(620px, calc(100vw - 32px)); background: var(--surface); border: 1px solid var(--border-strong);
    border-radius: 12px; box-shadow: var(--shadow); overflow: hidden; }
  .search { display: flex; align-items: center; gap: 10px; padding: 10px 14px; border-bottom: 1px solid var(--border); color: var(--muted); }
  .search input { flex: 1; border: 0 !important; background: transparent !important; font-size: 15px; padding: 4px 0 !important; color: var(--text); }
  .search input:focus-visible { outline: none; }
  ul { list-style: none; margin: 0; padding: 6px; max-height: 50vh; overflow-y: auto; }
  li { display: flex; align-items: center; gap: 10px; padding: 8px 10px; border-radius: var(--radius-sm); cursor: pointer; }
  li.active { background: var(--accent-soft); color: var(--accent-text); }
  li[aria-disabled="true"] { opacity: .6; cursor: not-allowed; }
  .none { cursor: default; }
</style>
