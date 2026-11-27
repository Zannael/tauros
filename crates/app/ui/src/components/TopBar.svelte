<script>
  import Icon from './Icon.svelte';
  import logo from '../assets/logo.png';
  import { chat, project, emu, view, miniGame, paletteOpen, agentOpen, toggleAgent } from '../lib/app.js';
  import { t } from '../lib/i18n.js';
  const { project: path } = chat;
  const { build, tests } = project;
  const { status } = emu;
  const name = $derived($path ? $path.split(/[\\/]/).filter(Boolean).pop() : '');
</script>

<header class="topbar">
  <div class="brand"><img src={logo} alt="" width="28" height="28" class="pixel" /><strong>Tauros</strong></div>
  <div class="project" title={$path || ''}>
    <Icon name="folder" size={16} />
    {#if name}<span class="ellipsis">{name}</span>
    {:else}<span class="muted ellipsis">{$status.rom_name ? $t('ROM only · no project') : $t('No project open')}</span>{/if}
  </div>
  {#if name}
    <div class="health" aria-label={$t('Project health')}>
      {#if $build}
        <span class="chip {$build.ok ? 'ok' : 'err'}" title={$build.error || ''}>
          <Icon name={$build.ok ? 'check' : 'alert'} size={13} />{$build.ok ? $t('Build OK') : $t('Build failed')}
          {#if $build.result?.counts?.warnings}· {$t('{n} warnings', { n: $build.result.counts.warnings })}{/if}
        </span>
      {:else}<span class="chip">{$t('Not built in this session')}</span>{/if}
      {#if $tests}
        <span class="chip {$tests.ok ? 'ok' : 'err'}">
          <Icon name="flask" size={13} />{$tests.ok ? $t('Tests passed') : $t('Tests failed')}
        </span>
      {/if}
    </div>
  {/if}
  <div class="spacer"></div>
  <button type="button" class="search" onclick={() => paletteOpen.set(true)} title={$t('Search actions')}>
    <Icon name="command" size={15} /><span>{$t('Search actions…')}</span><kbd>Ctrl K</kbd>
  </button>
  {#if $view !== 'emulator'}
    <button type="button" class="ghost icon" aria-pressed={String($miniGame)} onclick={() => miniGame.update((v) => !v)}
      title={$t('Show the game here')} aria-label={$t('Show the game here')}><Icon name="game" /></button>
  {/if}
  <button type="button" class="ghost icon" aria-pressed={String($agentOpen)} onclick={toggleAgent}
    title={$t('Agent panel (Ctrl+J)')} aria-label={$t('Agent panel (Ctrl+J)')}><Icon name="panel" /></button>
</header>

<style>
  .topbar { display: flex; align-items: center; gap: 14px; height: 48px; padding: 0 12px; background: var(--surface);
    border-bottom: 1px solid var(--border); }
  .brand { display: flex; align-items: center; gap: 8px; font-size: 15px; width: 172px; flex: none; }
  .brand strong { color: var(--accent-text); }
  .project { display: flex; align-items: center; gap: 7px; font-weight: 600; min-width: 0; max-width: 260px;
    padding-left: 14px; border-left: 1px solid var(--border); height: 24px; }
  .health { display: flex; gap: 6px; min-width: 0; overflow: hidden; }
  .spacer { flex: 1; }
  .search { color: var(--muted); background: var(--surface-2); min-width: 220px; justify-content: flex-start; }
  .search span { flex: 1; text-align: left; }
  @media (max-width: 1180px) {
    .brand { width: auto; }
    .search { min-width: 0; }
    .search span { display: none; }
  }
</style>
