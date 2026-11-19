//! Deterministic in-process fake adapter for testing [`AgentBridge`] consumers.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;

use crate::{
    event_channel, AgentBridge, AgentEvent, AuthMode, BridgeCapabilities, BridgeError, ErrorCode,
    FinishReason, SessionConfig, SessionId, TokenUsage, ToolCallId, TurnId, TurnStream,
    UserMessage,
};

/// Single scripted step executed by [`FakeAdapter`] during a turn.
#[derive(Debug, Clone)]
pub enum FakeStep {
    /// Emit a [`AgentEvent::TextDelta`].
    Text(String),
    /// Emit a [`AgentEvent::ThinkingDelta`].
    Thinking(String),
    /// Emit a [`AgentEvent::Status`].
    Status(String),
    /// Emit [`AgentEvent::ToolCallStarted`] followed by [`AgentEvent::ToolCallCompleted`].
    ToolCall {
        /// Tool call correlation id.
        call_id: ToolCallId,
        /// Tool name.
        name: String,
        /// Tool input arguments.
        arguments: serde_json::Value,
        /// Whether the tool call reports an error.
        is_error: bool,
        /// Tool output payload.
        output: serde_json::Value,
    },
    /// Pause the fake worker until either the test releases the gate or
    /// [`AgentBridge::cancel`] is called for this turn.
    WaitForGate,
}

/// Scripted response returned by [`FakeAdapter`] for one [`AgentBridge::send_message`] call.
#[derive(Debug, Clone)]
pub struct FakeTurnScript {
    /// Ordered steps to emit.
    pub steps: Vec<FakeStep>,
    /// Optional token usage reported on normal completion.
    pub usage: Option<TokenUsage>,
}

impl FakeTurnScript {
    /// Creates a script that streams a sequence of text chunks and finishes cleanly.
    pub fn text_chunks(chunks: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            steps: chunks
                .into_iter()
                .map(|c| FakeStep::Text(c.into()))
                .collect(),
            usage: None,
        }
    }
}

struct FakeSession {
    config: SessionConfig,
    messages: Vec<UserMessage>,
    turns: BTreeMap<TurnId, ActiveTurn>,
}

struct ActiveTurn {
    cancelled: Arc<AtomicBool>,
    gate_tx: Option<mpsc::Sender<()>>,
    finished: Arc<AtomicBool>,
}

/// In-process [`AgentBridge`] implementation for tests and UI previews.
pub struct FakeAdapter {
    supported_auth: Vec<AuthMode>,
    scripts: VecDeque<FakeTurnScript>,
    sessions: BTreeMap<SessionId, FakeSession>,
    next_session: u64,
    next_turn: u64,
}

impl Default for FakeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeAdapter {
    /// Creates a new fake adapter accepting [`AuthMode::None`] by default.
    pub fn new() -> Self {
        Self {
            supported_auth: vec![AuthMode::None],
            scripts: VecDeque::new(),
            sessions: BTreeMap::new(),
            next_session: 1,
            next_turn: 1,
        }
    }

    /// Configures which [`AuthMode`]s this fake adapter accepts in [`AgentBridge::start_session`].
    pub fn with_supported_auth(mut self, modes: Vec<AuthMode>) -> Self {
        self.supported_auth = modes;
        self
    }

    /// Queues a [`FakeTurnScript`] to be played on the next [`AgentBridge::send_message`].
    pub fn push_script(&mut self, script: FakeTurnScript) {
        self.scripts.push_back(script);
    }

    /// Releases a [`FakeStep::WaitForGate`] pause on an active turn so it can continue.
    pub fn release_gate(&self, session_id: &SessionId, turn_id: &TurnId) -> Result<(), BridgeError> {
        let session = self.sessions.get(session_id).ok_or_else(|| {
            BridgeError::new(
                ErrorCode::SessionNotFound,
                format!("unknown session {session_id}"),
            )
        })?;
        let turn = session.turns.get(turn_id).ok_or_else(|| {
            BridgeError::new(ErrorCode::TurnNotFound, format!("unknown turn {turn_id}"))
        })?;
        if let Some(tx) = &turn.gate_tx {
            let _ = tx.send(());
        }
        Ok(())
    }

    /// Returns the messages recorded for an open session.
    pub fn recorded_messages(&self, session_id: &SessionId) -> Option<&[UserMessage]> {
        self.sessions
            .get(session_id)
            .map(|session| session.messages.as_slice())
    }

    /// Returns the [`SessionConfig`] stored for an open session.
    pub fn session_config(&self, session_id: &SessionId) -> Option<&SessionConfig> {
        self.sessions.get(session_id).map(|session| &session.config)
    }
}

impl AgentBridge for FakeAdapter {
    fn capabilities(&self) -> BridgeCapabilities {
        BridgeCapabilities {
            provider_name: "fake".to_owned(),
            supports_streaming: true,
            supports_tool_calls: true,
            supports_cancel: true,
            supported_auth: self.supported_auth.clone(),
        }
    }

    fn start_session(&mut self, config: SessionConfig) -> Result<SessionId, BridgeError> {
        if !self.supported_auth.contains(&config.auth_mode) {
            return Err(BridgeError::new(
                ErrorCode::AuthRequired,
                format!("auth mode {:?} is not supported by fake adapter", config.auth_mode),
            ));
        }
        let id = SessionId::new(format!("fake-session-{}", self.next_session));
        self.next_session += 1;
        self.sessions.insert(
            id.clone(),
            FakeSession {
                config,
                messages: Vec::new(),
                turns: BTreeMap::new(),
            },
        );
        Ok(id)
    }

