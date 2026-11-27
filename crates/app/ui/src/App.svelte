<script>
  import { onMount } from 'svelte';
  import TopBar from './components/TopBar.svelte';
  import Sidebar from './components/Sidebar.svelte';
  import ViewHeader from './components/ViewHeader.svelte';
  import StatusBar from './components/StatusBar.svelte';
  import AgentPanel from './components/AgentPanel.svelte';
  import CommandPalette from './components/CommandPalette.svelte';
  import Toasts from './components/Toasts.svelte';
  import MiniGame from './components/MiniGame.svelte';
  import ProjectView from './views/ProjectView.svelte';
  import EmulatorView from './views/EmulatorView.svelte';
  import HistoryView from './views/HistoryView.svelte';
  import MapgenView from './views/MapgenView.svelte';
  import SettingsView from './views/SettingsView.svelte';
  import { VIEWS, view, miniGame, paletteOpen, agentOpen, toggleAgent, emu, start } from './lib/app.js';
  import { isTextField } from './lib/keys.js';
  import { pickFile, ACCEPT } from './lib/files.js';
  import { uiPrefs } from './lib/prefs.js';
  import { t } from './lib/i18n.js';

  const title = $derived($t(VIEWS.find((v) => v.id === $view)?.label || ''));

  $effect(() => {
    const root = document.documentElement;
    if ($uiPrefs.theme === 'system') root.removeAttribute('data-theme'); else root.dataset.theme = $uiPrefs.theme;
    root.lang = $uiPrefs.language;
  });

  onMount(() => { start(); });

  // Global shortcuts. Emulator shortcuts work from every view but never while a
  // text field has the keyboard.
  function onKey(e) {
    const mod = e.ctrlKey || e.metaKey;
    if (mod && !e.altKey && e.key.toLowerCase() === 'k') { e.preventDefault(); paletteOpen.update((v) => !v); return; }
    if (mod && !e.altKey && e.key.toLowerCase() === 'j') { e.preventDefault(); toggleAgent(); return; }
    if ($paletteOpen || isTextField(e.target) || e.defaultPrevented) return;
    if (mod && !e.altKey && !e.shiftKey) {
      const k = e.key.toLowerCase();
      const act = { o: async () => { const f = await pickFile(ACCEPT.rom); if (f) emu.loadRom(f); },
        p: emu.togglePause, n: emu.step, r: emu.reset, b: emu.rewind }[k];
      if (act) { e.preventDefault(); act(); }
      return;
    }
    if (e.shiftKey && e.key === 'Tab') { e.preventDefault(); emu.toggleFastForward(); return; }
    if (e.key === 'F12') { e.preventDefault(); emu.screenshot(); }
  }
</script>

<svelte:window onkeydown={onKey} />
<div class="shell">
  <TopBar />
  <div class="middle">
    <Sidebar />
    <main class="main">
      <ViewHeader {title} />
      <div class="content">
        {#if $view === 'project'}<ProjectView />
        {:else if $view === 'emulator'}<EmulatorView />
        {:else if $view === 'history'}<HistoryView />
        {:else if $view === 'mapgen'}<MapgenView />
        {:else if $view === 'settings'}<SettingsView />{/if}
        {#if $miniGame && $view !== 'emulator'}<MiniGame />{/if}
      </div>
    </main>
    {#if $agentOpen}<AgentPanel />{/if}
  </div>
  <StatusBar />
</div>
<CommandPalette />
<Toasts />

<style>
  .shell { height: 100%; display: flex; flex-direction: column; }
  .middle { flex: 1; min-height: 0; display: flex; }
  .main { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .content { flex: 1; min-height: 0; position: relative; display: flex; flex-direction: column; }
</style>
