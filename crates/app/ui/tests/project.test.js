// Boot point handling: bounded coordinates, remembered points per map and the
// guards before Preview or Build & play reach the backend.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { get } from 'svelte/store';
import { createProject } from '../src/lib/project.js';
import { translate } from '../src/lib/i18n.js';

const t = (text, values) => translate('en', text, values);

function setup({ agentBusy = false, check = () => ({ ok: true, map_refused: false, problem: null, nearest: null }), bounds } = {}) {
  const calls = [], notes = [];
  const ipc = {
    invoke: async (cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === 'project_info') return { maps: ['MAP_A', 'MAP_B'], map: 'MAP_A', x: 5, y: 6,
        map_bounds: bounds || [{ id: 'MAP_A', width: 20, height: 20, default_x: 10, default_y: 10, bootable: true, refusal: null },
          { id: 'MAP_B', width: 8, height: 8, default_x: 4, default_y: 4, bootable: true, refusal: null }] };
      if (cmd === 'boot_point_check') return check(args);
      if (cmd === 'project_context') return { live_status: 'ok', new_build_pending: false };
      if (cmd === 'preview_map' || cmd === 'build_and_play') return { rom_sha256: 'abcdef0123456789', boot_frames: 10 };
      if (cmd === 'project_build') return { ok: true, rom_sha256: 'ff', counts: { errors: 0, warnings: 2, omitted: 0 }, diagnostics: [] };
      return null;
    },
  };
  const project = createProject({ ipc, t, notify: (text, k) => notes.push([text, k]), isAgentBusy: () => agentBusy });
  return { project, calls, notes };
}

test('the launch point is loaded and points are remembered per map', async () => {
  const { project } = setup();
  await project.refresh();
  assert.deepEqual(get(project.point), { map: 'MAP_A', x: 5, y: 6 });
  project.chooseMap('MAP_B');
  assert.deepEqual(get(project.point), { map: 'MAP_B', x: 4, y: 4 });
  project.setCoordinate('x', '7');
  project.chooseMap('MAP_A');
  assert.deepEqual(get(project.point), { map: 'MAP_A', x: 5, y: 6 });
  project.chooseMap('MAP_B');
  assert.deepEqual(get(project.point), { map: 'MAP_B', x: 7, y: 4 });
});

test('out-of-bounds points never reach the backend', async () => {
  const { project, calls, notes } = setup();
  await project.refresh();
  project.setCoordinate('x', '25');
  assert.equal(project.pointValid(), false);
  assert.equal(await project.boot(false), false);
  assert(!calls.some(([c]) => c === 'preview_map'));
  assert.equal(notes.at(-1)[1], 'error');
});

test('preview and build & play send the chosen point', async () => {
  const { project, calls } = setup();
  await project.refresh();
  assert.equal(await project.boot(false), true);
  assert.deepEqual(calls.find(([c]) => c === 'preview_map')[1], { map: 'MAP_A', x: 5, y: 6 });
  assert.equal(await project.boot(true), true);
  assert.equal(get(project.build).ok, true);
  assert.equal(get(project.operation), null);
});

test('a busy agent blocks booting', async () => {
  const { project, calls } = setup({ agentBusy: true });
  await project.refresh();
  assert.equal(await project.boot(true), false);
  assert(!calls.some(([c]) => c === 'build_and_play'));
});

test('a build records its diagnostics counts', async () => {
  const { project } = setup();
  await project.refresh();
  await project.runBuild();
  assert.equal(get(project.build).result.counts.warnings, 2);
});

const bounds = [
  { id: 'MAP_FRLG', width: 10, height: 10, default_x: 5, default_y: 5, bootable: false, refusal: 'it belongs to REGION_KANTO' },
  { id: 'MAP_A', width: 20, height: 20, default_x: 10, default_y: 9, bootable: true, refusal: null },
];

test('a map the build cannot boot is refused with its reason, before any boot', async () => {
  const { project, calls, notes } = setup({ bounds });
  await project.refresh();
  assert.equal(project.refusal('MAP_FRLG'), 'it belongs to REGION_KANTO');
  project.chooseMap('MAP_FRLG');
  assert.equal(get(project.point).map, 'MAP_A', 'the selection did not move');
  assert.match(notes.at(-1)[0], /MAP_FRLG cannot be booted: it belongs to REGION_KANTO/);
  assert(!calls.some(([c]) => c === 'preview_map'));
});

test('a saved point on an unbootable map falls back to the first bootable map at its default tile', async () => {
  const ipc = { invoke: async (cmd) => cmd === 'project_info' ? { maps: ['MAP_FRLG', 'MAP_A'], map: null, path: '/p', map_bounds: bounds } : null };
  const notes = [];
  const project = createProject({ ipc, t, notify: (text) => notes.push(text), lastPoint: () => ({ map: 'MAP_FRLG', x: 1, y: 1 }) });
  await project.refresh();
  assert.deepEqual(get(project.point), { map: 'MAP_A', x: 10, y: 9 });
});

test('a blocked remembered point moves to the nearest walkable tile and says so', async () => {
  const check = ({ x, y }) => x === 5 && y === 6 ? { ok: false, map_refused: false, problem: 'wall', nearest: [5, 5] } : { ok: true };
  const { project, notes } = setup({ check });
  await project.refresh();
  assert.deepEqual(get(project.point), { map: 'MAP_A', x: 5, y: 5 });
  assert.match(notes.at(-1)[0], /\(5, 6\) of MAP_A is blocked.*\(5, 5\)/);
});

test('a refused blocked point offers the nearest walkable tile and names map, point and cause', async () => {
  const cause = 'the tile is solid (collision); the nearest walkable tile is (7, 8)';
  const check = ({ x }) => x === 7 ? { ok: false, map_refused: false, problem: cause, nearest: [7, 8] } : { ok: true };
  const notes = [];
  const ipc = { invoke: async (cmd, args) => {
    if (cmd === 'project_info') return { maps: ['MAP_A'], map: 'MAP_A', x: 3, y: 6, map_bounds: [{ id: 'MAP_A', width: 20, height: 20, default_x: 10, default_y: 10, bootable: true }] };
    if (cmd === 'boot_point_check') return check(args);
    if (cmd === 'preview_map') throw cause;
    return null;
  } };
  const project = createProject({ ipc, t, notify: (text, k) => notes.push([text, k]) });
  await project.refresh();
  project.setCoordinate('x', '7');
  assert.equal(await project.boot(false), false);
  assert.deepEqual(get(project.suggestion), { x: 7, y: 8 });
  assert.deepEqual(notes.at(-1), [`Preview failed at MAP_A (7, 6): ${cause}`, 'error']);
  project.setCoordinate('y', '8');
  assert.equal(get(project.suggestion), null, 'editing the point clears the offer');
});

test('a boot failure names the map, the point and the cause', async () => {
  const notes = [];
  const ipc = { invoke: async (cmd) => {
    if (cmd === 'project_info') return { maps: ['MAP_A'], map: 'MAP_A', x: 3, y: 4, map_bounds: [{ id: 'MAP_A', width: 9, height: 9, default_x: 4, default_y: 4, bootable: true }] };
    if (cmd === 'build_and_play') throw 'boot: the target map not reached in 900 frames';
    return { ok: true };
  } };
  const project = createProject({ ipc, t, notify: (text, k) => notes.push([text, k]) });
  await project.refresh();
  assert.equal(await project.boot(true), false);
  assert.deepEqual(notes.at(-1), ['Build & play failed at MAP_A (3, 4): boot: the target map not reached in 900 frames', 'error']);
});