    fn send_message(
        &mut self,
        session_id: &SessionId,
        message: UserMessage,
    ) -> Result<TurnStream, BridgeError> {
        if message.text.trim().is_empty() && message.attachments.is_empty() {
            return Err(BridgeError::new(
                ErrorCode::InvalidConfig,
                "message must contain text or at least one attachment",
            ));
        }

        let session = self.sessions.get_mut(session_id).ok_or_else(|| {
            BridgeError::new(
                ErrorCode::SessionNotFound,
                format!("unknown session {session_id}"),
            )
        })?;

        let turn_id = TurnId::new(format!("fake-turn-{}", self.next_turn));
        self.next_turn += 1;

        let script = self.scripts.pop_front().unwrap_or_else(|| {
            FakeTurnScript::text_chunks([format!("echo: {}", message.text)])
        });
        session.messages.push(message);

        let (sender, stream) = event_channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let (gate_tx, gate_rx) = mpsc::channel();

        session.turns.insert(
            turn_id.clone(),
            ActiveTurn {
                cancelled: Arc::clone(&cancelled),
                gate_tx: Some(gate_tx),
                finished: Arc::clone(&finished),
            },
        );

        let worker_turn_id = turn_id.clone();
        thread::spawn(move || {
            let _ = sender.send(AgentEvent::TurnStarted {
                turn_id: worker_turn_id.clone(),
            });

            for step in script.steps {
                if cancelled.load(Ordering::SeqCst) {
                    let _ = sender.send(AgentEvent::TurnCompleted {
                        turn_id: worker_turn_id,
                        finish_reason: FinishReason::Cancelled,
                        usage: None,
                    });
                    finished.store(true, Ordering::SeqCst);
                    return;
                }

                match step {
                    FakeStep::Text(delta) => {
                        if sender
                            .send(AgentEvent::TextDelta {
                                turn_id: worker_turn_id.clone(),
                                delta,
                            })
                            .is_err()
                        {
                            finished.store(true, Ordering::SeqCst);
                            return;
                        }
                    }
                    FakeStep::Thinking(delta) => {
                        if sender
                            .send(AgentEvent::ThinkingDelta {
                                turn_id: worker_turn_id.clone(),
                                delta,
                            })
                            .is_err()
                        {
                            finished.store(true, Ordering::SeqCst);
                            return;
                        }
                    }
                    FakeStep::Status(message) => {
                        if sender
                            .send(AgentEvent::Status {
                                turn_id: worker_turn_id.clone(),
                                message,
                            })
                            .is_err()
                        {
                            finished.store(true, Ordering::SeqCst);
                            return;
                        }
                    }
                    FakeStep::ToolCall {
                        call_id,
                        name,
                        arguments,
                        is_error,
                        output,
                    } => {
                        if sender
                            .send(AgentEvent::ToolCallStarted {
                                turn_id: worker_turn_id.clone(),
                                call_id: call_id.clone(),
                                name: name.clone(),
                                arguments,
                            })
                            .is_err()
                        {
                            finished.store(true, Ordering::SeqCst);
                            return;
                        }
                        if cancelled.load(Ordering::SeqCst) {
                            let _ = sender.send(AgentEvent::TurnCompleted {
                                turn_id: worker_turn_id,
                                finish_reason: FinishReason::Cancelled,
                                usage: None,
                            });
                            finished.store(true, Ordering::SeqCst);
                            return;
                        }
                        if sender
                            .send(AgentEvent::ToolCallCompleted {
                                turn_id: worker_turn_id.clone(),
                                call_id,
                                name,
                                is_error,
                                output,
                            })
                            .is_err()
                        {
                            finished.store(true, Ordering::SeqCst);
                            return;
                        }
                    }
                    FakeStep::WaitForGate => {
                        let _ = gate_rx.recv();
                        if cancelled.load(Ordering::SeqCst) {
                            let _ = sender.send(AgentEvent::TurnCompleted {
                                turn_id: worker_turn_id,
                                finish_reason: FinishReason::Cancelled,
                                usage: None,
                            });
                            finished.store(true, Ordering::SeqCst);
                            return;
                        }
                    }
                }
            }

            let finish_reason = if cancelled.load(Ordering::SeqCst) {
                FinishReason::Cancelled
            } else {
                FinishReason::Completed
            };
            let _ = sender.send(AgentEvent::TurnCompleted {
                turn_id: worker_turn_id,
                finish_reason,
                usage: if finish_reason == FinishReason::Completed {
                    script.usage
                } else {
                    None
                },
            });
            finished.store(true, Ordering::SeqCst);
        });

        Ok(TurnStream::new(turn_id, stream))
    }

    fn cancel(&mut self, session_id: &SessionId, turn_id: &TurnId) -> Result<(), BridgeError> {
        let session = self.sessions.get_mut(session_id).ok_or_else(|| {
            BridgeError::new(
                ErrorCode::SessionNotFound,
                format!("unknown session {session_id}"),
            )
        })?;
        let turn = session.turns.get_mut(turn_id).ok_or_else(|| {
            BridgeError::new(ErrorCode::TurnNotFound, format!("unknown turn {turn_id}"))
        })?;
        turn.cancelled.store(true, Ordering::SeqCst);
        if let Some(tx) = turn.gate_tx.take() {
            let _ = tx.send(());
        }
        Ok(())
    }

    fn close_session(&mut self, session_id: &SessionId) -> Result<(), BridgeError> {
        let mut session = self.sessions.remove(session_id).ok_or_else(|| {
            BridgeError::new(
                ErrorCode::SessionNotFound,
                format!("unknown session {session_id}"),
            )
        })?;
        for turn in session.turns.values_mut() {
            if !turn.finished.load(Ordering::SeqCst) {
                turn.cancelled.store(true, Ordering::SeqCst);
                if let Some(tx) = turn.gate_tx.take() {
                    let _ = tx.send(());
                }
            }
        }
        Ok(())
    }
}
