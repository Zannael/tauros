// Boot point handling: bounded coordinates, remembered points per map and the
// guards before Preview or Build & play reach the backend.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { get } from 'svelte/store';
import { createProject } from '../src/lib/project.js';

function setup({ agentBusy = false } = {}) {
  const calls = [], notes = [];
  const ipc = {
    invoke: async (cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === 'project_info') return { maps: ['MAP_A', 'MAP_B'], map: 'MAP_A', x: 5, y: 6,
        map_bounds: [{ id: 'MAP_A', width: 20, height: 20, default_x: 10, default_y: 10 }, { id: 'MAP_B', width: 8, height: 8, default_x: 4, default_y: 4 }] };
      if (cmd === 'project_context') return { live_status: 'ok', new_build_pending: false };
      if (cmd === 'preview_map' || cmd === 'build_and_play') return { rom_sha256: 'abcdef0123456789', boot_frames: 10 };
      if (cmd === 'project_build') return { ok: true, rom_sha256: 'ff', counts: { errors: 0, warnings: 2, omitted: 0 }, diagnostics: [] };
      return null;
    },
  };
  const project = createProject({ ipc, notify: (t, k) => notes.push([t, k]), isAgentBusy: () => agentBusy });
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
