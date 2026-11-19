import { spawn } from 'node:child_process';
import readline from 'node:readline';
import {
  query,
  type McpServerConfig as SdkMcpServerConfig,
  type Options as SdkOptions,
} from '@anthropic-ai/claude-agent-sdk';
import type {
  AgentEvent,
  ErrorCode,
  McpServerConfig,
  SessionConfig,
  SidecarCommand,
  SidecarMessage,
  TokenUsage,
  UserMessage,
} from './protocol.js';

interface ActiveTurn {
  turnId: string;
  abortController: AbortController;
  cancelled: boolean;
}

interface SessionState {
  sessionId: string;
  config: SessionConfig;
  sdkSessionId?: string;
  activeTurn?: ActiveTurn;
}

const sessions = new Map<string, SessionState>();

function emit(msg: SidecarMessage): void {
  process.stdout.write(`${JSON.stringify(msg)}\n`);
}

function ackOk(id: string): void {
  emit({ type: 'ack', id, ok: true });
}

function ackErr(id: string, code: ErrorCode, message: string): void {
  emit({ type: 'ack', id, ok: false, code, message });
}

function emitEvent(sessionId: string, event: AgentEvent): void {
  // Release the session before announcing the end of its turn, so a client that
  // sends the next message as soon as it sees `turn_completed` is not refused.
  if (event.type === 'turn_completed') {
    const session = sessions.get(sessionId);
    if (session?.activeTurn?.turnId === event.turn_id) {
      session.activeTurn = undefined;
    }
  }
  emit({ type: 'event', session_id: sessionId, event });
}

function toSdkMcpServers(
  servers: Record<string, McpServerConfig> | undefined,
): Record<string, SdkMcpServerConfig> | undefined {
  if (!servers || Object.keys(servers).length === 0) {
    return undefined;
  }
  const out: Record<string, SdkMcpServerConfig> = {};
  for (const [name, cfg] of Object.entries(servers)) {
    if (cfg.transport === 'stdio') {
      out[name] = {
        type: 'stdio',
        command: cfg.command,
        args: cfg.args ?? [],
        env: cfg.env ?? {},
        alwaysLoad: true,
      };
    } else if (cfg.transport === 'streamable_http') {
      out[name] = {
        type: 'http',
        url: cfg.url,
        alwaysLoad: true,
      };
    }
  }
  return out;
}

function buildPrompt(message: UserMessage): string {
  if (!message.attachments || message.attachments.length === 0) {
    return message.text;
  }
  const lines = [message.text, '', 'Attachments:'];
  for (const att of message.attachments) {
    if (att.kind === 'path') {
      lines.push(`- File${att.label ? ` (${att.label})` : ''}: ${att.path}`);
    } else {
      lines.push(`- Context (${att.category}): ${JSON.stringify(att.data)}`);
    }
  }
  return lines.join('\n');
}

function mapAllowedTools(config: SessionConfig): string[] | undefined {
  const servers = config.mcp_servers;
  if (!servers || Object.keys(servers).length === 0) {
    return undefined;
  }
  return Object.keys(servers).map((name) => `mcp__${name}__*`);
}

function isAuthErrorMessage(msg: string): boolean {
  const lower = msg.toLowerCase();
  return (
    lower.includes('not logged in') ||
    lower.includes('oauth') ||
    lower.includes('authentication') ||
    lower.includes('login') ||
    lower.includes('api key')
  );
}

/**
 * Runs a turn via `@anthropic-ai/claude-agent-sdk` using the unmodified host
 * Claude Code sign-in (RES-002 personal-use auth model). Tauros never reads,
 * stores, or intermediates claude.ai credentials or tokens.
 */
