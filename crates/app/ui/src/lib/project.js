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
  const suggestion = writable(null);         // { x, y }: nearest walkable tile offered after a refused point
  const bounds = new Map(), remembered = new Map();

  function validPoint(map, x, y) {
    const b = bounds.get(map);
    return !!b && Number.isInteger(x) && Number.isInteger(y) && x >= 0 && y >= 0 && x < b.width && y < b.height;
  }

  const api = {
    info, point, operation, live, build, tests, suggestion,
    bounds: (map) => bounds.get(map),
    // Why the build cannot boot this map, or null when it can.
    refusal: (map) => bounds.get(map)?.refusal ?? null,

    pointValid() {
      const p = get(point);
      return p.x !== '' && p.y !== '' && validPoint(p.map, Number(p.x), Number(p.y));
    },

    // Asks the backend whether the player can start at the point; null when it cannot say.
    async checkPoint(map, x, y) {
      try { return await ipc.invoke('boot_point_check', { map, x: Number(x), y: Number(y) }); } catch { return null; }
    },

    // A saved or launch point that is blocked moves to the nearest walkable tile; a
    // map the build cannot boot falls back to the first bootable one at its default.
    async resolvePoint() {
      const p = get(point);
      if (!bounds.has(p.map) || p.x === '' || p.y === '') return;
      const report = await api.checkPoint(p.map, p.x, p.y);
      if (!report || report.ok) return;
      if (!report.map_refused && report.nearest) {
        const [nx, ny] = report.nearest;
        point.set({ map: p.map, x: nx, y: ny });
        notify(t('The saved point ({x}, {y}) of {map} is blocked; moved to the nearest walkable tile ({nx}, {ny}).', { map: p.map, x: p.x, y: p.y, nx, ny }), 'info');
      } else {
        const first = api.firstBootable();
        if (first) {
          const b = bounds.get(first);
          point.set({ map: first, x: b.default_x, y: b.default_y });
          notify(t('{map} cannot be booted: {reason}. Starting from {first}.', { map: p.map, reason: report.problem || '', first }), 'info');
        }
      }
    },

    firstBootable() {
      const ok = (id) => bounds.get(id)?.bootable !== false && bounds.has(id);
      return ok(FIRST_MAP) ? FIRST_MAP : [...bounds.keys()].find(ok) ?? null;
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
          const usable = saved && bounds.has(saved.map) && bounds.get(saved.map).bootable !== false;
          const map = usable ? saved.map : api.firstBootable() ?? data.maps[0];
          const b = bounds.get(map);
          point.set(usable && saved.map === map ? { map, x: saved.x, y: saved.y } : { map, x: b?.default_x ?? 0, y: b?.default_y ?? 0 });
        }
        await api.resolvePoint();
      }
      await api.refreshLive();
    },

    // Remembers valid points per map; a new map starts at its saved point or centre.
    chooseMap(map) {
      const refusal = api.refusal(map);
      if (refusal) { notify(t('{map} cannot be booted: {reason}', { map, reason: refusal }), 'error'); return; }
      suggestion.set(null);
      const p = get(point);
      if (validPoint(p.map, Number(p.x), Number(p.y))) remembered.set(p.map, { x: Number(p.x), y: Number(p.y) });
      const saved = remembered.get(map), b = bounds.get(map);
      point.set(saved ? { map, ...saved } : { map, x: b ? b.default_x : 0, y: b ? b.default_y : 0 });
    },
    // Sets the whole point at once (Maps view); refused maps keep the current point.
    setPoint(map, x, y) {
      if (!bounds.has(map) || api.refusal(map)) return false;
      suggestion.set(null);
      const p = get(point);
      if (validPoint(p.map, Number(p.x), Number(p.y))) remembered.set(p.map, { x: Number(p.x), y: Number(p.y) });
      point.set({ map, x, y });
      return true;
    },
    setCoordinate(axis, value) {
      suggestion.set(null);
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
      const failed = (what, cause) => notify(t('{what} failed at {map} ({x}, {y}): {cause}', { what: t(what), map, x, y, cause }), 'error');
      suggestion.set(null);
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
        failed(withBuild ? 'Build & play' : 'Preview', `${err}`);
        // The backend refused (and logged) the point; offer the nearest walkable tile.
        const verdict = await api.checkPoint(map, x, y);
        if (verdict && !verdict.ok && verdict.nearest) suggestion.set({ x: verdict.nearest[0], y: verdict.nearest[1] });
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
