// Open game project: maps and the boot point (map, x, y) used by Preview and
// Build & play, live emulator location, and the Project view's build and tests.
import { writable, get } from 'svelte/store';

// A project opened without a boot point starts at its last one, else here.
const FIRST_MAP = 'MAP_LITTLEROOT_TOWN';

export function createProject({ ipc, t = (s) => s, notify = () => {}, isAgentBusy = () => false,
  lastPoint = () => null, rememberPoint = () => {} }) {
  const info = writable(null);               // project_info: maps, bounds, launch point
  const point = writable({ map: '', x: '', y: '' });
  const operation = writable(null);          // 'build' | 'test' | 'preview' | 'build_and_play'
  const live = writable(null);               // project_context result
  const build = writable(null);              // { ok, at, result | error }
  const tests = writable(null);              // { ok, at, result | error }
  const bounds = new Map(), remembered = new Map();

  function validPoint(map, x, y) {
    const b = bounds.get(map);
    return !!b && Number.isInteger(x) && Number.isInteger(y) && x >= 0 && y >= 0 && x < b.width && y < b.height;
  }

  const api = {
    info, point, operation, live, build, tests,
    bounds: (map) => bounds.get(map),

    pointValid() {
      const p = get(point);
      return p.x !== '' && p.y !== '' && validPoint(p.map, Number(p.x), Number(p.y));
    },

    async refresh() {
      let data;
      try { data = await ipc.invoke('project_info'); } catch { return; }
      if (!data) return;
      const first = !get(info);
      bounds.clear();
      for (const b of data.map_bounds || []) bounds.set(b.id, b);
      info.set(data);
      if (first) {
        if (data.map) {
          point.set({ map: data.map, x: data.x, y: data.y });
          rememberPoint(data.path, get(point));
        } else {
          const saved = lastPoint(data.path);
          const map = saved && bounds.has(saved.map) ? saved.map : bounds.has(FIRST_MAP) ? FIRST_MAP : data.maps[0];
          const b = bounds.get(map);
          point.set(saved && saved.map === map ? { map, x: saved.x, y: saved.y } : { map, x: b?.default_x ?? 0, y: b?.default_y ?? 0 });
        }
      }
      await api.refreshLive();
    },

    // Remembers valid points per map; a new map starts at its saved point or centre.
    chooseMap(map) {
      const p = get(point);
      if (validPoint(p.map, Number(p.x), Number(p.y))) remembered.set(p.map, { x: Number(p.x), y: Number(p.y) });
      const saved = remembered.get(map), b = bounds.get(map);
      point.set(saved ? { map, ...saved } : { map, x: b ? b.default_x : 0, y: b ? b.default_y : 0 });
    },
    setCoordinate(axis, value) {
      point.update((p) => ({ ...p, [axis]: value === '' ? '' : Number(value) }));
    },

    async refreshLive() {
      const p = get(point);
      if (!p.map) return;
      try {
        live.set(await ipc.invoke('project_context', {
          selectedMap: p.map, selectedX: p.x === '' ? null : Number(p.x), selectedY: p.y === '' ? null : Number(p.y),
        }));
      } catch { /* no project or context unavailable */ }
    },

    // Preview boots the last build; Build & play validates, snapshots and builds first.
    async boot(withBuild) {
      if (get(operation)) return false;
      if (!api.pointValid()) { notify(t('Choose integer coordinates inside this map.'), 'error'); return false; }
      if (isAgentBusy()) { notify(t('The agent is working; wait for it or stop it.'), 'error'); return false; }
      const { map, x, y } = get(point);
      remembered.set(map, { x: Number(x), y: Number(y) });
      operation.set(withBuild ? 'build_and_play' : 'preview');
      try {
        const report = await ipc.invoke(withBuild ? 'build_and_play' : 'preview_map', { map, x: Number(x), y: Number(y) });
        rememberPoint(get(info)?.path, { map, x: Number(x), y: Number(y) });
        if (withBuild) build.set({ ok: true, at: new Date(), result: { rom_sha256: report.rom_sha256, counts: null, diagnostics: [] } });
        notify(t(withBuild ? 'Built {sha} and booted at {map} ({x}, {y}).' : 'Previewing the last build at {map} ({x}, {y}).',
          { sha: report.rom_sha256.slice(0, 12), map, x, y }), 'success');
        return true;
      } catch (err) {
        if (withBuild) build.set({ ok: false, at: new Date(), error: `${err}` });
        notify(`${t(withBuild ? 'Build & play failed' : 'Preview failed')}: ${err}`, 'error');
        return false;
      } finally {
        operation.set(null);
        api.refreshLive();
      }
    },

    async runBuild() {
      if (get(operation)) return;
      operation.set('build');
      try {
        const result = await ipc.invoke('project_build');
        build.set({ ok: result.ok, at: new Date(), result });
        notify(t('Build finished: {errors} errors, {warnings} warnings.', result.counts), result.counts.errors ? 'error' : 'success');
        await api.refresh();
      } catch (err) {
        build.set({ ok: false, at: new Date(), error: `${err}` });
        notify(`${t('Build failed')}: ${err}`, 'error');
      } finally {
        operation.set(null);
        api.refreshLive();
      }
    },

    async runTests(filter) {
      if (get(operation)) return;
      operation.set('test');
      try {
        const result = await ipc.invoke('project_test', { filter: filter || null });
        tests.set({ ok: result.ok, at: new Date(), result });
        notify(result.ok ? t('Tests passed.') : t('Some tests failed.'), result.ok ? 'success' : 'error');
      } catch (err) {
        tests.set({ ok: false, at: new Date(), error: `${err}` });
        notify(`${t('Tests could not run')}: ${err}`, 'error');
      } finally {
        operation.set(null);
      }
    },
  };
  return api;
}