async function runSdkTurn(
  session: SessionState,
  turn: ActiveTurn,
  message: UserMessage,
): Promise<void> {
  const { sessionId, config } = session;
  const { turnId, abortController } = turn;

  emitEvent(sessionId, { type: 'turn_started', turn_id: turnId });

  const mcpServers = toSdkMcpServers(config.mcp_servers);
  const allowedTools = mapAllowedTools(config);

  const options: SdkOptions = {
    abortController,
    cwd: config.working_dir,
    model: config.model,
    systemPrompt: config.system_prompt,
    mcpServers,
    allowedTools,
    // Keep edits strictly inside Tauros's domain MCP tools (no built-in file/shell tools).
    tools: [],
    includePartialMessages: true,
    // `resume` reloads the previous turns from the host CLI's own transcript
    // store; without persistence every turn would start a new conversation.
    persistSession: true,
    settingSources: [],
    resume: session.sdkSessionId,
  };

  const toolNamesById = new Map<string, string>();
  let streamedTextForCurrentMessage = false;
  let usage: TokenUsage | undefined;
  let completedEmitted = false;

  try {
    const stream = query({
      prompt: buildPrompt(message),
      options,
    });

    for await (const raw of stream) {
      if (turn.cancelled) {
        break;
      }

      const msg = raw as Record<string, unknown>;
      const msgType = typeof msg.type === 'string' ? msg.type : '';

      if (msgType === 'system') {
        if (typeof msg.session_id === 'string') {
          session.sdkSessionId = msg.session_id;
        }
        const subtype = typeof msg.subtype === 'string' ? msg.subtype : 'system';
        emitEvent(sessionId, {
          type: 'status',
          turn_id: turnId,
          message: `sdk:${subtype}`,
        });
        continue;
      }

      if (msgType === 'stream_event') {
        const event = msg.event as Record<string, unknown> | undefined;
        if (!event) continue;
        if (event.type === 'content_block_delta') {
          const delta = event.delta as Record<string, unknown> | undefined;
          if (!delta) continue;
          if (delta.type === 'text_delta' && typeof delta.text === 'string') {
            streamedTextForCurrentMessage = true;
            emitEvent(sessionId, {
              type: 'text_delta',
              turn_id: turnId,
              delta: delta.text,
            });
          } else if (delta.type === 'thinking_delta' && typeof delta.thinking === 'string') {
            emitEvent(sessionId, {
              type: 'thinking_delta',
              turn_id: turnId,
              delta: delta.thinking,
            });
          }
        }
        continue;
      }

      if (msgType === 'assistant') {
        const inner = msg.message as Record<string, unknown> | undefined;
        const content = Array.isArray(inner?.content) ? inner.content : [];
        for (const block of content) {
          if (!block || typeof block !== 'object') continue;
          const b = block as Record<string, unknown>;
          if (b.type === 'tool_use') {
            const callId = String(b.id ?? `call-${toolNamesById.size + 1}`);
            const name = String(b.name ?? 'unknown');
            toolNamesById.set(callId, name);
            emitEvent(sessionId, {
              type: 'tool_call_started',
              turn_id: turnId,
              call_id: callId,
              name,
              arguments: b.input ?? {},
            });
          } else if (
            b.type === 'text' &&
            typeof b.text === 'string' &&
            !streamedTextForCurrentMessage
          ) {
            emitEvent(sessionId, {
              type: 'text_delta',
              turn_id: turnId,
              delta: b.text,
            });
          }
        }
        streamedTextForCurrentMessage = false;
        continue;
      }

      if (msgType === 'user') {
        const inner = msg.message as Record<string, unknown> | undefined;
        const content = Array.isArray(inner?.content) ? inner.content : [];
        for (const block of content) {
          if (!block || typeof block !== 'object') continue;
          const b = block as Record<string, unknown>;
          if (b.type === 'tool_result') {
            const callId = String(b.tool_use_id ?? 'unknown');
            const name = toolNamesById.get(callId) ?? 'unknown';
            emitEvent(sessionId, {
              type: 'tool_call_completed',
              turn_id: turnId,
              call_id: callId,
              name,
              is_error: Boolean(b.is_error),
              output: b.content ?? null,
            });
          }
        }
        continue;
      }

      if (msgType === 'result') {
        if (typeof msg.session_id === 'string') {
          session.sdkSessionId = msg.session_id;
        }
        const u = msg.usage as Record<string, unknown> | undefined;
        if (u && typeof u.input_tokens === 'number' && typeof u.output_tokens === 'number') {
          usage = {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
          };
        }
        const isError = Boolean(msg.is_error);
        const subtype = typeof msg.subtype === 'string' ? msg.subtype : '';
        if (isError) {
          const errText =
            typeof msg.result === 'string'
              ? msg.result
              : Array.isArray(msg.errors)
                ? msg.errors.map(String).join('; ')
                : `SDK turn failed (${subtype})`;
          emitEvent(sessionId, {
            type: 'error',
            turn_id: turnId,
            code: isAuthErrorMessage(errText) ? 'auth_required' : 'provider',
            message: errText,
          });
          emitEvent(sessionId, {
            type: 'turn_completed',
            turn_id: turnId,
            finish_reason: 'failed',
            usage,
          });
        } else {
          emitEvent(sessionId, {
            type: 'turn_completed',
            turn_id: turnId,
            finish_reason: subtype === 'error_max_turns' ? 'max_turns' : 'completed',
            usage,
          });
        }
        completedEmitted = true;
      }
    }

    if (!completedEmitted) {
      emitEvent(sessionId, {
        type: 'turn_completed',
        turn_id: turnId,
        finish_reason: turn.cancelled ? 'cancelled' : 'completed',
        usage,
      });
    }
  } catch (err) {
    const errMsg = err instanceof Error ? err.message : String(err);
    if (turn.cancelled || abortController.signal.aborted) {
      emitEvent(sessionId, {
        type: 'turn_completed',
        turn_id: turnId,
        finish_reason: 'cancelled',
        usage,
      });
    } else {
      emitEvent(sessionId, {
        type: 'error',
        turn_id: turnId,
        code: isAuthErrorMessage(errMsg) ? 'auth_required' : 'provider',
        message: errMsg,
      });
      emitEvent(sessionId, {
        type: 'turn_completed',
        turn_id: turnId,
        finish_reason: 'failed',
        usage,
      });
    }
  } finally {
    if (session.activeTurn?.turnId === turnId) {
      session.activeTurn = undefined;
    }
  }
}

