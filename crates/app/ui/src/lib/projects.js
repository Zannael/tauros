// APP-008: the projects folder, its projects and opening one. Opening relaunches
// Tauros on the project (after the normal shutdown), at its last boot point.
import { writable, get } from 'svelte/store';

export function createProjects({ ipc, prefs, setPref, notify = () => {}, t = (s) => s }) {
  const defaultDir = writable('');
  const list = writable({ dir: '', items: [], error: '', loading: false });
  const opening = writable(null);

  const folder = () => get(prefs).projectsDir || get(defaultDir);

  const api = {
    defaultDir, list, opening, folder,

    async init() {
      try { defaultDir.set(await ipc.invoke('projects_default_dir')); } catch { /* keep empty */ }
    },

    async refresh() {
      const dir = folder();
      if (!dir) return;
      list.set({ dir, items: get(list).dir === dir ? get(list).items : [], error: '', loading: true });
      try { list.set({ dir, items: await ipc.invoke('projects_list', { dir }), error: '', loading: false }); }
      catch (err) { list.set({ dir, items: [], error: `${err}`, loading: false }); }
    },

    // The system folder chooser, starting from `start` (the projects folder).
    async chooseFolder(start, title) {
      const picked = await ipc.invoke('plugin:dialog|open', { options: { directory: true, multiple: false, defaultPath: start || undefined, title } });
      return Array.isArray(picked) ? picked[0] || null : picked || null;
    },

    async open(path) {
      if (!path || get(opening)) return false;
      const point = get(prefs).lastPoints?.[path];
      opening.set(path);
      try {
        await ipc.invoke('project_open', { path, map: point?.map ?? null, x: point?.x ?? null, y: point?.y ?? null });
        notify(t('Opening {name}: Tauros restarts on it.', { name: path.split(/[\\/]/).filter(Boolean).pop() }));
        return true;
      } catch (err) {
        opening.set(null);
        notify(`${t('Cannot open the project')}: ${err}`, 'error');
        return false;
      }
    },

    setFolder(dir) { setPref('projectsDir', dir === get(defaultDir) ? '' : dir); api.refresh(); },
  };
  return api;
}

// Kept per project so that reopening it boots where the last preview did.
export function pointsStore(prefs, setPref) {
  return {
    last: (path) => (path ? get(prefs).lastPoints?.[path] || null : null),
    remember(path, point) {
      if (!path || !point?.map || point.x === '' || point.y === '') return;
      setPref('lastPoints', { ...get(prefs).lastPoints, [path]: { map: point.map, x: Number(point.x), y: Number(point.y) } });
    },
  };
}
