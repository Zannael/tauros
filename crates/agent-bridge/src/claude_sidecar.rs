//! Supervised Node.js sidecar adapter for `@anthropic-ai/claude-agent-sdk` (`AGT-002`).
//!
//! Launches `sidecar/claude-agent-sdk/dist/index.js` as a child process and speaks
//! newline-delimited JSON (NDJSON) over stdin/stdout. Per `RES-002`, personal use
//! relies exclusively on [`AuthMode::HostSignIn`] (the unmodified Claude Code
//! binary's own sign-in); Tauros never collects, reads, stores or intermediates
//! claude.ai credentials or tokens.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{
    event_channel, AgentBridge, AgentEvent, AuthMode, BridgeCapabilities, BridgeError, ErrorCode,
    EventSender, FinishReason, ModelOption, SessionConfig, SessionId, TurnId, TurnStream, UserMessage,
};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_STDERR_LINES: usize = 32;

/// Configuration for launching [`ClaudeSidecarAdapter`].
#[derive(Debug, Clone)]
pub struct ClaudeSidecarConfig {
    /// Node.js executable name or path (defaults to `"node"`).
    pub node_binary: PathBuf,
    /// Path to the compiled sidecar entry point (`sidecar/claude-agent-sdk/dist/index.js`).
    pub entry_script: PathBuf,
    /// Maximum time to wait for the sidecar's initial `ready` handshake.
    pub startup_timeout: Duration,
    /// Maximum time to wait for synchronous command acknowledgements.
    pub command_timeout: Duration,
}

