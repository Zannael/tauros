// Projects folder, listing and opening (APP-008): the default folder, the
// remembered boot point sent on open, and the point a project starts at.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { writable, get } from 'svelte/store';
import { createProjects, pointsStore } from '../src/lib/projects.js';
import { createProject } from '../src/lib/project.js';

function prefsStore(initial = {}) {
  const prefs = writable({ projectsDir: '', lastPoints: {}, ...initial });
  const setPref = (k, v) => prefs.update((p) => ({ ...p, [k]: v }));
  return { prefs, setPref };
}

test('the default folder is used until another one is chosen', async () => {
  const { prefs, setPref } = prefsStore();
  const calls = [];
  const projects = createProjects({ prefs, setPref, ipc: { invoke: async (c, a) => {
    calls.push([c, a]);
    if (c === 'projects_default_dir') return '/data/projects';
    if (c === 'projects_list') return [{ name: 'hack', path: `${a.dir}/hack`, built: true }];
  } } });
  await projects.init();
  await projects.refresh();
  assert.equal(get(projects.list).items[0].path, '/data/projects/hack');
  projects.setFolder('/elsewhere');
  await new Promise((r) => setTimeout(r, 0));
  assert.equal(get(prefs).projectsDir, '/elsewhere');
  assert.equal(calls.at(-1)[1].dir, '/elsewhere');
  projects.setFolder('/data/projects');
  assert.equal(get(prefs).projectsDir, '', 'choosing the default folder stores nothing');
});

test('opening sends the remembered point and reports failures', async () => {
  const { prefs, setPref } = prefsStore({ lastPoints: { '/p/a': { map: 'MAP_A', x: 3, y: 4 } } });
  const calls = [], notes = [];
  const projects = createProjects({ prefs, setPref, notify: (t, k) => notes.push(k), ipc: { invoke: async (c, a) => { calls.push([c, a]); } } });
  assert.equal(await projects.open('/p/a'), true);
  assert.deepEqual(calls.at(-1), ['project_open', { path: '/p/a', map: 'MAP_A', x: 3, y: 4 }]);
  const again = createProjects({ prefs, setPref, notify: (t, k) => notes.push(k), ipc: { invoke: async (c, a) => { calls.push([c, a]); throw 'not a project'; } } });
  assert.equal(await again.open('/p/b'), false);
  assert.deepEqual(calls.at(-1)[1], { path: '/p/b', map: null, x: null, y: null });
  assert.equal(notes.at(-1), 'error');
  assert.equal(get(again.opening), null, 'a failed open can be retried');
});

test('a project without a launch point starts at its remembered point, else Littleroot', async () => {
  const { prefs, setPref } = prefsStore({ lastPoints: { '/p/a': { map: 'MAP_B', x: 2, y: 3 } } });
  const points = pointsStore(prefs, setPref);
  const info = (path) => ({ path, built: true, maps: ['MAP_B', 'MAP_LITTLEROOT_TOWN'], map: null, x: null, y: null,
    map_bounds: [{ id: 'MAP_B', width: 8, height: 8, default_x: 4, default_y: 4 }, { id: 'MAP_LITTLEROOT_TOWN', width: 20, height: 20, default_x: 10, default_y: 9 }] });
  const make = (path) => createProject({ lastPoint: points.last, rememberPoint: points.remember,
    ipc: { invoke: async (c) => (c === 'project_info' ? info(path) : c === 'preview_map' ? { rom_sha256: 'ab'.repeat(8) } : null) } });
  const a = make('/p/a');
  await a.refresh();
  assert.deepEqual(get(a.point), { map: 'MAP_B', x: 2, y: 3 });
  const b = make('/p/b');
  await b.refresh();
  assert.deepEqual(get(b.point), { map: 'MAP_LITTLEROOT_TOWN', x: 10, y: 9 });
  b.setCoordinate('x', '12');
  await b.boot(false);
  assert.deepEqual(get(prefs).lastPoints['/p/b'], { map: 'MAP_LITTLEROOT_TOWN', x: 12, y: 9 });
});
