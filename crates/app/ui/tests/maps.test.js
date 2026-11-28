// Maps view controller: loading maps, tiles becoming the boot point, events and
// warps, the live player marker and following the emulator.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { get } from 'svelte/store';
import { createProject } from '../src/lib/project.js';
import { createMaps, tileClass, eventLabel, kindIndex } from '../src/lib/maps.js';
import { translate } from '../src/lib/i18n.js';

const t = (text, values) => translate('en', text, values);

const MAPS = {
  MAP_A: { id: 'MAP_A', width: 3, height: 2, refusal: null, metatiles: [1, 2, 3, 4, 5, 6], collision: [0, 1, 0, 0, 0, 0],
    elevation: [3, 3, 1, 0, 3, 15], behaviors: [0, 2, null, 0, 0, 0], behavior_names: { 0: 'MB_NORMAL', 2: 'MB_TALL_GRASS' },
    events: [
      { kind: 'npc', local_id: 'LOCALID_BOY', graphics: 'OBJ_EVENT_GFX_BOY_1', position: { x: 0, y: 1, elevation: 3 } },
      { kind: 'warp', position: { x: 2, y: 0, elevation: 0 }, destination_map: 'MAP_B', destination_warp: 1 },
    ], connections: [] },
  MAP_B: { id: 'MAP_B', width: 2, height: 2, refusal: null, metatiles: [0, 0, 0, 0], collision: [0, 0, 0, 0], elevation: [3, 3, 3, 3],
    behaviors: [0, 0, 0, 0], behavior_names: {}, connections: [],
    events: [
      { kind: 'warp', position: { x: 0, y: 0, elevation: 0 }, destination_map: 'MAP_A', destination_warp: 0 },
      { kind: 'background', background: 'sign', script: 'B_Sign', position: { x: 0, y: 1, elevation: 0 } },
      { kind: 'warp', position: { x: 1, y: 1, elevation: 0 }, destination_map: 'MAP_A', destination_warp: 0 },
    ] },
  MAP_FRLG: { id: 'MAP_FRLG', width: 1, height: 1, refusal: 'it belongs to REGION_KANTO', metatiles: [0], collision: [0],
    elevation: [3], behaviors: [0], behavior_names: {}, events: [], connections: [] },
};

function setup({ live = { live_status: 'ok', live_map: 'MAP_A', live_x: 1, live_y: 1 }, delay = {} } = {}) {
  const calls = [], notes = [];
  const ipc = {
    invoke: async (cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === 'project_info') return { maps: Object.keys(MAPS), map: 'MAP_A', x: 0, y: 1, path: '/p', built: true,
        map_bounds: Object.values(MAPS).map((m) => ({ id: m.id, width: m.width, height: m.height, default_x: 0, default_y: 0,
          bootable: !m.refusal, refusal: m.refusal })) };
      if (cmd === 'map_view') {
        if (delay[args.map]) await delay[args.map];
        if (!MAPS[args.map]) throw new Error(`unknown map ${args.map}`);
        return MAPS[args.map];
      }
      if (cmd === 'boot_point_check') return args.x === 1 && args.y === 0
        ? { ok: false, map_refused: false, problem: 'solid (collision)', nearest: [0, 0] } : { ok: true, map_refused: false, problem: null, nearest: null };
      if (cmd === 'project_context') return live;
      if (cmd === 'preview_map' || cmd === 'build_and_play') return { rom_sha256: 'abcdef0123456789', boot_frames: 10 };
      return null;
    },
  };
  const notify = (text, kind) => notes.push([text, kind]);
  const project = createProject({ ipc, t, notify });
  const maps = createMaps({ ipc, project, notify, t });
  return { project, maps, calls, notes };
}

test('tiles are classified as the boot check reads them', () => {
  assert.equal(tileClass(1, 3), 'solid');
  assert.equal(tileClass(0, 1), 'water');
  assert.equal(tileClass(0, 15), 'walkable');
  assert.equal(eventLabel(MAPS.MAP_A.events[0], 0), 'LOCALID_BOY');
  assert.equal(eventLabel(MAPS.MAP_A.events[1], 0), '→ MAP_B #1');
  // MAP_B's third event is its second warp, warp #1.
  assert.equal(kindIndex(MAPS.MAP_B.events, 2), 1);
  assert.equal(kindIndex(MAPS.MAP_B.events, 1), 0);
});

