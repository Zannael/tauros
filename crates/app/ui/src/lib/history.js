// APP-004 change history over the snapshot commits, with non-destructive restore.
import { writable, get } from 'svelte/store';

export function createHistory({ ipc, t = (s) => s, notify = () => {}, isAgentBusy = () => false }) {
  const entries = writable([]);
  const state = writable({ loading: false, restoring: null, error: '' });

  const api = {
    entries, state,
    async load() {
      state.update((s) => ({ ...s, loading: true, error: '' }));
      try { entries.set(await ipc.invoke('history_list', { limit: 100 })); state.update((s) => ({ ...s, loading: false })); }
      catch (err) { entries.set([]); state.update((s) => ({ ...s, loading: false, error: `${err}` })); }
    },
    // Restore keeps every commit: it snapshots pending work, then records a new
    // commit whose files match the chosen point.
    async restore(entry) {
      if (get(state).restoring) return;
      if (isAgentBusy()) { notify(t('Wait for the agent to finish before restoring.'), 'error'); return; }
      state.update((s) => ({ ...s, restoring: entry.commit }));
      try {
        const receipt = await ipc.invoke('history_undo', { targetCommit: entry.commit });
        notify(receipt.undo_commit
          ? t('Restored {sha}. Recorded as {undo}; build or preview to see it in the game.', { sha: entry.short_commit, undo: receipt.undo_commit.slice(0, 8) })
          : t('No files changed: the sources already match {sha}.', { sha: entry.short_commit }), 'success');
        await api.load();
      } catch (err) {
        notify(`${t('Restore failed')}: ${err}`, 'error');
      } finally {
        state.update((s) => ({ ...s, restoring: null }));
      }
    },
  };
  return api;
}

export const originOf = (entry) => entry.provenance || entry.origin;