/**
 * Deterministic test driver (`extra.test_driver = "direct_mcp"`) that connects
 * directly to the configured stdio MCP server (e.g. `mcp-bridge.js` wired to
 * Tauros's in-process `TaurosServer`), executes a real MCP `tools/call`
 * requested in the prompt JSON, and streams the resulting `AgentEvent`s back.
 * Used by Rust integration tests to verify the full sidecar + in-process MCP
 * pipeline without requiring an active cloud OAuth session.
 */
async function runDirectMcpTestTurn(
  session: SessionState,
  turn: ActiveTurn,
  message: UserMessage,
): Promise<void> {
  const { sessionId, config } = session;
  const { turnId, abortController } = turn;

  emitEvent(sessionId, { type: 'turn_started', turn_id: turnId });

  const serverEntry = Object.entries(config.mcp_servers ?? {}).find(
    ([, cfg]) => cfg.transport === 'stdio',
  );
  if (!serverEntry) {
    emitEvent(sessionId, {
      type: 'error',
      turn_id: turnId,
      code: 'invalid_config',
      message: 'direct_mcp test driver requires a stdio MCP server in session config',
    });
    emitEvent(sessionId, {
      type: 'turn_completed',
      turn_id: turnId,
      finish_reason: 'failed',
    });
    if (session.activeTurn?.turnId === turnId) {
      session.activeTurn = undefined;
    }
    return;
  }

  let directive: { tool: string; arguments?: Record<string, unknown>; delay_ms?: number };
  try {
    directive = JSON.parse(message.text) as {
      tool: string;
      arguments?: Record<string, unknown>;
      delay_ms?: number;
    };
  } catch {
    directive = { tool: 'tauros_status', arguments: {} };
  }

  if (directive.delay_ms && directive.delay_ms > 0) {
    emitEvent(sessionId, {
      type: 'status',
      turn_id: turnId,
      message: 'waiting before tool call',
    });
    const cancelledDuringDelay = await new Promise<boolean>((resolve) => {
      const timer = setTimeout(() => resolve(false), directive.delay_ms);
      abortController.signal.addEventListener(
        'abort',
        () => {
          clearTimeout(timer);
          resolve(true);
        },
        { once: true },
      );
    });
    if (cancelledDuringDelay || turn.cancelled) {
      emitEvent(sessionId, {
        type: 'turn_completed',
        turn_id: turnId,
        finish_reason: 'cancelled',
      });
      if (session.activeTurn?.turnId === turnId) {
        session.activeTurn = undefined;
      }
      return;
    }
  }

  const [, serverCfg] = serverEntry;
  if (serverCfg.transport !== 'stdio') {
    return;
  }

  const child = spawn(serverCfg.command, serverCfg.args ?? [], {
    stdio: ['pipe', 'pipe', 'pipe'],
    env: { ...process.env, ...(serverCfg.env ?? {}) },
  });

  const rl = readline.createInterface({ input: child.stdout });
  const pending = new Map<number, (resp: Record<string, unknown>) => void>();
  let nextId = 1;

  rl.on('line', (line) => {
    if (!line.trim()) return;
    try {
      const parsed = JSON.parse(line) as Record<string, unknown>;
      if (typeof parsed.id === 'number' && pending.has(parsed.id)) {
        const resolve = pending.get(parsed.id)!;
        pending.delete(parsed.id);
        resolve(parsed);
      }
    } catch {
      // ignore non-JSON lines
    }
  });

  const rpc = (method: string, params: unknown): Promise<Record<string, unknown>> =>
    new Promise((resolve, reject) => {
      const id = nextId++;
      pending.set(id, resolve);
      const req = JSON.stringify({ jsonrpc: '2.0', id, method, params });
      child.stdin.write(`${req}\n`, (err) => {
        if (err) {
          pending.delete(id);
          reject(err);
        }
      });
    });

  try {
    await rpc('initialize', {
      protocolVersion: '2025-11-25',
      capabilities: {},
      clientInfo: { name: 'tauros-sidecar-direct-mcp', version: '0.1.0' },
    });
    child.stdin.write(
      `${JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized', params: {} })}\n`,
    );

    const callId = `mcp-call-${turnId}`;
    const args = directive.arguments ?? {};
    emitEvent(sessionId, {
      type: 'tool_call_started',
      turn_id: turnId,
      call_id: callId,
      name: directive.tool,
      arguments: args,
    });

    const resp = await rpc('tools/call', {
      name: directive.tool,
      arguments: args,
    });

    const result = (resp.result ?? {}) as Record<string, unknown>;
    const isError = Boolean(result.isError);
    const content = Array.isArray(result.content) ? result.content : [];
    const firstText =
      content.length > 0 && typeof (content[0] as Record<string, unknown>).text === 'string'
        ? ((content[0] as Record<string, unknown>).text as string)
        : '';
    let parsedOutput: unknown = firstText;
    try {
      parsedOutput = JSON.parse(firstText);
    } catch {
      // keep string output
    }

    emitEvent(sessionId, {
      type: 'tool_call_completed',
      turn_id: turnId,
      call_id: callId,
      name: directive.tool,
      is_error: isError,
      output: parsedOutput,
    });

    emitEvent(sessionId, {
      type: 'text_delta',
      turn_id: turnId,
      delta: `Executed ${directive.tool}: ${firstText}`,
    });

    emitEvent(sessionId, {
      type: 'turn_completed',
      turn_id: turnId,
      finish_reason: 'completed',
    });
  } catch (err) {
    emitEvent(sessionId, {
      type: 'error',
      turn_id: turnId,
      code: 'transport',
      message: err instanceof Error ? err.message : String(err),
    });
    emitEvent(sessionId, {
      type: 'turn_completed',
      turn_id: turnId,
      finish_reason: 'failed',
    });
  } finally {
    rl.close();
    child.stdin.end();
    child.kill();
    if (session.activeTurn?.turnId === turnId) {
      session.activeTurn = undefined;
    }
  }
}

