//! AGT-003: the app's chat conversation streams agent events to the window sink,
//! refuses overlapping turns, cancels and resets; with the real sidecar and the
//! in-process MCP server a domain tool call reaches the connector and streams back.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;
use tauros_agent_bridge::claude_sidecar::ClaudeSidecarConfig;
use tauros_agent_bridge::fake::{FakeAdapter, FakeStep, FakeTurnScript};
use tauros_agent_bridge::{AgentBridge, AgentEvent, FinishReason, SessionConfig, ToolCallId};
use tauros_app::chat::{claude_backend, Chat, ChatBackend};
use tauros_mcp::TaurosServer;
use tauros_model::memory::MemoryConnector;
use tauros_model::{Capabilities, Connector, GameFamily, GameInfo, ProjectSnapshot, Species, SpeciesId, TrainerId};

const WAIT: Duration = Duration::from_secs(10);

fn fake_chat(scripts: Vec<FakeTurnScript>) -> (Chat, Arc<Mutex<u32>>) {
    let scripts = Arc::new(Mutex::new(scripts));
    let sessions = Arc::new(Mutex::new(0));
    let started = Arc::clone(&sessions);
    let chat = Chat::new(move || {
        let mut bridge = FakeAdapter::new();
        for script in scripts.lock().unwrap().drain(..) {
            bridge.push_script(script);
        }
        let session = bridge.start_session(SessionConfig::default()).map_err(|e| e.message)?;
        *started.lock().unwrap() += 1;
        Ok(ChatBackend { bridge: Box::new(bridge), session, keep_alive: Box::new(()) })
    });
    (chat, sessions)
}

fn event_sink() -> (impl FnMut(AgentEvent) + Send + 'static, Receiver<AgentEvent>) {
    let (tx, rx) = mpsc::channel();
    (move |event| tx.send(event).unwrap(), rx)
}

/// Collects events up to and including the turn's `TurnCompleted`.
fn until_completed(rx: &Receiver<AgentEvent>) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    loop {
        let event = rx.recv_timeout(WAIT).expect("the turn should keep streaming until it completes");
        let done = matches!(event, AgentEvent::TurnCompleted { .. });
        events.push(event);
        if done {
            return events;
        }
    }
}

