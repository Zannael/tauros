/** Native SDK tools backed by host-owned Git transactions and a confined shell. */
import fs from 'node:fs/promises';
import net from 'node:net';
import path from 'node:path';
import readline from 'node:readline';
import type { HookCallback, Options } from '@anthropic-ai/claude-agent-sdk';

export const NATIVE_TOOLS = ['Read', 'Glob', 'Grep', 'Edit', 'Write', 'Bash'];
const MUTATIONS = new Set(['Edit', 'Write', 'Bash']);

function quote(value: string): string { return `'${value.replaceAll("'", "'\\''")}'`; }

/** Minimal mount namespace: no host home, credentials, network or writable Git. */
export function shellCommand(root: string, command: string): string {
  const args = ['/usr/bin/timeout', '--kill-after=2s', '50s', '/usr/bin/bwrap',
    '--unshare-all', '--die-with-parent', '--new-session', '--cap-drop', 'ALL', '--clearenv',
    '--ro-bind', '/usr', '/usr', '--symlink', 'usr/bin', '/bin',
    '--symlink', 'usr/lib', '/lib', '--symlink', 'usr/lib64', '/lib64',
    '--proc', '/proc', '--dev', '/dev', '--tmpfs', '/tmp',
    '--bind', root, root, '--ro-bind', path.join(root, '.git'), path.join(root, '.git'),
    ...['.gitignore', '.gitattributes', '.gitmodules'].flatMap((name) => ['--ro-bind-try', path.join(root, name), path.join(root, name)]),
    '--setenv', 'PATH', '/usr/bin:/bin', '--setenv', 'HOME', '/tmp',
    '--setenv', 'LANG', 'C.UTF-8', '--chdir', root,
    '/bin/bash', '--noprofile', '--norc', '-c', command];
  return args.map(quote).join(' ');
}

async function sourcePath(root: string, requested: unknown, directory = false): Promise<string> {
  if (typeof requested !== 'string') throw new Error('a project path is required');
  if (requested.split(/[\\/]/).includes('..')) throw new Error('path traversal is not allowed');
  const target = path.resolve(root, requested);
  const relative = path.relative(root, target);
  if (relative.startsWith('..') || path.isAbsolute(relative) || (!relative && !directory)) {
    throw new Error('path must be inside the game project');
  }
  let current = root;
  for (const part of relative.split(path.sep).filter(Boolean)) {
    if (['.git', '.gitignore', '.gitattributes', '.gitmodules'].includes(part)) {
      throw new Error('Git control files are managed by Tauros');
    }
    current = path.join(current, part);
    try {
      const stat = await fs.lstat(current);
      if (stat.isSymbolicLink() || (stat.isFile() && stat.nlink > 1)) {
        throw new Error('symlink and hardlink paths are not allowed');
      }
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error;
    }
  }
  return target;
}

interface Lease { socket: net.Socket; lines: AsyncIterator<string>; }

async function response(lease: Lease): Promise<Record<string, unknown>> {
  const line = await lease.lines.next();
  if (line.done) throw new Error('native snapshot host disconnected');
  const result = JSON.parse(line.value) as Record<string, unknown>;
  if (!result.ok) throw new Error(String(result.message ?? 'native transaction failed'));
  return result;
}

export class NativeTools {
  private readonly leases = new Map<string, Lease>();
  private failure?: string;

  constructor(private readonly root: string, private readonly socketPath: string) {}

