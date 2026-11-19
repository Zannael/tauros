/**
 * Wire protocol shared between `crates/agent-bridge/src/claude_sidecar.rs`
 * and the TypeScript Claude Agent SDK sidecar.
 *
 * All messages are newline-delimited JSON (NDJSON) over stdin/stdout.
 * Diagnostics go exclusively to stderr.
 */

export type AuthMode =
  | 'none'
  | 'host_sign_in'
  | 'api_key'
  | 'cloud_credentials'
  | 'oauth_plan';

export type McpServerConfig =
  | {
      transport: 'stdio';
      command: string;
      args?: string[];
      env?: Record<string, string>;
    }
  | {
      transport: 'streamable_http';
      url: string;
    };

export interface SessionConfig {
  working_dir?: string;
  system_prompt?: string;
  model?: string;
  mcp_servers?: Record<string, McpServerConfig>;
  auth_mode: AuthMode;
  extra?: Record<string, unknown>;
}

export type Attachment =
  | { kind: 'path'; path: string; label?: string }
  | { kind: 'context'; category: string; data: unknown };

export interface UserMessage {
  text: string;
  attachments?: Attachment[];
}

export type ErrorCode =
  | 'auth_required'
  | 'auth_failed'
  | 'invalid_config'
  | 'session_not_found'
  | 'turn_not_found'
  | 'busy'
  | 'transport'
  | 'cancelled'
  | 'provider'
  | 'internal';

export type FinishReason = 'completed' | 'cancelled' | 'failed' | 'max_turns';

export interface TokenUsage {
  input_tokens: number;
  output_tokens: number;
}

export type AgentEvent =
  | { type: 'turn_started'; turn_id: string }
  | { type: 'text_delta'; turn_id: string; delta: string }
  | { type: 'thinking_delta'; turn_id: string; delta: string }
  | {
      type: 'tool_call_started';
      turn_id: string;
      call_id: string;
      name: string;
      arguments: unknown;
    }
  | {
      type: 'tool_call_completed';
      turn_id: string;
      call_id: string;
      name: string;
      is_error: boolean;
      output: unknown;
    }
  | { type: 'status'; turn_id: string; message: string }
  | {
      type: 'turn_completed';
      turn_id: string;
      finish_reason: FinishReason;
      usage?: TokenUsage;
    }
  | {
      type: 'error';
      turn_id?: string;
      code: ErrorCode;
      message: string;
    };

export type SidecarCommand =
  | {
      type: 'start_session';
      id: string;
      session_id: string;
      config: SessionConfig;
    }
  | {
      type: 'send_message';
      id: string;
      session_id: string;
      turn_id: string;
      message: UserMessage;
    }
  | {
      type: 'cancel';
      id: string;
      session_id: string;
      turn_id: string;
    }
  | {
      type: 'close_session';
      id: string;
      session_id: string;
    }
  | { type: 'shutdown' };

export type SidecarMessage =
  | { type: 'ready'; version: string; sdk: string }
  | {
      type: 'ack';
      id: string;
      ok: true;
    }
  | {
      type: 'ack';
      id: string;
      ok: false;
      code: ErrorCode;
      message: string;
    }
  | {
      type: 'event';
      session_id: string;
      event: AgentEvent;
    };
