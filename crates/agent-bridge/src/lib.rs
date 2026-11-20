//! Layer 2: the vendor-independent agent bridge (`AGT-001`) with adapters for the
//! Agent SDK sidecar, Codex and Gemini (`AGT-002`, `AGT-006`, `AGT-007`).
//!
//! The bridge defines a synchronous, runtime-agnostic contract so Layer 1
//! (`tauros-app`) and tests can swap agent backends without coupling to any
//! vendor SDK, wire protocol or async runtime. Adapters may drive child-process
//! sidecars or async clients internally while pushing [`AgentEvent`]s onto an
//! [`EventStream`].

pub mod claude_sidecar;
pub mod codex;
pub mod fake;

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::time::Duration;

use serde::{Deserialize, Serialize};

macro_rules! id_newtype {
    ($name:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Wraps a non-empty identifier string.
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// Returns the underlying string slice.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }
    };
}

id_newtype!(SessionId, "Opaque identifier for an open agent session.");
id_newtype!(TurnId, "Opaque identifier for a single prompt/response turn.");
id_newtype!(ToolCallId, "Opaque identifier correlating a tool call start and completion.");

/// Authentication category requested for an agent session.
///
/// The bridge models only the authentication mechanism, never raw credentials;
/// adapters are responsible for reading secrets from the user's configured
/// local store or environment and keeping them out of logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    /// No authentication (used by local or fake test adapters).
    None,
    /// Unmodified host CLI sign-in for personal use (e.g. Claude Code / Codex CLI).
    HostSignIn,
    /// User-supplied provider API key (suitable for public releases).
    ApiKey,
    /// Cloud provider credentials such as Vertex AI Application Default Credentials.
    CloudCredentials,
    /// Provider OAuth plan flow (e.g. Sign in with ChatGPT).
    OAuthPlan,
}

/// Static capabilities advertised by an [`AgentBridge`] adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeCapabilities {
    /// Human-readable adapter identifier (e.g. `"claude-agent-sdk"`, `"codex"`, `"gemini"`).
    pub provider_name: String,
    /// Whether the adapter emits incremental text or thinking deltas during a turn.
    pub supports_streaming: bool,
    /// Whether the adapter reports MCP / tool call lifecycle events.
    pub supports_tool_calls: bool,
    /// Whether in-flight turns can be cancelled via [`AgentBridge::cancel`].
    pub supports_cancel: bool,
    /// Authentication modes accepted by this adapter.
    pub supported_auth: Vec<AuthMode>,
}

/// Transport configuration for an MCP server exposed to the agent runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "snake_case")]
pub enum McpServerConfig {
    /// Local stdio subprocess launched by the agent runtime.
    Stdio {
        /// Executable path or command name.
        command: String,
        /// Command-line arguments.
        #[serde(default)]
        args: Vec<String>,
        /// Extra environment variables (must not hold provider credentials).
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
    /// Streamable HTTP endpoint (localhost or configured URL).
    StreamableHttp {
        /// Endpoint URL.
        url: String,
    },
}

/// Configuration used when opening a session with [`AgentBridge::start_session`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionConfig {
    /// Working directory for the agent session, if applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<PathBuf>,
    /// Optional system prompt or session instructions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    /// Optional model selector understood by the target adapter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Named MCP servers (such as `"tauros"`) to attach to the session.
    #[serde(default)]
    pub mcp_servers: BTreeMap<String, McpServerConfig>,
    /// Authentication mode requested for this session.
    pub auth_mode: AuthMode,
    /// Non-secret adapter-specific options.
    #[serde(default)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            working_dir: None,
            system_prompt: None,
            model: None,
            mcp_servers: BTreeMap::new(),
            auth_mode: AuthMode::None,
            extra: BTreeMap::new(),
        }
    }
}

/// Optional structured attachment sent alongside a user prompt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Attachment {
    /// Reference to a local file path relevant to the prompt.
    Path {
        /// Path to the referenced file.
        path: PathBuf,
        /// Optional display label.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    /// Structured editor context (e.g. currently selected map or trainer).
    Context {
        /// Context category (e.g. `"selected_map"`, `"selected_trainer"`).
        category: String,
        /// Arbitrary JSON payload describing the context.
        data: serde_json::Value,
    },
}

