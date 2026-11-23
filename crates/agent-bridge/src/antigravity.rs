//! Antigravity client over the Agent Client Protocol (AGT-007).
//!
//! Tauros is an ACP client of the unmodified `agy acp` server, the route Google
//! documents for "any ACP client". The server reuses the subscription sign-in
//! cached by Antigravity CLI; Tauros never reads, stores or forwards credentials
//! and strips API-key and cloud credential variables so no paid route is chosen.
//!
//! Every tool call pauses on `session/request_permission`. Only tools of the MCP
//! servers Tauros attached are allowed; built-in file edits, reads, commands,
//! browser and any unknown request are denied. Native project edits therefore go
//! through host MCP tools that snapshot before writing. Runtime state and settings
//! live in a dedicated directory, so the user's own Antigravity MCP servers and
//! permission rules are not loaded.
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::*;
use serde_json::{json, Value};

/// ACP error code `agy acp` returns when no cached sign-in is available.
const AUTH_REQUIRED: i64 = -32000;

/// Environment variables that would select a paid or cloud credential route.
const CREDENTIAL_ENV: &[&str] = &[
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
    "GOOGLE_APPLICATION_CREDENTIALS",
    "GOOGLE_CLOUD_PROJECT",
    "GOOGLE_CLOUD_LOCATION",
    "GOOGLE_GENAI_USE_VERTEXAI",
    "AGY_ADC_AUTH",
];

#[derive(Clone, Debug)]
pub struct AntigravityConfig {
    /// `agy` executable; a bare name is searched on absolute `PATH` entries.
    pub binary: PathBuf,
    /// Dedicated runtime directory (settings, conversations and the empty
    /// workspace). Must be outside the game project.
    pub home: PathBuf,
    /// Extra arguments after `acp`, such as `--sandbox`.
    pub extra_args: Vec<String>,
    pub timeout: Duration,
}

impl AntigravityConfig {
    pub fn new(home: impl Into<PathBuf>) -> Self {
        Self {
            binary: "agy".into(),
            home: home.into(),
            extra_args: Vec::new(),
            timeout: Duration::from_secs(60),
        }
    }
}

fn resolve_binary(requested: &Path, search_path: Option<&std::ffi::OsStr>) -> Result<PathBuf, BridgeError> {
    fn executable(path: &Path) -> bool {
        std::fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
    }
    let found = if requested.is_absolute() || requested.components().count() > 1 {
        executable(requested).then(|| requested.to_owned())
    } else {
        search_path.and_then(|paths| std::env::split_paths(paths)
            // Do not discover game-local executables through relative PATH entries.
            .filter(|dir| dir.is_absolute())
            .map(|dir| dir.join(requested)).find(|path| executable(path)))
    };
    let found = found.ok_or_else(|| transport(
        "Antigravity CLI not found. Install it or set TAUROS_AGY_BIN to the agy executable, then restart Tauros."))?;
    std::fs::canonicalize(found).map_err(|_| transport("Cannot resolve the agy executable; check TAUROS_AGY_BIN"))
}

fn transport(message: impl Into<String>) -> BridgeError {
    BridgeError::new(ErrorCode::Transport, message)
}

fn write(stdin: &Mutex<ChildStdin>, value: Value) -> Result<(), BridgeError> {
    let mut stdin = stdin.lock().map_err(|_| transport("Antigravity stdin poisoned"))?;
    writeln!(stdin, "{value}").and_then(|()| stdin.flush()).map_err(|e| transport(e.to_string()))
}

/// Allow only calls to tools of the MCP servers Tauros attached. ACP reports
/// them as `mcp_<server>_<tool>`; server names are restricted to a fixed
/// alphabet without `_` so prefixes cannot overlap.
pub fn permission_allowed(tool_call: &Value, servers: &HashSet<String>) -> bool {
    let name = tool_call["name"].as_str().unwrap_or("");
    split_mcp_name(name, servers).is_some()
}

