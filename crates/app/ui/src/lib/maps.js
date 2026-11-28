// Maps view (APP-001, read-only): the shown map, its overlays, the selected tile
// and event, and the link to the emulator. Choosing a tile on a bootable map sets
// the shared boot point, so Preview and Build & play start there from any view.
import { writable, get } from 'svelte/store';

export const TILE = 16;
export const ZOOMS = [1, 2, 3, 4];
export const OVERLAYS = ['none', 'collision', 'elevation'];

// What a block is for walking, as the boot point check reads it (EMU-006).
export function tileClass(collision, elevation) {
  if (collision !== 0) return 'solid';
  if (elevation === 1) return 'water';
  return 'walkable';
}

// Events are numbered within their kind, as the game and Porymap do: a warp's
// number is what other maps' `destination_warp` refers to.
export function kindIndex(events, index) {
  const kind = events[index]?.kind;
  return events.slice(0, index).filter((e) => e.kind === kind).length;
}

export function eventLabel(event, number) {
  switch (event.kind) {
    case 'npc': return event.local_id || `${event.graphics} #${number}`;
    case 'warp': return `→ ${event.destination_map} #${event.destination_warp}`;
    case 'trigger': return event.script || `trigger #${number}`;
    default: return event.script || `${event.background} #${number}`;
  }
}

export function createMaps({ ipc, project, notify = () => {}, t = (s) => s }) {
  const current = writable(null);        // map_view result
  const loading = writable(null);        // map id being loaded
  const error = writable(null);          // { map, message }
  const tile = writable(null);           // { x, y } selected
  const tileCheck = writable(null);      // boot_point_check result for the selected tile
  const selectedEvent = writable(null);  // index into current.events
  const settings = writable({ zoom: 2, overlay: 'none', events: true, grid: false, follow: false });
  let request = 0, lastLive = null;

  const block = (map, x, y) => {
    if (!map || x < 0 || y < 0 || x >= map.width || y >= map.height) return null;
    const i = y * map.width + x;
    const behavior = map.behaviors[i];
    return {
      x, y, metatile: map.metatiles[i], collision: map.collision[i], elevation: map.elevation[i],
      behavior, behaviorName: behavior == null ? null : map.behavior_names[behavior] ?? null,
      kind: tileClass(map.collision[i], map.elevation[i]),
    };
  };

  const api = {
    current, loading, error, tile, tileCheck, selectedEvent, settings,
    block: (x, y) => block(get(current), x, y),
    eventsAt: (x, y) => (get(current)?.events || []).map((e, i) => [e, i]).filter(([e]) => e.position.x === x && e.position.y === y),

    // Loads a map; an older request finishing late does not replace a newer one.
    async open(map, { select = null } = {}) {
      if (!map) return false;
      const mine = ++request;
      loading.set(map);
      error.set(null);
      try {
        const data = await ipc.invoke('map_view', { map });
        if (mine !== request) return false;
        current.set(data);
        selectedEvent.set(null);
        tile.set(null);
        tileCheck.set(null);
        if (select) await api.selectTile(select.x, select.y);
        return true;
      } catch (err) {
        if (mine === request) error.set({ map, message: `${err}` });
        return false;
      } finally {
        if (mine === request) loading.set(null);
      }
    },
    reload() { const map = get(current)?.id; return map ? api.open(map) : Promise.resolve(false); },

    // On a bootable map the tile becomes the boot point; the check says whether
    // the player can stand there.
    async selectTile(x, y) {
      const map = get(current);
      if (!block(map, x, y)) return;
      tile.set({ x, y });
      tileCheck.set(null);
      if (map.refusal || project.refusal(map.id)) return;
      project.setPoint(map.id, x, y);
      const check = await project.checkPoint(map.id, x, y);
      const now = get(tile);
      if (get(current) === map && now && now.x === x && now.y === y) tileCheck.set(check);
    },

    async selectEvent(index) {
      const event = get(current)?.events[index];
      if (!event) return;
      selectedEvent.set(index);
      await api.selectTile(event.position.x, event.position.y);
    },

    // A warp opens its destination at the destination warp's tile.
    async followWarp(event) {
      if (event?.kind !== 'warp') return false;
      if (!(await api.open(event.destination_map))) return false;
      const warps = get(current).events.map((e, i) => [e, i]).filter(([e]) => e.kind === 'warp');
      const target = warps[event.destination_warp];
      if (target) await api.selectEvent(target[1]);
      else notify(t('{map} has no warp #{n}.', { map: event.destination_map, n: event.destination_warp }), 'info');
      return true;
    },

    setSetting(key, value) { settings.update((s) => ({ ...s, [key]: value })); },

    // Where the game's player is, when the emulator runs on the shown map.
    liveOnMap(live) {
      const map = get(current);
      if (!map || !live || live.live_status !== 'ok' || live.live_map !== map.id) return null;
      return live.live_x == null || live.live_y == null ? null : { x: live.live_x, y: live.live_y };
    },

    // Opens the emulator's current map at the player's tile.
    async showLive(live) {
      if (!live || live.live_status !== 'ok' || !live.live_map) { notify(t('The game is not in the world.'), 'info'); return false; }
      return api.open(live.live_map, { select: { x: live.live_x, y: live.live_y } });
    },

    // Following: when the game's map changes, it opens; moves within a map and the
    // maps the user opens meanwhile are left alone.
    async onLive(live) {
      const map = live?.live_status === 'ok' ? live.live_map : null;
      if (!map || map === lastLive) return;
      lastLive = map;
      if (!get(settings).follow || map === get(current)?.id || map === get(loading)) return;
      await api.open(map);
    },

    // Turning following on opens the game's map now.
    async setFollow(on, live) {
      api.setSetting('follow', on);
      const map = live?.live_status === 'ok' ? live.live_map : null;
      if (on && map && map !== get(current)?.id) await api.open(map);
    },

    // Starts the game at the selected tile; the game then plays beside the view.
    async play(withBuild) {
      const map = get(current), at = get(tile);
      if (!map || !at) { notify(t('Choose a tile on the map first.'), 'error'); return false; }
      const refusal = map.refusal || project.refusal(map.id);
      if (refusal) { notify(t('{map} cannot be booted: {reason}', { map: map.id, reason: refusal }), 'error'); return false; }
      project.setPoint(map.id, at.x, at.y);
      return project.boot(withBuild);
    },
  };
  return api;
}