impl ClaudeSidecarConfig {
    /// Creates a config pointing at `entry_script` with default Node binary and timeouts.
    pub fn new(entry_script: impl Into<PathBuf>) -> Self {
        Self {
            node_binary: PathBuf::from("node"),
            entry_script: entry_script.into(),
            startup_timeout: DEFAULT_TIMEOUT,
            command_timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Resolves the sidecar entry script relative to a Tauros repository root.
    pub fn from_repo_root(repo_root: &Path) -> Self {
        Self::new(
            repo_root
                .join("sidecar")
                .join("claude-agent-sdk")
                .join("dist")
                .join("index.js"),
        )
    }

    /// Resolves the `mcp-bridge.js` helper path beside `entry_script`.
    pub fn mcp_bridge_script(&self) -> PathBuf {
        self.entry_script
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("mcp-bridge.js")
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum OutboundCommand<'a> {
    StartSession {
        id: &'a str,
        session_id: &'a SessionId,
        config: &'a SessionConfig,
    },
    SendMessage {
        id: &'a str,
        session_id: &'a SessionId,
        turn_id: &'a TurnId,
        message: &'a UserMessage,
    },
    Cancel {
        id: &'a str,
        session_id: &'a SessionId,
        turn_id: &'a TurnId,
    },
    CloseSession {
        id: &'a str,
        session_id: &'a SessionId,
    },
    ListModels {
        id: &'a str,
        session_id: &'a SessionId,
    },
    SetModel {
        id: &'a str,
        session_id: &'a SessionId,
        model: &'a str,
    },
    Shutdown,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum InboundMessage {
    Ready {
        #[allow(dead_code)]
        version: String,
        #[allow(dead_code)]
        sdk: String,
    },
    Ack {
        id: String,
        ok: bool,
        #[serde(default)]
        code: Option<ErrorCode>,
        #[serde(default)]
        message: Option<String>,
        #[serde(default)]
        data: serde_json::Value,
    },
    Event {
        session_id: SessionId,
        event: AgentEvent,
    },
}

#[derive(Default)]
struct SharedState {
    pending_acks: HashMap<String, Sender<Result<serde_json::Value, BridgeError>>>,
    active_turns: HashMap<(SessionId, TurnId), EventSender>,
    stderr_tail: VecDeque<String>,
}

/// Supervised [`AgentBridge`] adapter backed by the TypeScript Claude Agent SDK sidecar.
pub struct ClaudeSidecarAdapter {
    child: Child,
    stdin: Option<ChildStdin>,
    state: Arc<Mutex<SharedState>>,
    stdout_thread: Option<JoinHandle<()>>,
    stderr_thread: Option<JoinHandle<()>>,
    next_req: AtomicU64,
    next_session: AtomicU64,
    next_turn: AtomicU64,
    command_timeout: Duration,
}

impl ClaudeSidecarAdapter {
    /// Spawns and performs the readiness handshake with the TypeScript sidecar.
    pub fn spawn(config: ClaudeSidecarConfig) -> Result<Self, BridgeError> {
        if !config.entry_script.is_file() {
            return Err(BridgeError::new(
                ErrorCode::InvalidConfig,
                format!(
                    "sidecar entry script not found: {} (run `npm --prefix sidecar/claude-agent-sdk run build`)",
                    config.entry_script.display()
                ),
            ));
        }

        let mut command = Command::new(&config.node_binary);
        crate::process::isolate(&mut command);
        let mut child = command
            .arg(&config.entry_script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                BridgeError::new(
                    ErrorCode::Transport,
                    format!(
                        "failed to spawn sidecar `{}`: {e}",
                        config.node_binary.display()
                    ),
                )
            })?;

        let stdin = child.stdin.take().ok_or_else(|| {
            BridgeError::new(ErrorCode::Transport, "sidecar stdin was not piped")
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            BridgeError::new(ErrorCode::Transport, "sidecar stdout was not piped")
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            BridgeError::new(ErrorCode::Transport, "sidecar stderr was not piped")
        })?;

        let state = Arc::new(Mutex::new(SharedState::default()));

        let stderr_state = Arc::clone(&state);
        let stderr_thread = thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                if let Ok(mut guard) = stderr_state.lock() {
                    if guard.stderr_tail.len() >= MAX_STDERR_LINES {
                        guard.stderr_tail.pop_front();
                    }
                    guard.stderr_tail.push_back(line);
                }
            }
        });

        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), BridgeError>>();
        let stdout_state = Arc::clone(&state);
        let stdout_thread = thread::spawn(move || {
            let reader = BufReader::new(stdout);
            let mut ready_tx = Some(ready_tx);
            for line in reader.lines() {
                let Ok(line) = line else {
                    break;
                };
                if line.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<InboundMessage>(&line) {
                    Ok(InboundMessage::Ready { .. }) => {
                        if let Some(tx) = ready_tx.take() {
                            let _ = tx.send(Ok(()));
                        }
                    }
                    Ok(InboundMessage::Ack {
                        id,
                        ok,
                        code,
                        message,
                        data,
                    }) => {
                        let result = if ok {
                            Ok(data)
                        } else {
                            Err(BridgeError::new(
                                code.unwrap_or(ErrorCode::Internal),
                                message.unwrap_or_else(|| "sidecar rejected command".to_owned()),
                            ))
                        };
                        if let Ok(mut guard) = stdout_state.lock() {
                            if let Some(tx) = guard.pending_acks.remove(&id) {
                                let _ = tx.send(result);
                            }
                        }
                    }
                    Ok(InboundMessage::Event { session_id, event }) => {
                        let turn_id = event_turn_id(&event).cloned();
                        let is_terminal = matches!(event, AgentEvent::TurnCompleted { .. });
                        if let Some(turn_id) = turn_id {
                            if let Ok(mut guard) = stdout_state.lock() {
                                let key = (session_id, turn_id);
                                if let Some(sender) = guard.active_turns.get(&key) {
                                    let _ = sender.send(event);
                                }
                                if is_terminal {
                                    guard.active_turns.remove(&key);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        if let Ok(mut guard) = stdout_state.lock() {
                            if guard.stderr_tail.len() >= MAX_STDERR_LINES {
                                guard.stderr_tail.pop_front();
                            }
                            guard
                                .stderr_tail
                                .push_back(format!("malformed sidecar stdout: {e}"));
                        }
                    }
                }
            }

            // Sidecar stdout closed: fail any waiting handshake, pending acks, or active turns.
            let diag = {
                let guard = stdout_state.lock().ok();
                guard
                    .map(|g| g.stderr_tail.iter().cloned().collect::<Vec<_>>().join(" | "))
                    .unwrap_or_default()
            };
            let msg = if diag.is_empty() {
                "sidecar process exited unexpectedly".to_owned()
            } else {
                format!("sidecar process exited unexpectedly: {diag}")
            };
            if let Some(tx) = ready_tx.take() {
                let _ = tx.send(Err(BridgeError::new(ErrorCode::Transport, msg.clone())));
            }
            if let Ok(mut guard) = stdout_state.lock() {
                for (_, tx) in guard.pending_acks.drain() {
                    let _ = tx.send(Err(BridgeError::new(ErrorCode::Transport, msg.clone())));
                }
                for ((_, turn_id), sender) in guard.active_turns.drain() {
                    let _ = sender.send(AgentEvent::Error {
                        turn_id: Some(turn_id.clone()),
                        code: ErrorCode::Transport,
                        message: msg.clone(),
                    });
                    let _ = sender.send(AgentEvent::TurnCompleted {
                        turn_id,
                        finish_reason: FinishReason::Failed,
                        usage: None,
                    });
                }
            }
        });

        let adapter = Self {
            child,
            stdin: Some(stdin),
            state,
            stdout_thread: Some(stdout_thread),
            stderr_thread: Some(stderr_thread),
            next_req: AtomicU64::new(1),
            next_session: AtomicU64::new(1),
            next_turn: AtomicU64::new(1),
            command_timeout: config.command_timeout,
        };

        match ready_rx.recv_timeout(config.startup_timeout) {
            Ok(Ok(())) => Ok(adapter),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(BridgeError::new(
                ErrorCode::Transport,
                "timed out waiting for sidecar ready handshake",
            )),
        }
    }

    /// Returns `true` if the supervised sidecar child process is still running.
    pub fn is_alive(&mut self) -> bool {
        #[cfg(target_os = "linux")]
        {
            // Do not reap here: group teardown in Drop retains the leader PID
            // until its descendants have been signalled.
            std::fs::read_to_string(format!("/proc/{}/stat", self.child.id()))
                .ok().and_then(|stat| stat.rsplit_once(") ").map(|(_, rest)| !rest.starts_with(['Z', 'X'])))
                .unwrap_or(false)
        }
        #[cfg(not(target_os = "linux"))]
        matches!(self.child.try_wait(), Ok(None))
    }

    fn send_command_with_ack(
        &mut self,
        req_id: &str,
        cmd: &OutboundCommand<'_>,
    ) -> Result<(), BridgeError> {
        self.send_command_with_data(req_id, cmd).map(|_| ())
    }

    /// Like [`Self::send_command_with_ack`], returning the ack's `data` payload.
    fn send_command_with_data(
        &mut self,
        req_id: &str,
        cmd: &OutboundCommand<'_>,
    ) -> Result<serde_json::Value, BridgeError> {
        let (tx, rx) = mpsc::channel();
        {
            let mut guard = self
                .state
                .lock()
                .map_err(|_| BridgeError::new(ErrorCode::Internal, "sidecar state poisoned"))?;
            guard.pending_acks.insert(req_id.to_owned(), tx);
        }

        if let Err(err) = self.write_command(cmd) {
            if let Ok(mut guard) = self.state.lock() {
                guard.pending_acks.remove(req_id);
            }
            return Err(err);
        }

        match rx.recv_timeout(self.command_timeout) {
            Ok(result) => result,
            Err(_) => {
                if let Ok(mut guard) = self.state.lock() {
                    guard.pending_acks.remove(req_id);
                }
                Err(BridgeError::new(
                    ErrorCode::Transport,
                    format!("timed out waiting for sidecar ack for request {req_id}"),
                ))
            }
        }
    }

    fn write_command(&mut self, cmd: &OutboundCommand<'_>) -> Result<(), BridgeError> {
        let stdin = self.stdin.as_mut().ok_or_else(|| {
            BridgeError::new(ErrorCode::Transport, "sidecar stdin is already closed")
        })?;
        let line = serde_json::to_string(cmd)
            .map_err(|e| BridgeError::new(ErrorCode::Internal, format!("serialize error: {e}")))?;
        writeln!(stdin, "{line}")
            .and_then(|()| stdin.flush())
            .map_err(|e| {
                BridgeError::new(
                    ErrorCode::Transport,
                    format!("failed writing command to sidecar: {e}"),
                )
            })
    }

    fn next_req_id(&self) -> String {
        format!("req-{}", self.next_req.fetch_add(1, Ordering::Relaxed))
    }
}

impl AgentBridge for ClaudeSidecarAdapter {
    fn capabilities(&self) -> BridgeCapabilities {
        BridgeCapabilities {
            provider_name: "claude-agent-sdk".to_owned(),
            supports_streaming: true,
            supports_tool_calls: true,
            supports_cancel: true,
            supported_auth: vec![AuthMode::HostSignIn],
        }
    }

    fn start_session(&mut self, config: SessionConfig) -> Result<SessionId, BridgeError> {
        if config.auth_mode != AuthMode::HostSignIn {
            return Err(BridgeError::new(
                ErrorCode::InvalidConfig,
                format!(
                    "unsupported auth_mode {:?}: AGT-002 uses AuthMode::HostSignIn (RES-002); API-key release auth is AGT-005",
                    config.auth_mode
                ),
            ));
        }

        let session_id = SessionId::new(format!(
            "claude-session-{}",
            self.next_session.fetch_add(1, Ordering::Relaxed)
        ));
        let req_id = self.next_req_id();
        self.send_command_with_ack(
            &req_id,
            &OutboundCommand::StartSession {
                id: &req_id,
                session_id: &session_id,
                config: &config,
            },
        )?;
        Ok(session_id)
    }

    fn send_message(
        &mut self,
        session_id: &SessionId,
        message: UserMessage,
    ) -> Result<TurnStream, BridgeError> {
        let turn_id = TurnId::new(format!(
            "turn-{}",
            self.next_turn.fetch_add(1, Ordering::Relaxed)
        ));
        let (sender, stream) = event_channel();

        // Register the stream before sending the command so early events are never dropped.
        {
            let mut guard = self
                .state
                .lock()
                .map_err(|_| BridgeError::new(ErrorCode::Internal, "sidecar state poisoned"))?;
            guard
                .active_turns
                .insert((session_id.clone(), turn_id.clone()), sender);
        }

        let req_id = self.next_req_id();
        if let Err(err) = self.send_command_with_ack(
            &req_id,
            &OutboundCommand::SendMessage {
                id: &req_id,
                session_id,
                turn_id: &turn_id,
                message: &message,
            },
        ) {
            if let Ok(mut guard) = self.state.lock() {
                guard
                    .active_turns
                    .remove(&(session_id.clone(), turn_id.clone()));
            }
            return Err(err);
        }

        Ok(TurnStream::new(turn_id, stream))
    }

    fn cancel(&mut self, session_id: &SessionId, turn_id: &TurnId) -> Result<(), BridgeError> {
        let req_id = self.next_req_id();
        self.send_command_with_ack(
            &req_id,
            &OutboundCommand::Cancel {
                id: &req_id,
                session_id,
                turn_id,
            },
        )
    }

    fn close_session(&mut self, session_id: &SessionId) -> Result<(), BridgeError> {
        let req_id = self.next_req_id();
        let res = self.send_command_with_ack(
            &req_id,
            &OutboundCommand::CloseSession {
                id: &req_id,
                session_id,
            },
        );
        if let Ok(mut guard) = self.state.lock() {
            guard.active_turns.retain(|(sid, _), _| sid != session_id);
        }
        res
    }

    fn list_models(&mut self, session_id: &SessionId) -> Result<Vec<ModelOption>, BridgeError> {
        let req_id = self.next_req_id();
        let data = self.send_command_with_data(&req_id, &OutboundCommand::ListModels { id: &req_id, session_id })?;
        serde_json::from_value(data)
            .map_err(|_| BridgeError::new(ErrorCode::Transport, "sidecar returned an invalid model list"))
    }

    fn set_model(&mut self, session_id: &SessionId, model: &str) -> Result<(), BridgeError> {
        let req_id = self.next_req_id();
        self.send_command_with_ack(&req_id, &OutboundCommand::SetModel { id: &req_id, session_id, model })
    }
}

impl Drop for ClaudeSidecarAdapter {
    fn drop(&mut self) {
        let _ = self.write_command(&OutboundCommand::Shutdown);
        self.stdin.take();

        // Give the protocol shutdown a short grace period, then reap the
        // entire group, including CLI/MCP children retaining inherited pipes.
        thread::sleep(Duration::from_millis(200));
        crate::process::terminate(&mut self.child);

        if let Some(handle) = self.stdout_thread.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.stderr_thread.take() {
            let _ = handle.join();
        }
    }
}

fn event_turn_id(event: &AgentEvent) -> Option<&TurnId> {
    match event {
        AgentEvent::TurnStarted { turn_id }
        | AgentEvent::TextDelta { turn_id, .. }
        | AgentEvent::ThinkingDelta { turn_id, .. }
        | AgentEvent::ToolCallStarted { turn_id, .. }
        | AgentEvent::ToolCallCompleted { turn_id, .. }
        | AgentEvent::Status { turn_id, .. }
        | AgentEvent::TurnCompleted { turn_id, .. } => Some(turn_id),
        AgentEvent::Error { turn_id, .. } => turn_id.as_ref(),
    }
}