fn split_mcp_name<'a>(name: &'a str, servers: &HashSet<String>) -> Option<(&'a str, &'a str)> {
    let rest = name.strip_prefix("mcp_")?;
    let (server, tool) = rest.split_once('_')?;
    (servers.contains(server) && !tool.is_empty()).then_some((server, tool))
}

fn valid_server_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Reply to a permission request. Cancelled turns answer `cancelled` as ACP requires.
fn permission_reply(params: &Value, servers: &HashSet<String>, cancelled: bool) -> Value {
    if cancelled {
        return json!({"outcome": {"outcome": "cancelled"}});
    }
    let options = params["options"].as_array().cloned().unwrap_or_default();
    let pick = |prefix: &str| options.iter()
        .find(|o| o["kind"].as_str().is_some_and(|k| k == format!("{prefix}_once")))
        .and_then(|o| o["optionId"].as_str()).map(str::to_owned);
    let choice = if permission_allowed(&params["toolCall"], servers) { pick("allow") } else { pick("reject") };
    match choice {
        Some(id) => json!({"outcome": {"outcome": "selected", "optionId": id}}),
        // No usable option: never fall back to allowing.
        None => json!({"outcome": {"outcome": "cancelled"}}),
    }
}

/// A JSON-RPC result, or the error code and message.
type Reply = Result<Value, (i64, String)>;

struct Active {
    local: TurnId,
    sender: EventSender,
    cancelled: bool,
    started: HashSet<String>,
}

#[derive(Default)]
struct State {
    closed: bool,
    replies: HashMap<u64, mpsc::Sender<Reply>>,
    /// Pending `session/prompt` requests by id, mapped to their session.
    prompts: HashMap<u64, String>,
    active: HashMap<String, Active>,
    sessions: HashMap<String, Session>,
}

struct Session {
    servers: HashSet<String>,
    preamble: Option<String>,
    models: Vec<ModelOption>,
}

/// Models from the `session/new` reply, marking the one the session uses.
fn session_models(result: &Value) -> Vec<ModelOption> {
    let current = result["models"]["currentModelId"].as_str().unwrap_or("");
    result["models"]["availableModels"].as_array().into_iter().flatten().filter_map(|model| {
        let id = model["modelId"].as_str().filter(|id| !id.is_empty())?;
        Some(ModelOption {
            id: id.to_owned(),
            name: model["name"].as_str().unwrap_or(id).to_owned(),
            description: model["description"].as_str().map(str::to_owned),
            current: id == current,
        })
    }).collect()
}

/// One supervised `agy acp` process; sessions remain open across chat turns.
pub struct AntigravityAdapter {
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    state: Arc<Mutex<State>>,
    reader: Option<JoinHandle<()>>,
    next: Arc<AtomicU64>,
    timeout: Duration,
    workspace: PathBuf,
    /// Pending `authenticate` reply, if a Google sign-in was started.
    login: Mutex<Option<mpsc::Receiver<Reply>>>,
}

