<script>
  // The projects in the projects folder, searchable, and any other folder through
  // the system chooser. Opening one restarts Tauros on it.
  import Icon from './Icon.svelte';
  import { projects, chat, navigate } from '../lib/app.js';
  import { t } from '../lib/i18n.js';
  let { prominent = false } = $props();
  const { list, opening } = projects;
  const { project: current, busy: agentBusy } = chat;
  let query = $state('');
  let armed = $state(null);
  let armTimer;

  const shown = $derived($list.items.filter((p) => p.name.toLowerCase().includes(query.trim().toLowerCase())));

  // Opening stops an agent reply in progress, so that needs a second click.
  function open(path) {
    if ($agentBusy && armed !== path) {
      armed = path; clearTimeout(armTimer); armTimer = setTimeout(() => (armed = null), 6000);
      return;
    }
    armed = null;
    projects.open(path);
  }
  async function browse() {
    const path = await projects.chooseFolder(projects.folder(), $t('Open a game project'));
    if (path) open(path);
  }
</script>

<section class="card projects" class:prominent>
  <header class="row">
    <Icon name="folder" /><h2 class="grow">{$t('Projects')}</h2>
    <button type="button" class={prominent ? 'primary' : ''} disabled={!!$opening} onclick={browse}>
      <Icon name="folder" size={15} />{$t('Open from a folder…')}</button>
  </header>
  <p class="small muted ellipsis" title={$list.dir}>{$t('In {dir}', { dir: $list.dir || '—' })} ·
    <button type="button" class="link" onclick={() => navigate('settings')}>{$t('Change the folder')}</button></p>
  {#if $list.items.length > 6}
    <input type="search" bind:value={query} placeholder={$t('Search projects…')} aria-label={$t('Search projects…')} />
  {/if}
  {#if $list.error}
    <p class="small error-text">{$list.error}</p>
  {:else if !$list.items.length && !$list.loading}
    <p class="muted small">{$t('No game projects in this folder. A project is a decomp folder with data/maps/map_groups.json.')}</p>
  {/if}
  <ul>
    {#each shown as p (p.path)}
      <li>
        <span class="grow text"><strong class="ellipsis">{p.name}</strong><span class="muted small ellipsis">{p.path}</span></span>
        <span class="chip {p.built ? 'ok' : 'warn'}">{p.built ? $t('Built') : $t('Not built')}</span>
        {#if p.path === $current}
          <span class="chip accent">{$t('Open now')}</span>
        {:else}
          <button type="button" class={armed === p.path ? 'primary' : ''} disabled={!!$opening} onclick={() => open(p.path)}>
            {#if $opening === p.path}<span class="spin"></span>{/if}
            {armed === p.path ? $t('Click again: the agent’s reply stops') : $t('Open')}</button>
        {/if}
      </li>
    {/each}
  </ul>
  {#if $opening}<p class="small muted" role="status">{$t('Tauros is closing and reopening on the project…')}</p>{/if}
</section>

<style>
  .projects { padding: 14px; display: grid; gap: 10px; align-content: start; min-width: 0; }
  .projects.prominent { max-width: 760px; width: 100%; justify-self: center; }
  header { gap: 8px; }
  ul { list-style: none; margin: 0; padding: 0; display: grid; gap: 4px; max-height: 360px; overflow-y: auto; }
  li { display: flex; align-items: center; gap: 8px; padding: 6px 8px; border-radius: var(--radius-sm); background: var(--surface-2); min-width: 0; }
  .text { display: grid; min-width: 0; }
  .link { border: 0; background: none; padding: 0; min-height: 0; color: var(--accent-text); text-decoration: underline; font-size: inherit; }
  .link:hover:not(:disabled) { background: none; }
</style>