function handleCommand(cmd: SidecarCommand): void {
  switch (cmd.type) {
    case 'start_session': {
      // Per RES-002, AGT-002 supports only HostSignIn (unmodified host Claude Code sign-in).
      // API-key release auth belongs to AGT-005.
      if (cmd.config.auth_mode !== 'host_sign_in') {
        ackErr(
          cmd.id,
          'invalid_config',
          `unsupported auth_mode '${cmd.config.auth_mode}': AGT-002 uses 'host_sign_in' (RES-002); API-key release auth is AGT-005`,
        );
        return;
      }
      sessions.set(cmd.session_id, {
        sessionId: cmd.session_id,
        config: cmd.config,
      });
      ackOk(cmd.id);
      return;
    }

    case 'send_message': {
      const session = sessions.get(cmd.session_id);
      if (!session) {
        ackErr(cmd.id, 'session_not_found', `unknown session ${cmd.session_id}`);
        return;
      }
      if (session.activeTurn) {
        ackErr(cmd.id, 'busy', `session ${cmd.session_id} already has an active turn`);
        return;
      }
      const turn: ActiveTurn = {
        turnId: cmd.turn_id,
        abortController: new AbortController(),
        cancelled: false,
      };
      session.activeTurn = turn;
      ackOk(cmd.id);

      const useDirectMcp = session.config.extra?.test_driver === 'direct_mcp';
      const runner = useDirectMcp ? runDirectMcpTestTurn : runSdkTurn;
      void runner(session, turn, cmd.message);
      return;
    }

    case 'cancel': {
      const session = sessions.get(cmd.session_id);
      if (!session) {
        ackErr(cmd.id, 'session_not_found', `unknown session ${cmd.session_id}`);
        return;
      }
      if (!session.activeTurn || session.activeTurn.turnId !== cmd.turn_id) {
        ackErr(cmd.id, 'turn_not_found', `turn ${cmd.turn_id} is not active`);
        return;
      }
      session.activeTurn.cancelled = true;
      session.activeTurn.abortController.abort();
      ackOk(cmd.id);
      return;
    }

    case 'close_session': {
      const session = sessions.get(cmd.session_id);
      if (!session) {
        ackErr(cmd.id, 'session_not_found', `unknown session ${cmd.session_id}`);
        return;
      }
      if (session.activeTurn) {
        session.activeTurn.cancelled = true;
        session.activeTurn.abortController.abort();
      }
      sessions.delete(cmd.session_id);
      ackOk(cmd.id);
      return;
    }

    case 'shutdown': {
      for (const session of sessions.values()) {
        if (session.activeTurn) {
          session.activeTurn.cancelled = true;
          session.activeTurn.abortController.abort();
        }
      }
      sessions.clear();
      process.exit(0);
    }
  }
}

const input = readline.createInterface({
  input: process.stdin,
  terminal: false,
});

input.on('line', (line) => {
  const trimmed = line.trim();
  if (!trimmed) return;
  try {
    const cmd = JSON.parse(trimmed) as SidecarCommand;
    handleCommand(cmd);
  } catch (err) {
    process.stderr.write(
      `tauros-sidecar invalid command: ${err instanceof Error ? err.message : String(err)}\n`,
    );
  }
});

input.on('close', () => {
  for (const session of sessions.values()) {
    if (session.activeTurn) {
      session.activeTurn.cancelled = true;
      session.activeTurn.abortController.abort();
    }
  }
  process.exit(0);
});

emit({ type: 'ready', version: '0.1.0', sdk: '@anthropic-ai/claude-agent-sdk' });