/// Message submitted by the user (from chat or voice transcription) to start a turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserMessage {
    /// Plain-text prompt content.
    pub text: String,
    /// Optional file or editor-context attachments.
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

impl UserMessage {
    /// Creates a plain-text user message with no attachments.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            attachments: Vec::new(),
        }
    }

    /// Appends an attachment and returns the updated message.
    pub fn with_attachment(mut self, attachment: Attachment) -> Self {
        self.attachments.push(attachment);
        self
    }
}

/// Reason a turn stopped emitting events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// The agent completed its response normally.
    Completed,
    /// The turn was cancelled via [`AgentBridge::cancel`].
    Cancelled,
    /// The turn terminated due to an error.
    Failed,
    /// The adapter stopped because it reached its configured turn/step limit.
    MaxTurns,
}

/// Optional token accounting reported by an adapter at the end of a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Prompt / input tokens billed or consumed by the turn.
    pub input_tokens: u64,
    /// Generated / output tokens billed or consumed by the turn.
    pub output_tokens: u64,
}

/// Machine-readable error category returned by [`BridgeError`] and [`AgentEvent::Error`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Required authentication credentials or sign-in are missing.
    AuthRequired,
    /// Authentication failed or was rejected by the provider.
    AuthFailed,
    /// Session configuration or message input is invalid.
    InvalidConfig,
    /// The referenced [`SessionId`] does not exist or has been closed.
    SessionNotFound,
    /// The referenced [`TurnId`] does not exist or is no longer active.
    TurnNotFound,
    /// The session already has an active turn and cannot accept another concurrently.
    Busy,
    /// Communication with the sidecar or remote runtime failed.
    Transport,
    /// Operation was cancelled.
    Cancelled,
    /// The upstream provider returned an error.
    Provider,
    /// Unexpected internal error in the adapter.
    Internal,
}

/// Error returned by [`AgentBridge`] operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeError {
    /// Structured error category.
    pub code: ErrorCode,
    /// Human-readable description (must never contain secrets).
    pub message: String,
}

impl BridgeError {
    /// Constructs a new [`BridgeError`].
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for BridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for BridgeError {}

/// Incremental event streamed by an adapter while processing a turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    /// Marks the start of a turn.
    TurnStarted {
        /// Identifier of the active turn.
        turn_id: TurnId,
    },
    /// Incremental assistant text chunk.
    TextDelta {
        /// Identifier of the active turn.
        turn_id: TurnId,
        /// UTF-8 text fragment to append to the response.
        delta: String,
    },
    /// Incremental reasoning / thinking chunk, if exposed by the adapter.
    ThinkingDelta {
        /// Identifier of the active turn.
        turn_id: TurnId,
        /// UTF-8 reasoning fragment.
        delta: String,
    },
    /// An MCP or runtime tool invocation has started.
    ToolCallStarted {
        /// Identifier of the active turn.
        turn_id: TurnId,
        /// Correlation identifier for the matching [`AgentEvent::ToolCallCompleted`].
        call_id: ToolCallId,
        /// Tool name (e.g. `"create_trainer"`).
        name: String,
        /// Structured JSON arguments passed to the tool.
        arguments: serde_json::Value,
    },
    /// An MCP or runtime tool invocation has finished.
    ToolCallCompleted {
        /// Identifier of the active turn.
        turn_id: TurnId,
        /// Correlation identifier from [`AgentEvent::ToolCallStarted`].
        call_id: ToolCallId,
        /// Tool name.
        name: String,
        /// Whether the tool reported an error.
        is_error: bool,
        /// Structured JSON result or error payload.
        output: serde_json::Value,
    },
    /// Non-fatal progress or status message for UI display.
    Status {
        /// Identifier of the active turn.
        turn_id: TurnId,
        /// Human-readable status text.
        message: String,
    },
    /// Terminal event signalling that the turn has ended.
    TurnCompleted {
        /// Identifier of the finished turn.
        turn_id: TurnId,
        /// Why the turn ended.
        finish_reason: FinishReason,
        /// Optional token usage reported by the provider.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<TokenUsage>,
    },
    /// Error emitted during streaming.
    Error {
        /// Turn identifier, if the error is scoped to a turn.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turn_id: Option<TurnId>,
        /// Machine-readable error category.
        code: ErrorCode,
        /// Human-readable error message.
        message: String,
    },
}