test('a chosen tile becomes the boot point with its walkability', async () => {
  const { project, maps } = setup();
  await project.refresh();
  await maps.open('MAP_A');
  assert.equal(get(maps.current).id, 'MAP_A');
  assert.deepEqual(maps.block(1, 0), { x: 1, y: 0, metatile: 2, collision: 1, elevation: 3, behavior: 2,
    behaviorName: 'MB_TALL_GRASS', kind: 'solid' });
  assert.equal(maps.block(3, 0), null);
  await maps.selectTile(1, 0);
  assert.deepEqual(get(project.point), { map: 'MAP_A', x: 1, y: 0 });
  assert.equal(get(maps.tileCheck).problem, 'solid (collision)');
  await maps.selectTile(2, 1);
  assert.deepEqual(get(project.point), { map: 'MAP_A', x: 2, y: 1 });
  assert.equal(get(maps.tileCheck).ok, true);
  await maps.selectTile(9, 9);
  assert.deepEqual(get(maps.tile), { x: 2, y: 1 }, 'outside the map nothing changes');
});

test('a refused map can be viewed but never becomes the boot point', async () => {
  const { project, maps, notes } = setup();
  await project.refresh();
  await maps.open('MAP_FRLG');
  await maps.selectTile(0, 0);
  assert.deepEqual(get(maps.tile), { x: 0, y: 0 });
  assert.equal(get(maps.tileCheck), null);
  assert.equal(get(project.point).map, 'MAP_A');
  assert.equal(await maps.play(false), false);
  assert.match(notes.at(-1)[0], /cannot be booted/);
});

test('events select their tile and warps open the destination warp', async () => {
  const { project, maps, notes } = setup();
  await project.refresh();
  await maps.open('MAP_A');
  await maps.selectEvent(0);
  assert.deepEqual(get(maps.tile), { x: 0, y: 1 });
  assert.equal(maps.eventsAt(0, 1).length, 1);
  await maps.followWarp(MAPS.MAP_A.events[1]);
  assert.equal(get(maps.current).id, 'MAP_B');
  // Warp #1 of MAP_B is its second warp (event 2), not event 1 (a sign).
  assert.equal(get(maps.selectedEvent), 2);
  assert.deepEqual(get(project.point), { map: 'MAP_B', x: 1, y: 1 });
  await maps.followWarp({ kind: 'warp', destination_map: 'MAP_B', destination_warp: 7 });
  assert.match(notes.at(-1)[0], /no warp #7/);
});

test('a slow load does not replace a newer one and errors are kept', async () => {
  let release;
  const slow = new Promise((r) => (release = r));
  const { maps } = setup({ delay: { MAP_A: slow } });
  const first = maps.open('MAP_A');
  await maps.open('MAP_B');
  release();
  await first;
  assert.equal(get(maps.current).id, 'MAP_B');
  await maps.open('MAP_NOPE');
  assert.match(get(maps.error).message, /unknown map/);
  assert.equal(get(maps.current).id, 'MAP_B');
  assert.equal(get(maps.loading), null);
});

test('the live player is marked on its map and following opens new maps', async () => {
  const { project, maps } = setup();
  await project.refresh();
  await maps.open('MAP_B');
  const live = { live_status: 'ok', live_map: 'MAP_A', live_x: 1, live_y: 1 };
  assert.equal(maps.liveOnMap(live), null);
  await maps.onLive(live);
  assert.equal(get(maps.current).id, 'MAP_B', 'not following by default');
  await maps.setFollow(true, live);
  assert.equal(get(maps.current).id, 'MAP_A', 'turning following on opens the game map');
  assert.deepEqual(maps.liveOnMap(live), { x: 1, y: 1 });
  assert.equal(maps.liveOnMap({ ...live, live_status: 'title_or_uninitialized' }), null);
  // A map the user opens stays open while the game stays on its map.
  await maps.open('MAP_B');
  await maps.onLive({ ...live, live_x: 2 });
  assert.equal(get(maps.current).id, 'MAP_B');
  // The game moving to another map opens it; a title screen in between changes nothing.
  await maps.onLive({ live_status: 'title_or_uninitialized' });
  assert.equal(get(maps.current).id, 'MAP_B');
  await maps.onLive({ ...live, live_map: 'MAP_FRLG', live_x: 0, live_y: 0 });
  assert.equal(get(maps.current).id, 'MAP_FRLG');
  await maps.setFollow(false, live);
  await maps.onLive(live);
  assert.equal(get(maps.current).id, 'MAP_FRLG');
  await maps.showLive(live);
  assert.equal(get(maps.current).id, 'MAP_A');
  assert.deepEqual(get(maps.tile), { x: 1, y: 1 });
});

test('play boots the last build or builds at the selected tile', async () => {
  const { project, maps, calls } = setup();
  await project.refresh();
  await maps.open('MAP_A');
  assert.equal(await maps.play(false), false, 'no tile chosen');
  await maps.selectTile(2, 1);
  assert.equal(await maps.play(false), true);
  assert.deepEqual(calls.find(([c]) => c === 'preview_map')[1], { map: 'MAP_A', x: 2, y: 1 });
  assert.equal(await maps.play(true), true);
  assert.deepEqual(calls.find(([c]) => c === 'build_and_play')[1], { map: 'MAP_A', x: 2, y: 1 });
});