  async prepare(name: string, input: Record<string, unknown>, id: string): Promise<Record<string, unknown>> {
    if (this.failure) throw new Error(this.failure);
    if (!NATIVE_TOOLS.includes(name)) throw new Error(`unsupported native tool: ${name}`);
    const updated = { ...input };
    if (name === 'Bash') {
      if (typeof input.command !== 'string' || !input.command.trim()) throw new Error('missing shell command');
      if (input.run_in_background) throw new Error('background commands are not allowed');
      updated.command = shellCommand(this.root, input.command);
      updated.timeout = 60000;
      updated.run_in_background = false;
      updated.dangerouslyDisableSandbox = false;
    } else if (name === 'Read' || name === 'Edit' || name === 'Write') {
      updated.file_path = await sourcePath(this.root, input.file_path);
    } else {
      updated.path = await sourcePath(this.root, input.path ?? this.root, true);
      if (name === 'Glob' && (typeof input.pattern !== 'string' || path.isAbsolute(input.pattern)
          || input.pattern.split(/[\\/]/).includes('..'))) {
        throw new Error('glob pattern must stay inside the game project');
      }
    }
    if (MUTATIONS.has(name)) {
      if (this.leases.size) throw new Error('native edit in progress; retry tools sequentially');
      const socket = net.createConnection(this.socketPath);
      const lease = { socket, lines: readline.createInterface({ input: socket })[Symbol.asyncIterator]() };
      // Install before awaiting: parallel hooks cannot both enter a transaction.
      this.leases.set(id, lease);
      try {
        await new Promise<void>((resolve, reject) => {
          socket.once('connect', resolve);
          socket.once('error', reject);
          socket.setTimeout(70000, () => socket.destroy(new Error('native transaction timeout')));
        });
        socket.write(`${JSON.stringify({ tool: name, path: updated.file_path })}\n`);
        await response(lease);
      } catch (error) {
        socket.destroy();
        this.leases.delete(id);
        throw error;
      }
    }
    return updated;
  }

  async finish(id: string): Promise<void> {
    const lease = this.leases.get(id);
    if (!lease) return;
    try {
      lease.socket.write('{}\n');
      await response(lease);
    } catch (error) {
      this.failure = String(error);
      throw error;
    } finally {
      lease.socket.destroy();
      this.leases.delete(id);
    }
  }

  async close(): Promise<void> {
    let failure = this.failure;
    for (const id of [...this.leases.keys()]) {
      try { await this.finish(id); } catch (error) { failure = String(error); }
    }
    if (failure) throw new Error(failure);
  }

  options(mcpTools: string[]): Partial<Options> {
    const pre: HookCallback = async (hook, id) => {
      if (hook.hook_event_name !== 'PreToolUse') return {};
      const name = hook.tool_name;
      // No new tools or subagents can bypass this closed list.
      if (mcpTools.some((pattern) => name.startsWith(pattern.slice(0, -1)))) {
        if (this.leases.size || this.failure) return { hookSpecificOutput: {
          hookEventName: 'PreToolUse', permissionDecision: 'deny',
          permissionDecisionReason: this.failure ?? 'native edit in progress; retry sequentially',
        } };
        return {};
      }
      try {
        const updatedInput = await this.prepare(name, hook.tool_input as Record<string, unknown>, id ?? hook.tool_use_id);
        return { hookSpecificOutput: { hookEventName: 'PreToolUse', permissionDecision: 'allow', updatedInput } };
      } catch (error) {
        return { hookSpecificOutput: { hookEventName: 'PreToolUse', permissionDecision: 'deny', permissionDecisionReason: String(error) } };
      }
    };
    const post: HookCallback = async (hook, id) => {
      if (hook.hook_event_name === 'PostToolUse' || hook.hook_event_name === 'PostToolUseFailure') {
        await this.finish(id ?? hook.tool_use_id);
      }
      return {};
    };
    return {
      tools: NATIVE_TOOLS,
      allowedTools: mcpTools, // Native calls are allowed only by the guarding hook.
      hooks: {
        PreToolUse: [{ hooks: [pre], onFailure: 'block', timeout: 80 }],
        PostToolUse: [{ hooks: [post], onFailure: 'block', timeout: 80 }],
        PostToolUseFailure: [{ hooks: [post], onFailure: 'block', timeout: 80 }],
      },
      canUseTool: async () => ({ behavior: 'deny', message: 'tool must pass the Tauros native guard' }),
    };
  }
}
