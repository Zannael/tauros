<script>
  // An image fetched through the map generator backend (data URL, cached).
  import { mapgen } from '../lib/app.js';
  let { path, alt = '', class: klass = '', title = '', onclick = null } = $props();
  let src = $state('');
  $effect(() => { const p = path; src = ''; mapgen.image(p).then((url) => { if (p === path) src = url; }); });
</script>

{#if onclick}
  <button type="button" class="img-btn" {title} {onclick}><img {src} {alt} class={klass} /></button>
{:else}
  <img {src} {alt} {title} class={klass} />
{/if}

<style>
  .img-btn { padding: 0; border: 0; background: none; display: block; width: 100%; min-height: 0; cursor: zoom-in; }
  .img-btn:hover:not(:disabled) { background: none; }
</style>