impl AntigravityAdapter {
    pub fn spawn(config: AntigravityConfig) -> Result<Self, BridgeError> {
        std::fs::create_dir_all(&config.home).map_err(|e| transport(e.to_string()))?;
        let home = std::fs::canonicalize(&config.home).map_err(|e| transport(e.to_string()))?;
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| transport(e.to_string()))?;
        let workspace = home.join("workspace");
        let settings = home.join("gemini");
        for dir in [&workspace, &settings] {
            std::fs::create_dir_all(dir).map_err(|e| transport(e.to_string()))?;
        }
        let binary = resolve_binary(&config.binary, std::env::var_os("PATH").as_deref())?;
        let mut command = Command::new(&binary);
        crate::process::isolate(&mut command);
        command.arg("acp").arg("--gemini-dir").arg(&settings).arg("--disable-telemetry");
        command.args(&config.extra_args);
        for name in CREDENTIAL_ENV {
            command.env_remove(name);
        }
        let mut child = command
            .current_dir(&workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| transport(format!("cannot launch Antigravity ACP server: {e}")))?;
        let stdin = Arc::new(Mutex::new(child.stdin.take().ok_or_else(|| transport("missing agy stdin"))?));
        let stdout = child.stdout.take().ok_or_else(|| transport("missing agy stdout"))?;
        let state = Arc::new(Mutex::new(State::default()));
        let reader_state = Arc::clone(&state);
        let reader_stdin = Arc::clone(&stdin);
        let reader = thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let Ok(value) = serde_json::from_str::<Value>(&line) else { continue };
                if let Some(method) = value["method"].as_str() {
                    if value.get("id").is_some_and(|id| !id.is_null()) {
                        let reply = request(&reader_state, method, &value["params"]);
                        let message = match reply {
                            Ok(result) => json!({"jsonrpc":"2.0","id":value["id"],"result":result}),
                            Err(()) => json!({"jsonrpc":"2.0","id":value["id"],
                                "error":{"code":-32601,"message":"unsupported by Tauros"}}),
                        };
                        let _ = write(&reader_stdin, message);
                    } else if method == "session/update" {
                        update(&reader_state, &value["params"]);
                    }
                } else if let Some(id) = value["id"].as_u64() {
                    response(&reader_state, id, &value);
                }
            }
            let mut state = reader_state.lock().unwrap();
            state.closed = true;
            for (_, reply) in state.replies.drain() {
                let _ = reply.send(Err((0, "Antigravity disconnected".into())));
            }
            for (_, active) in state.active.drain() {
                let _ = active.sender.send(AgentEvent::Error {
                    turn_id: Some(active.local.clone()),
                    code: ErrorCode::Transport,
                    message: "Antigravity disconnected; native edits retained in history".into(),
                });
                let _ = active.sender.send(AgentEvent::TurnCompleted {
                    turn_id: active.local,
                    finish_reason: FinishReason::Failed,
                    usage: None,
                });
            }
        });
        let adapter = Self {
            child,
            stdin,
            state,
            reader: Some(reader),
            next: Arc::new(AtomicU64::new(1)),
            timeout: config.timeout,
            workspace,
            login: Mutex::new(None),
        };
        // No filesystem or terminal capabilities: the agent cannot route writes
        // through the client, and its own are denied at the permission prompt.
        let init = adapter.rpc("initialize", json!({
            "protocolVersion": 1,
            "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false},
            "clientInfo": {"name": "tauros", "version": env!("CARGO_PKG_VERSION")}
        }))?;
        if init["protocolVersion"] != 1 {
            return Err(transport("unsupported ACP protocol version from agy; update Antigravity CLI"));
        }
        Ok(adapter)
    }

    fn send_request(&self, method: &str, params: Value) -> Result<(u64, mpsc::Receiver<Reply>), BridgeError> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = mpsc::channel();
        {
            let mut state = self.state.lock().unwrap();
            if state.closed {
                return Err(transport("Antigravity disconnected; start a new chat"));
            }
            state.replies.insert(id, sender);
        }
        if let Err(error) = write(&self.stdin, json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})) {
            self.state.lock().unwrap().replies.remove(&id);
            return Err(error);
        }
        Ok((id, receiver))
    }

    fn rpc(&self, method: &str, params: Value) -> Result<Value, BridgeError> {
        let (id, receiver) = self.send_request(method, params)?;
        match receiver.recv_timeout(self.timeout) {
            Ok(Ok(value)) => Ok(value),
            Ok(Err((AUTH_REQUIRED, _))) => Err(BridgeError::new(ErrorCode::AuthRequired,
                "Antigravity sign-in required: run `agy` once and log in with Google, or `agy acp --login`")),
            // Provider messages can carry account context; report only the method.
            Ok(Err(_)) => Err(BridgeError::new(ErrorCode::Provider,
                format!("Antigravity rejected {method}; check the CLI version and sign-in"))),
            Err(_) => {
                self.state.lock().unwrap().replies.remove(&id);
                Err(transport(format!("Antigravity {method} timed out")))
            }
        }
    }
}

