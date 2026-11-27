<script>
  // The game frame at an integer scale: "auto" picks the largest multiple that fits
  // the space, a fixed scale is kept unless it would overflow. Without the aspect
  // lock the frame stretches to fill the space.
  import { onMount } from 'svelte';
  import { game, emu } from '../lib/app.js';
  import { t } from '../lib/i18n.js';
  let { scale = 'auto', aspectLocked = true, compact = false } = $props();
  const { status } = emu;
  let box, canvas;
  let avail = $state({ w: 240, h: 160 });

  onMount(() => {
    const remove = game.addScreen(canvas.getContext('2d'));
    const observer = new ResizeObserver(([entry]) => { avail = { w: entry.contentRect.width, h: entry.contentRect.height }; });
    observer.observe(box);
    return () => { remove(); observer.disconnect(); };
  });

  const fit = $derived(Math.max(1, Math.floor(Math.min(avail.w / 240, avail.h / 160))));
  const factor = $derived(scale === 'auto' ? fit : Math.min(Number.parseInt(scale, 10) || fit, fit));
  const size = $derived(aspectLocked ? { w: 240 * factor, h: 160 * factor } : { w: Math.max(240, avail.w), h: Math.max(160, avail.h) });
  const badge = $derived($status.mode === 'agent_held' ? $t('Agent held · preview paused')
    : $status.mode === 'paused' ? $t('Paused · frame {n}', { n: $status.frame_count }) : '');
</script>

<div class="screen-box" class:compact bind:this={box}>
  <div class="frame" style:width="{size.w}px" style:height="{size.h}px">
    <canvas bind:this={canvas} width="240" height="160" tabindex="0" aria-label={$t('Game screen')}
      onmousedown={() => canvas.focus()} onclick={game.ensureAudio}></canvas>
    {#if badge}<div class="badge" data-mode={$status.mode}>{badge}</div>{/if}
    {#if !$status.loaded}<div class="idle">{$t('No game running')}</div>{/if}
  </div>
</div>

<style>
  .screen-box { flex: 1; min-height: 0; min-width: 0; display: grid; place-items: center; overflow: hidden; }
  .frame { position: relative; }
  canvas { width: 100%; height: 100%; display: block; image-rendering: pixelated; background: var(--game-bg); border-radius: 4px; }
  canvas:focus { outline: none; }
  canvas:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; }
  .badge { position: absolute; top: 8px; right: 8px; padding: 3px 10px; border-radius: 999px; font-size: 11.5px; font-weight: 650;
    background: rgba(13, 11, 18, .85); color: #f2c063; border: 1px solid #f2c06388; pointer-events: none; }
  .idle { position: absolute; inset: 0; display: grid; place-items: center; color: #aaa2c1; font-size: 13px; pointer-events: none; }
  .compact .badge { top: 4px; right: 4px; font-size: 10.5px; }
</style>