/// Producer half of an [`EventStream`], used by adapters to push streamed events.
#[derive(Debug, Clone)]
pub struct EventSender {
    tx: Sender<AgentEvent>,
}

impl EventSender {
    /// Pushes an event to the stream consumer.
    pub fn send(&self, event: AgentEvent) -> Result<(), BridgeError> {
        self.tx
            .send(event)
            .map_err(|_| BridgeError::new(ErrorCode::Cancelled, "event stream consumer dropped"))
    }
}

/// Consumer half of a turn's event stream.
#[derive(Debug)]
pub struct EventStream {
    rx: Receiver<AgentEvent>,
}

impl EventStream {
    /// Blocks until the next [`AgentEvent`] arrives, or returns `None` when closed.
    pub fn next_event(&self) -> Option<AgentEvent> {
        self.rx.recv().ok()
    }

    /// Returns the next buffered [`AgentEvent`] without blocking.
    pub fn try_next(&self) -> Result<AgentEvent, TryRecvError> {
        self.rx.try_recv()
    }

    /// Blocks up to `timeout` waiting for the next [`AgentEvent`].
    pub fn recv_timeout(&self, timeout: Duration) -> Result<AgentEvent, RecvTimeoutError> {
        self.rx.recv_timeout(timeout)
    }

    /// Drains all remaining events until the producer closes the channel.
    pub fn collect_all(self) -> Vec<AgentEvent> {
        self.into_iter().collect()
    }
}

impl Iterator for EventStream {
    type Item = AgentEvent;

    fn next(&mut self) -> Option<Self::Item> {
        self.rx.recv().ok()
    }
}

/// Creates a connected `(`[`EventSender`]`, `[`EventStream`]`)` pair.
pub fn event_channel() -> (EventSender, EventStream) {
    let (tx, rx) = mpsc::channel();
    (EventSender { tx }, EventStream { rx })
}

/// Handle returned by [`AgentBridge::send_message`] for reading a turn's events.
#[derive(Debug)]
pub struct TurnStream {
    turn_id: TurnId,
    events: EventStream,
}

impl TurnStream {
    /// Creates a [`TurnStream`] from a turn identifier and its [`EventStream`].
    pub fn new(turn_id: TurnId, events: EventStream) -> Self {
        Self { turn_id, events }
    }

    /// Returns the identifier of the turn.
    pub fn turn_id(&self) -> &TurnId {
        &self.turn_id
    }

    /// Borrows the underlying [`EventStream`].
    pub fn events(&self) -> &EventStream {
        &self.events
    }

    /// Mutably borrows the underlying [`EventStream`].
    pub fn events_mut(&mut self) -> &mut EventStream {
        &mut self.events
    }

    /// Consumes the handle and returns `(TurnId, EventStream)`.
    pub fn into_parts(self) -> (TurnId, EventStream) {
        (self.turn_id, self.events)
    }
}

/// Vendor-independent interface implemented by every Tauros agent adapter.
///
/// Adapters are expected to be object-safe (`Box<dyn AgentBridge>`) so Layer 1
/// can switch between Claude Agent SDK (`AGT-002`), Codex (`AGT-006`), Gemini
/// (`AGT-007`) and test fakes at runtime.
pub trait AgentBridge: Send {
    /// Returns the static capabilities of this adapter.
    fn capabilities(&self) -> BridgeCapabilities;

    /// Opens a new agent session with the given [`SessionConfig`].
    fn start_session(&mut self, config: SessionConfig) -> Result<SessionId, BridgeError>;

    /// Submits a [`UserMessage`] to an open session and returns a [`TurnStream`]
    /// that yields incremental [`AgentEvent`]s until a terminal event is emitted.
    fn send_message(
        &mut self,
        session_id: &SessionId,
        message: UserMessage,
    ) -> Result<TurnStream, BridgeError>;

    /// Requests cancellation of an in-flight turn.
    ///
    /// Adapters that support cancellation should stop the turn promptly and emit
    /// [`AgentEvent::TurnCompleted`] with [`FinishReason::Cancelled`].
    fn cancel(&mut self, session_id: &SessionId, turn_id: &TurnId) -> Result<(), BridgeError>;

    /// Closes an open session and releases its resources.
    fn close_session(&mut self, session_id: &SessionId) -> Result<(), BridgeError>;
}
