// Exercises the same policy callbacks used by the live SDK, against the Rust host.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { NativeTools } from '../dist/native-tools.js';

const [root, socket] = process.argv.slice(2);
const native = new NativeTools(root, socket);
const options = native.options(['mcp__tauros__*']);
const before = options.hooks.PreToolUse[0].hooks[0];
const after = options.hooks.PostToolUse[0].hooks[0];
const hook = (name, input, id) => before({ hook_event_name: 'PreToolUse', tool_name: name,
  tool_input: input, tool_use_id: id }, id, { signal: new AbortController().signal });

for (const [name, input] of [
  ['Read', { file_path: '/etc/passwd' }],
  ['Write', { file_path: '../outside.c', content: 'bad' }],
  ['Edit', { file_path: '.git/config' }],
  ['Glob', { pattern: '../**' }],
  ['Grep', { path: '/tmp', pattern: 'anything' }],
  ['Bash', { command: 'true', run_in_background: true }],
  ['Agent', { prompt: 'bypass the guard' }],
]) {
  const result = await hook(name, input, `denied-${name}`);
  assert.equal(result.hookSpecificOutput.permissionDecision, 'deny', name);
}

const prepared = await hook('Edit', { file_path: 'scene.c' }, 'edit');
assert.equal(prepared.hookSpecificOutput.permissionDecision, 'allow');
assert.equal(prepared.hookSpecificOutput.updatedInput.file_path, path.join(root, 'scene.c'));
assert.equal((await hook('Write', { file_path: 'other.c' }, 'parallel')).hookSpecificOutput.permissionDecision, 'deny');
assert.equal((await hook('mcp__tauros__build', {}, 'parallel-mcp')).hookSpecificOutput.permissionDecision, 'deny');
await fs.writeFile(path.join(root, 'scene.c'), 'edited through native hooks\n');
await after({ hook_event_name: 'PostToolUse', tool_name: 'Edit', tool_use_id: 'edit' }, 'edit', {});

if (process.env.TAUROS_NATIVE_SANDBOX_TESTS === '1') {
  const outside = path.join(path.dirname(root), `${path.basename(root)}-outside`);
  await fs.writeFile(outside, 'untouched');
  try {
    const input = await native.prepare('Bash', { command:
      `printf 'shell edit\\n' > shell.c; test ! -e /home; ! printf bad > '${outside}'; ! printf bad > .git/config; exit 7` }, 'bash');
    const result = spawnSync('/bin/bash', ['--noprofile', '--norc', '-c', input.command], { encoding: 'utf8' });
    assert.equal(result.status, 7, result.stderr);
    await native.finish('bash');
    assert.equal(await fs.readFile(path.join(root, 'shell.c'), 'utf8'), 'shell edit\n');
    assert.equal(await fs.readFile(outside, 'utf8'), 'untouched');
    assert.match(await fs.readFile(path.join(root, '.git/config'), 'utf8'), /repositoryformatversion/);
  } finally {
    await fs.unlink(outside);
  }
}
await native.close();
console.log('native SDK policy hooks and transaction channel passed');
