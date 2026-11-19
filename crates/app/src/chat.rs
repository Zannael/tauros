//! The app's chat conversation (`AGT-003`): one agent session behind an
//! [`AgentBridge`], started on the first message, whose turns are forwarded event by
//! event to a sink (the window's chat channel) so text and tool calls stream live.
//!
//! The bridge is created by a factory so the app can launch the Claude sidecar
//! wired to the in-process MCP server while tests use the fake adapter.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use tauros_agent_bridge::claude_sidecar::{ClaudeSidecarAdapter, ClaudeSidecarConfig};
use tauros_agent_bridge::{AgentBridge, AgentEvent, AuthMode, SessionConfig, SessionId, TurnId, UserMessage};
use tauros_mcp::TaurosServer;

use crate::mcp_host::InProcessMcpHost;

const SYSTEM_PROMPT: &str = "You are the assistant inside Tauros, a Pokémon ROM hack editor. \
Change the game project only through the Tauros MCP tools (trainers, maps, scenes, flags, \
builds, emulator); every change is snapshotted and can be undone from the app's history. \
Keep answers short and say which tools you used.";

/// A started agent session plus whatever must live as long as it (such as the
/// in-process MCP host the session's tools connect to).
pub struct ChatBackend {
    pub bridge: Box<dyn AgentBridge>,
    pub session: SessionId,
    pub keep_alive: Box<dyn Send>,
}

type Factory = Box<dyn Fn() -> Result<ChatBackend, String> + Send + Sync>;

struct Live {
    backend: ChatBackend,
    /// The turn whose events are still being forwarded, if any.
    active: Arc<Mutex<Option<TurnId>>>,
}

/// One conversation with the agent; every method may be called from any thread.
pub struct Chat {
    factory: Factory,
    live: Mutex<Option<Live>>,
}

impl Chat {
    pub fn new(factory: impl Fn() -> Result<ChatBackend, String> + Send + Sync + 'static) -> Self {
        Self { factory: Box::new(factory), live: Mutex::new(None) }
    }

    /// Starts a turn with `text` and forwards its events to `sink` on a separate
    /// thread until the turn ends. Starts the session first if there is none.
    /// Refuses while another turn is still streaming.
    pub fn send(&self, text: &str, sink: impl FnMut(AgentEvent) + Send + 'static) -> Result<TurnId, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("the message is empty".to_owned());
        }
        let mut guard = self.live.lock().unwrap();
        if guard.is_none() {
            let backend = (self.factory)()?;
            *guard = Some(Live { backend, active: Arc::new(Mutex::new(None)) });
        }
        let live = guard.as_mut().unwrap();
        if live.active.lock().unwrap().is_some() {
            return Err("the agent is still answering; stop it or wait".to_owned());
        }
        let turn = live
            .backend
            .bridge
            .send_message(&live.backend.session, UserMessage::text(text))
            .map_err(|e| e.message)?;
        let (turn_id, events) = turn.into_parts();
        *live.active.lock().unwrap() = Some(turn_id.clone());
        let active = Arc::clone(&live.active);
        let mut sink = sink;
        thread::Builder::new()
            .name("chat-turn".into())
            .spawn(move || {
                for event in events {
                    let terminal = matches!(event, AgentEvent::TurnCompleted { .. });
                    sink(event);
                    if terminal {
                        break;
                    }
                }
                *active.lock().unwrap() = None;
            })
            .map_err(|e| format!("cannot start the chat thread: {e}"))?;
        Ok(turn_id)
    }

    /// Asks the agent to stop the turn in progress; its stream ends with a
    /// cancelled `TurnCompleted`. Does nothing when no turn is running.
    pub fn cancel(&self) -> Result<(), String> {
        let mut guard = self.live.lock().unwrap();
        let Some(live) = guard.as_mut() else { return Ok(()) };
        let Some(turn) = live.active.lock().unwrap().clone() else { return Ok(()) };
        live.backend.bridge.cancel(&live.backend.session, &turn).map_err(|e| e.message)
    }

    /// Ends the conversation; the next message starts a new session.
    pub fn reset(&self) {
        if let Some(mut live) = self.live.lock().unwrap().take() {
            if let Some(turn) = live.active.lock().unwrap().clone() {
                let _ = live.backend.bridge.cancel(&live.backend.session, &turn);
            }
            let _ = live.backend.bridge.close_session(&live.backend.session);
        }
    }

    /// Whether a turn is still streaming.
    pub fn busy(&self) -> bool {
        self.live.lock().unwrap().as_ref().is_some_and(|live| live.active.lock().unwrap().is_some())
    }
}

impl Drop for Chat {
    fn drop(&mut self) {
        self.reset();
    }
}

/// Starts the Claude Agent SDK sidecar (host sign-in, `RES-002`) with one session
/// whose `tauros` MCP server is `server`, hosted in this process. Without a server
/// the agent has no Tauros tools. `extra` passes adapter options such as the
/// sidecar's deterministic test driver.
pub fn claude_backend(
    sidecar: ClaudeSidecarConfig,
    server: Option<TaurosServer>,
    working_dir: PathBuf,
    extra: BTreeMap<String, serde_json::Value>,
) -> Result<ChatBackend, String> {
    let host = server.map(InProcessMcpHost::start).transpose()?;
    let mut bridge = ClaudeSidecarAdapter::spawn(sidecar.clone()).map_err(|e| e.message)?;
    let mcp_servers = host
        .iter()
        .map(|host| ("tauros".to_owned(), host.mcp_server_config(&sidecar.node_binary, &sidecar.mcp_bridge_script())))
        .collect();
    let session = bridge
        .start_session(SessionConfig {
            working_dir: Some(working_dir),
            system_prompt: Some(SYSTEM_PROMPT.to_owned()),
            mcp_servers,
            auth_mode: AuthMode::HostSignIn,
            extra,
            ..SessionConfig::default()
        })
        .map_err(|e| e.message)?;
    Ok(ChatBackend { bridge: Box::new(bridge), session, keep_alive: Box::new(host) })
}

/// Short name of a tool as the agent runtime reports it (`mcp__tauros__build` → `build`).
pub fn tool_display_name(name: &str) -> &str {
    name.strip_prefix("mcp__").and_then(|rest| rest.split_once("__")).map_or(name, |(_, tool)| tool)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_names_lose_the_mcp_prefix() {
        assert_eq!(tool_display_name("mcp__tauros__create_trainer"), "create_trainer");
        assert_eq!(tool_display_name("create_trainer"), "create_trainer");
        assert_eq!(tool_display_name("mcp__odd"), "mcp__odd");
    }
}