fn wait_idle(chat: &Chat) {
    let start = std::time::Instant::now();
    while chat.busy() {
        assert!(start.elapsed() < WAIT, "the chat should become idle after the turn");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn turn_streams_text_and_tool_calls_in_order() {
    let script = FakeTurnScript {
        steps: vec![
            FakeStep::Text("Creating ".into()),
            FakeStep::ToolCall {
                call_id: ToolCallId::new("call-1"),
                name: "mcp__tauros__create_trainer".into(),
                arguments: json!({ "id": "TRAINER_X" }),
                is_error: false,
                output: json!({ "ok": true }),
            },
            FakeStep::Text("done.".into()),
        ],
        usage: None,
    };
    let (chat, sessions) = fake_chat(vec![script]);
    assert!(chat.send("   ", |_| {}).is_err(), "an empty message is refused");
    assert_eq!(*sessions.lock().unwrap(), 0, "an empty message does not start the agent");

    let (sink, rx) = event_sink();
    let turn = chat.send("make a trainer", sink).unwrap();
    let events = until_completed(&rx);
    let kinds: Vec<_> = events
        .iter()
        .map(|e| match e {
            AgentEvent::TurnStarted { .. } => "started".to_owned(),
            AgentEvent::TextDelta { delta, .. } => format!("text:{delta}"),
            AgentEvent::ToolCallStarted { name, .. } => format!("tool:{name}"),
            AgentEvent::ToolCallCompleted { is_error, .. } => format!("tool-done:{is_error}"),
            AgentEvent::TurnCompleted { finish_reason, .. } => format!("end:{finish_reason:?}"),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(
        kinds,
        ["started", "text:Creating ", "tool:mcp__tauros__create_trainer", "tool-done:false", "text:done.", "end:Completed"]
    );
    assert!(matches!(&events[0], AgentEvent::TurnStarted { turn_id } if *turn_id == turn));
    wait_idle(&chat);
    assert_eq!(*sessions.lock().unwrap(), 1);
}

#[test]
fn overlapping_turns_are_refused_and_cancel_ends_the_turn() {
    let gated = FakeTurnScript { steps: vec![FakeStep::Text("thinking".into()), FakeStep::WaitForGate], usage: None };
    let (chat, _) = fake_chat(vec![gated, FakeTurnScript::text_chunks(["second"])]);
    let (sink, rx) = event_sink();
    chat.send("first", sink).unwrap();
    assert!(matches!(rx.recv_timeout(WAIT).unwrap(), AgentEvent::TurnStarted { .. }));
    assert!(chat.busy());
    let refused = chat.send("too soon", |_| {}).unwrap_err();
    assert!(refused.contains("still answering"), "{refused}");

    chat.cancel().unwrap();
    let rest = until_completed(&rx);
    assert!(matches!(rest.last(), Some(AgentEvent::TurnCompleted { finish_reason: FinishReason::Cancelled, .. })));
    wait_idle(&chat);
    chat.cancel().expect("cancel with no turn running is a no-op");

    let (sink, rx) = event_sink();
    chat.send("again", sink).unwrap();
    assert!(until_completed(&rx).iter().any(|e| matches!(e, AgentEvent::TextDelta { delta, .. } if delta == "second")));
}

#[test]
fn reset_starts_a_new_session_on_the_next_message() {
    let (chat, sessions) = fake_chat(vec![FakeTurnScript::text_chunks(["a"])]);
    let (sink, rx) = event_sink();
    chat.send("one", sink).unwrap();
    until_completed(&rx);
    wait_idle(&chat);
    chat.reset();
    assert!(!chat.busy());
    let (sink, rx) = event_sink();
    chat.send("two", sink).unwrap();
    until_completed(&rx);
    assert_eq!(*sessions.lock().unwrap(), 2);
}

#[test]
fn factory_errors_reach_the_caller_and_are_retried() {
    let attempts = Arc::new(Mutex::new(0));
    let counted = Arc::clone(&attempts);
    let chat = Chat::new(move || {
        *counted.lock().unwrap() += 1;
        Err("sidecar entry script not found".to_owned())
    });
    assert_eq!(chat.send("hello", |_| {}).unwrap_err(), "sidecar entry script not found");
    assert!(chat.send("hello", |_| {}).is_err());
    assert_eq!(*attempts.lock().unwrap(), 2);
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn sample_server() -> (TaurosServer, Arc<Mutex<Box<dyn Connector + Send>>>) {
    let mut snapshot = ProjectSnapshot {
        game: GameInfo {
            family: GameFamily::Gen3,
            engine: "pokeemerald-expansion".to_owned(),
            capabilities: Capabilities { max_party_size: 6, max_moves: 4, min_level: 1, max_level: 100, held_items: true, abilities: true },
        },
        trainers: BTreeMap::new(),
        maps: BTreeMap::new(),
        scenes: BTreeMap::new(),
        species: BTreeMap::new(),
        flags: BTreeMap::new(),
    };
    let id = SpeciesId::from("SPECIES_POOCHYENA");
    snapshot.species.insert(id.clone(), Species { id, name: "Poochyena".to_owned(), national_dex: Some(261) });
    let connector: Arc<Mutex<Box<dyn Connector + Send>>> = Arc::new(Mutex::new(Box::new(MemoryConnector::new(snapshot))));
    (TaurosServer::with_shared_connector(Arc::clone(&connector)), connector)
}

#[test]
fn claude_backend_runs_a_domain_tool_through_the_sidecar() {
    let sidecar = ClaudeSidecarConfig::from_repo_root(&repo_root());
    assert!(sidecar.entry_script.is_file(), "run `npm --prefix sidecar/claude-agent-sdk run build` first");
    let (server, connector) = sample_server();
    let server = Mutex::new(Some(server));
    let chat = Chat::new(move || {
        let extra = BTreeMap::from([("test_driver".to_owned(), json!("direct_mcp"))]);
        claude_backend(sidecar.clone(), server.lock().unwrap().take(), repo_root(), extra)
    });
    let directive = json!({
        "tool": "create_trainer",
        "arguments": {
            "id": "TRAINER_CHAT_PANEL", "name": "CHAT", "class": "TRAINER_CLASS_LASS",
            "party": [{ "species": "SPECIES_POOCHYENA", "level": 5 }]
        }
    });
    let (sink, rx) = event_sink();
    chat.send(&directive.to_string(), sink).unwrap();
    let events = until_completed(&rx);
    assert!(events.iter().any(|e| matches!(e, AgentEvent::ToolCallStarted { name, .. } if name == "create_trainer")));
    assert!(events.iter().any(|e| matches!(e, AgentEvent::ToolCallCompleted { name, is_error: false, .. } if name == "create_trainer")));
    assert!(matches!(events.last(), Some(AgentEvent::TurnCompleted { finish_reason: FinishReason::Completed, .. })));
    let snapshot = connector.lock().unwrap().load().unwrap();
    assert!(snapshot.trainers.contains_key(&TrainerId::from("TRAINER_CHAT_PANEL")));
}

/// Two live turns in one conversation: the second must see the first.
#[test]
#[ignore = "requires an active host Claude Code sign-in and TAUROS_LIVE_CLAUDE=1"]
fn live_chat_remembers_the_previous_turn() {
    assert_eq!(std::env::var("TAUROS_LIVE_CLAUDE").as_deref(), Ok("1"));
    let sidecar = ClaudeSidecarConfig::from_repo_root(&repo_root());
    let chat = Chat::new(move || claude_backend(sidecar.clone(), None, repo_root(), BTreeMap::new()));
    let text = |events: &[AgentEvent]| {
        events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::TextDelta { delta, .. } => Some(delta.as_str()),
                _ => None,
            })
            .collect::<String>()
    };
    let (sink, rx) = event_sink();
    chat.send("Remember the code word PERSIMMON-42. Reply only with OK.", sink).unwrap();
    let first = until_completed(&rx);
    assert!(matches!(first.last(), Some(AgentEvent::TurnCompleted { finish_reason: FinishReason::Completed, .. })), "{first:#?}");
    wait_idle(&chat);
    let (sink, rx) = event_sink();
    chat.send("What was the code word? Reply with the code word only.", sink).unwrap();
    let second = until_completed(&rx);
    assert!(text(&second).contains("PERSIMMON-42"), "{second:#?}");
}