impl AntigravityAdapter {
    /// Whether the runtime has a cached sign-in. Opens and closes an empty
    /// session, because ACP has no status query; no account data is returned.
    pub fn signed_in(&self) -> Result<bool, BridgeError> {
        match self.rpc("session/new", json!({"cwd": self.workspace, "mcpServers": []})) {
            Ok(result) => {
                if let Some(id) = result["sessionId"].as_str() {
                    let _ = self.rpc("session/close", json!({"sessionId": id}));
                }
                Ok(true)
            }
            Err(error) if error.code == ErrorCode::AuthRequired => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// Starts the documented `oauth-personal` method. The runtime opens the
    /// browser and hosts the callback itself; no URL or token reaches Tauros.
    /// Cancel by dropping the adapter, which terminates the runtime.
    pub fn start_google_login(&self) -> Result<(), BridgeError> {
        let mut login = self.login.lock().unwrap();
        if login.is_some() {
            return Err(BridgeError::new(ErrorCode::Busy, "Google sign-in is already pending"));
        }
        let (_, receiver) = self.send_request("authenticate", json!({"methodId": "oauth-personal"}))?;
        *login = Some(receiver);
        Ok(())
    }

    /// `None` while pending; the provider's reply (plan tier) is discarded.
    pub fn google_login_result(&self) -> Result<Option<bool>, BridgeError> {
        let mut login = self.login.lock().unwrap();
        let Some(receiver) = login.as_ref() else {
            return Err(BridgeError::new(ErrorCode::InvalidConfig, "no Google sign-in was started"));
        };
        let result = match receiver.try_recv() {
            Err(mpsc::TryRecvError::Empty) => return Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err(transport("Antigravity exited during sign-in")),
            Ok(Ok(_)) => Ok(Some(true)),
            Ok(Err((0, _))) => Err(transport("Antigravity exited during sign-in")),
            Ok(Err(_)) => Ok(Some(false)),
        };
        *login = None;
        result
    }
}

/// Requests from the agent. Everything except permission prompts is refused.
fn request(state: &Mutex<State>, method: &str, params: &Value) -> Result<Value, ()> {
    if method != "session/request_permission" {
        return Err(());
    }
    let state = state.lock().unwrap();
    let session = params["sessionId"].as_str().unwrap_or("");
    let servers = state.sessions.get(session).map(|s| s.servers.clone()).unwrap_or_default();
    // A prompt from a session without an active turn is treated as cancelled.
    let cancelled = state.active.get(session).is_none_or(|a| a.cancelled);
    Ok(permission_reply(params, &servers, cancelled))
}

fn display_name(name: &str, servers: &HashSet<String>) -> String {
    match split_mcp_name(name, servers) {
        Some((server, tool)) => format!("mcp__{server}__{tool}"),
        None => name.to_owned(),
    }
}

fn update(state: &Mutex<State>, params: &Value) {
    let mut state = state.lock().unwrap();
    let session = params["sessionId"].as_str().unwrap_or("").to_owned();
    let servers = state.sessions.get(&session).map(|s| s.servers.clone()).unwrap_or_default();
    let Some(active) = state.active.get_mut(&session) else { return };
    let turn = active.local.clone();
    let update = &params["update"];
    let text = || update["content"]["text"].as_str().unwrap_or("").to_owned();
    let event = match update["sessionUpdate"].as_str().unwrap_or("") {
        "agent_message_chunk" => Some(AgentEvent::TextDelta { turn_id: turn, delta: text() }),
        "agent_thought_chunk" => Some(AgentEvent::ThinkingDelta { turn_id: turn, delta: text() }),
        kind @ ("tool_call" | "tool_call_update") => {
            let id = update["toolCallId"].as_str().unwrap_or("unknown").to_owned();
            let name = display_name(update["name"].as_str().unwrap_or("tool"), &servers);
            let status = update["status"].as_str().unwrap_or("");
            if matches!(status, "completed" | "failed") {
                active.started.remove(&id).then(|| AgentEvent::ToolCallCompleted {
                    turn_id: turn,
                    call_id: ToolCallId::new(id),
                    name,
                    is_error: status == "failed",
                    output: json!({"content": update["content"], "rawOutput": update["rawOutput"]}),
                })
            } else if kind == "tool_call" || !active.started.contains(&id) {
                active.started.insert(id.clone()).then(|| AgentEvent::ToolCallStarted {
                    turn_id: turn,
                    call_id: ToolCallId::new(id),
                    name,
                    arguments: update["rawInput"].clone(),
                })
            } else {
                None
            }
        }
        _ => None,
    };
    if let Some(event) = event {
        let _ = active.sender.send(event);
    }
}

fn response(state: &Mutex<State>, id: u64, value: &Value) {
    let mut state = state.lock().unwrap();
    if let Some(session) = state.prompts.remove(&id) {
        let Some(active) = state.active.remove(&session) else { return };
        let turn = active.local;
        let error = value.get("error").filter(|e| !e.is_null());
        let reason = value["result"]["stopReason"].as_str().unwrap_or("");
        let finish_reason = if active.cancelled || reason == "cancelled" {
            FinishReason::Cancelled
        } else if error.is_some() || reason == "refusal" {
            FinishReason::Failed
        } else if matches!(reason, "max_tokens" | "max_turn_requests") {
            FinishReason::MaxTurns
        } else {
            FinishReason::Completed
        };
        if let Some(error) = error.filter(|_| finish_reason != FinishReason::Cancelled) {
            let (code, message) = if error["code"] == AUTH_REQUIRED {
                (ErrorCode::AuthRequired, "Antigravity sign-in required: run `agy` once and log in with Google")
            } else {
                (ErrorCode::Provider, "Antigravity turn failed")
            };
            let _ = active.sender.send(AgentEvent::Error { turn_id: Some(turn.clone()), code, message: message.into() });
        }
        let usage = &value["result"]["usage"];
        let usage = usage.is_object().then(|| TokenUsage {
            input_tokens: usage["inputTokens"].as_u64().unwrap_or(0),
            output_tokens: usage["outputTokens"].as_u64().unwrap_or(0),
        });
        let _ = active.sender.send(AgentEvent::TurnCompleted { turn_id: turn, finish_reason, usage });
        return;
    }
    if let Some(reply) = state.replies.remove(&id) {
        let result = match value.get("error").filter(|e| !e.is_null()) {
            Some(error) => Err((error["code"].as_i64().unwrap_or(0), error["message"].as_str().unwrap_or("").to_owned())),
            None => Ok(value["result"].clone()),
        };
        let _ = reply.send(result);
    }
}

impl AgentBridge for AntigravityAdapter {
    fn capabilities(&self) -> BridgeCapabilities {
        BridgeCapabilities {
            provider_name: "antigravity-acp".into(),
            supports_streaming: true,
            supports_tool_calls: true,
            supports_cancel: true,
            supported_auth: vec![AuthMode::HostSignIn],
        }
    }

    fn start_session(&mut self, config: SessionConfig) -> Result<SessionId, BridgeError> {
        // API key and cloud routes are deferred (AGT-009).
        if config.auth_mode != AuthMode::HostSignIn {
            return Err(BridgeError::new(ErrorCode::InvalidConfig, "Antigravity requires host_sign_in"));
        }
        let mut servers = HashSet::new();
        let mut mcp = Vec::new();
        for (name, server) in config.mcp_servers {
            if !valid_server_name(&name) {
                return Err(BridgeError::new(ErrorCode::InvalidConfig,
                    "Antigravity MCP server names must use lowercase letters, digits and '-'"));
            }
            match server {
                McpServerConfig::Stdio { command, args, env } => mcp.push(json!({
                    "name": name, "command": command, "args": args,
                    "env": env.into_iter().map(|(name, value)| json!({"name": name, "value": value})).collect::<Vec<_>>()
                })),
                McpServerConfig::StreamableHttp { .. } => return Err(BridgeError::new(ErrorCode::InvalidConfig,
                    "Antigravity sessions accept only stdio MCP servers")),
            }
            servers.insert(name);
        }
        let result = self.rpc("session/new", json!({"cwd": self.workspace, "mcpServers": mcp}))?;
        let id = result["sessionId"].as_str().filter(|id| !id.is_empty())
            .ok_or_else(|| transport("missing Antigravity session id"))?.to_owned();
        let session = SessionId::new(id.clone());
        // Never inherit an auto-approving mode; prompts are what Tauros intercepts.
        self.rpc("session/set_mode", json!({"sessionId": id, "modeId": "default"}))?;
        if let Some(model) = &config.model {
            self.rpc("session/set_model", json!({"sessionId": id, "modelId": model}))?;
        }
        let models = session_models(&result);
        self.state.lock().unwrap().sessions.insert(id, Session { servers, preamble: config.system_prompt, models });
        Ok(session)
    }

    fn send_message(&mut self, session: &SessionId, message: UserMessage) -> Result<TurnStream, BridgeError> {
        let turn = TurnId::new(format!("antigravity-turn-{}", self.next.fetch_add(1, Ordering::Relaxed)));
        let (sender, stream) = event_channel();
        let preamble = {
            let mut state = self.state.lock().unwrap();
            if state.closed {
                return Err(transport("Antigravity disconnected; start a new chat"));
            }
            if state.active.contains_key(session.as_str()) {
                return Err(BridgeError::new(ErrorCode::Busy, "Antigravity is still answering"));
            }
            let entry = state.sessions.get_mut(session.as_str())
                .ok_or_else(|| BridgeError::new(ErrorCode::SessionNotFound, "unknown Antigravity session"))?;
            let preamble = entry.preamble.take();
            state.active.insert(session.to_string(), Active {
                local: turn.clone(), sender: sender.clone(), cancelled: false, started: HashSet::new(),
            });
            preamble
        };
        let _ = sender.send(AgentEvent::TurnStarted { turn_id: turn.clone() });
        // ACP has no portable system prompt; the instructions lead the first turn.
        let mut text = match preamble {
            Some(preamble) => format!("<tauros-instructions>\n{preamble}\n</tauros-instructions>\n\n{}", message.text),
            None => message.text,
        };
        if !message.attachments.is_empty() {
            text.push_str(&format!("\nContext: {}", serde_json::to_string(&message.attachments).unwrap()));
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        {
            let mut state = self.state.lock().unwrap();
            state.prompts.insert(id, session.to_string());
        }
        let sent = write(&self.stdin, json!({"jsonrpc":"2.0","id":id,"method":"session/prompt",
            "params": {"sessionId": session, "prompt": [{"type": "text", "text": text}]}}));
        if let Err(error) = sent {
            let mut state = self.state.lock().unwrap();
            state.prompts.remove(&id);
            state.active.remove(session.as_str());
            return Err(error);
        }
        Ok(TurnStream::new(turn, stream))
    }

    fn cancel(&mut self, session: &SessionId, turn: &TurnId) -> Result<(), BridgeError> {
        {
            let mut state = self.state.lock().unwrap();
            let active = state.active.get_mut(session.as_str()).filter(|a| &a.local == turn)
                .ok_or_else(|| BridgeError::new(ErrorCode::TurnNotFound, "unknown Antigravity turn"))?;
            active.cancelled = true;
        }
        // The prompt response then ends the turn with stopReason `cancelled`.
        write(&self.stdin, json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId": session}}))
    }

    fn list_models(&mut self, session: &SessionId) -> Result<Vec<ModelOption>, BridgeError> {
        let state = self.state.lock().unwrap();
        let session = state.sessions.get(session.as_str())
            .ok_or_else(|| BridgeError::new(ErrorCode::SessionNotFound, "unknown Antigravity session"))?;
        Ok(session.models.clone())
    }

    fn set_model(&mut self, session: &SessionId, model: &str) -> Result<(), BridgeError> {
        {
            let state = self.state.lock().unwrap();
            let entry = state.sessions.get(session.as_str())
                .ok_or_else(|| BridgeError::new(ErrorCode::SessionNotFound, "unknown Antigravity session"))?;
            if state.active.contains_key(session.as_str()) {
                return Err(BridgeError::new(ErrorCode::Busy, "Antigravity is still answering"));
            }
            if !entry.models.iter().any(|m| m.id == model) {
                return Err(BridgeError::new(ErrorCode::InvalidConfig, "model not offered by Antigravity"));
            }
        }
        self.rpc("session/set_model", json!({"sessionId": session, "modelId": model}))?;
        let mut state = self.state.lock().unwrap();
        if let Some(entry) = state.sessions.get_mut(session.as_str()) {
            for option in &mut entry.models {
                option.current = option.id == model;
            }
        }
        Ok(())
    }

    fn close_session(&mut self, session: &SessionId) -> Result<(), BridgeError> {
        let turn = self.state.lock().unwrap().active.get(session.as_str()).map(|a| a.local.clone());
        if let Some(turn) = turn {
            self.cancel(session, &turn)?;
        }
        // `session/close` is advertised by agy; failure leaves nothing to release.
        let _ = self.rpc("session/close", json!({"sessionId": session}));
        self.state.lock().unwrap().sessions.remove(session.as_str());
        Ok(())
    }
}

impl Drop for AntigravityAdapter {
    fn drop(&mut self) {
        crate::process::terminate(&mut self.child);
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn servers() -> HashSet<String> {
        ["tauros".to_owned(), "project".to_owned()].into()
    }

    fn prompt(name: &str) -> Value {
        json!({"toolCall": {"name": name}, "options": [
            {"optionId": "allow", "kind": "allow_once"}, {"optionId": "always", "kind": "allow_always"},
            {"optionId": "deny", "kind": "reject_once"}]})
    }

    #[test]
    fn only_attached_mcp_tools_are_allowed() {
        let allowed = json!({"outcome": {"outcome": "selected", "optionId": "allow"}});
        let denied = json!({"outcome": {"outcome": "selected", "optionId": "deny"}});
        assert_eq!(permission_reply(&prompt("mcp_tauros_build"), &servers(), false), allowed);
        assert_eq!(permission_reply(&prompt("mcp_project_tauros_write_file"), &servers(), false), allowed);
        for name in ["write_to_file", "run_command", "read_file", "mcp_other_tool", "mcp_tauros_", "mcp_taurosx_build", ""] {
            assert_eq!(permission_reply(&prompt(name), &servers(), false), denied, "{name}");
        }
        // Cancelled turns and requests without a reject option never allow.
        let cancelled = json!({"outcome": {"outcome": "cancelled"}});
        assert_eq!(permission_reply(&prompt("mcp_tauros_build"), &servers(), true), cancelled);
        let no_reject = json!({"toolCall": {"name": "run_command"}, "options": [{"optionId": "a", "kind": "allow_once"}]});
        assert_eq!(permission_reply(&no_reject, &servers(), false), cancelled);
    }

    #[test]
    fn server_names_cannot_overlap_prefixes() {
        assert!(valid_server_name("tauros") && valid_server_name("project-2"));
        assert!(!valid_server_name("tauros_native") && !valid_server_name("") && !valid_server_name("Tauros"));
        assert_eq!(display_name("mcp_tauros_create_trainer", &servers()), "mcp__tauros__create_trainer");
        assert_eq!(display_name("run_command", &servers()), "run_command");
    }
}
