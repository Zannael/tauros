//! AGT-007: Antigravity over ACP with the real host MCP and native transaction paths.
#[path = "../../connector-gen3/tests/common/mod.rs"]
mod common;
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauros_agent_bridge::antigravity::{AntigravityAdapter, AntigravityConfig};
use tauros_agent_bridge::{AgentBridge, AgentEvent, AuthMode, ErrorCode, FinishReason, SessionConfig, UserMessage};
use tauros_app::chat::{antigravity_backend, ChatBackend};
use tauros_app::history::{list_history, undo_to, ChangeOrigin};
use tauros_mcp::TaurosServer;
use tauros_model::memory::MemoryConnector;
use tauros_model::{Capabilities, Connector, GameFamily, GameInfo, ProjectSnapshot, Species, SpeciesId, TrainerId};

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git").current_dir(root).args(args).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().into()
}

fn fixture() -> common::TempDir {
    let dir = common::TempDir::new("agy-game");
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["config", "user.name", "Tauros Test"]);
    git(dir.path(), &["config", "user.email", "test@users.noreply.github.com"]);
    fs::write(dir.path().join("scene.c"), "original\n").unwrap();
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-qm", "initial"]);
    dir
}

fn connector() -> MemoryConnector {
    let species = BTreeMap::from([(SpeciesId::from("SPECIES_TEST"),
        Species { id: SpeciesId::from("SPECIES_TEST"), name: "Test".into(), national_dex: None })]);
    MemoryConnector::new(ProjectSnapshot {
        game: GameInfo {
            family: GameFamily::Gen3,
            engine: "original-synthetic-fixture".into(),
            capabilities: Capabilities { max_party_size: 6, max_moves: 4, min_level: 1, max_level: 100, held_items: true, abilities: true },
        },
        trainers: BTreeMap::new(),
        maps: BTreeMap::new(),
        scenes: BTreeMap::new(),
        species,
        flags: BTreeMap::new(),
    })
}

fn config(home: &Path) -> AntigravityConfig {
    let mut config = AntigravityConfig::new(home);
    config.binary = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/agy_acp_server.py");
    config
}

fn events(backend: &mut ChatBackend, prompt: &str) -> Vec<AgentEvent> {
    let turn = backend.bridge.send_message(&backend.session, UserMessage::text(prompt)).unwrap();
    collect(turn.events())
}

fn collect(stream: &tauros_agent_bridge::EventStream) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    loop {
        let event = stream.recv_timeout(Duration::from_secs(60)).expect("Antigravity turn timed out");
        let terminal = matches!(event, AgentEvent::TurnCompleted { .. });
        events.push(event);
        if terminal {
            return events;
        }
    }
}

fn completed(events: &[AgentEvent]) {
    assert!(!events.iter().any(|event| matches!(event, AgentEvent::Error { .. })), "{events:#?}");
    assert!(matches!(events.last(), Some(AgentEvent::TurnCompleted { finish_reason: FinishReason::Completed, .. })), "{events:#?}");
}

fn succeeded(events: &[AgentEvent], name: &str) -> bool {
    events.iter().any(|e| matches!(e, AgentEvent::ToolCallCompleted { name: n, is_error: false, .. } if n == name))
}

#[test]
fn mixed_native_edit_real_mcp_history_undo_continuity_and_snapshot_rejection() {
    let dir = fixture();
    let home = common::TempDir::new("agy-home");
    let snapshot = git(dir.path(), &["rev-parse", "HEAD"]);
    let connector = Arc::new(Mutex::new(Box::new(connector()) as Box<dyn Connector + Send>));
    let server = TaurosServer::with_shared_connector(Arc::clone(&connector)).with_operation_gate(Arc::new(Mutex::new(())));
    let mut backend = antigravity_backend(config(home.path()), Some(server), dir.path().into(), None).unwrap();
    let result = events(&mut backend, &format!("edit:{}", dir.path().display()));
    completed(&result);
    for name in ["mcp__project__tauros_read_file", "mcp__project__tauros_write_file", "mcp__tauros__create_trainer"] {
        assert!(succeeded(&result, name), "{name}: {result:#?}");
    }
    // Denied built-in calls stream as failed tool calls.
    for name in ["write_to_file", "run_command", "read_file", "mcp_other_tool"] {
        assert!(result.iter().any(|e| matches!(e, AgentEvent::ToolCallCompleted { name: n, is_error: true, .. } if n == name)), "{name}");
    }
    assert!(result.iter().any(|e| matches!(e, AgentEvent::TextDelta { delta, .. } if delta == "turn 1")));
    assert!(connector.lock().unwrap().load().unwrap().trainers.contains_key(&TrainerId::from("TRAINER_AGY_FIXTURE")));
    assert_eq!(fs::read_to_string(dir.path().join("scene.c")).unwrap(), "edited by Antigravity fixture\n");
    assert!(!dir.path().join("../escape.c").exists());
    let history = list_history(dir.path(), 5).unwrap();
    assert_eq!(history[0].origin, ChangeOrigin::AgentGenerated);
    assert!(history.iter().any(|entry| entry.summary.contains("native Write")), "{history:#?}");
    completed(&events(&mut backend, "continue"));
    undo_to(dir.path(), &snapshot).unwrap();
    assert_eq!(fs::read_to_string(dir.path().join("scene.c")).unwrap(), "original\n");
    // A dirty human edit forces a snapshot commit; a failing hook must reject
    // the write before the agent gets a successful tool result.
    fs::write(dir.path().join("scene.c"), "pending human change\n").unwrap();
    let hook = dir.path().join(".git/hooks/pre-commit");
    fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    completed(&events(&mut backend, &format!("reject:{}", dir.path().display())));
    assert_eq!(fs::read_to_string(dir.path().join("scene.c")).unwrap(), "pending human change\n");
    backend.bridge.close_session(&backend.session).unwrap();
}

#[test]
fn cancellation_disconnect_auth_and_api_refusal_are_terminal() {
    let home = common::TempDir::new("agy-home-cancel");
    let dir = fixture();
    let server = TaurosServer::default().with_operation_gate(Arc::new(Mutex::new(())));
    let mut backend = antigravity_backend(config(home.path()), Some(server), dir.path().into(), None).unwrap();
    let turn = backend.bridge.send_message(&backend.session, UserMessage::text("wait")).unwrap();
    assert!(matches!(turn.events().recv_timeout(Duration::from_secs(5)).unwrap(), AgentEvent::TurnStarted { .. }));
    backend.bridge.cancel(&backend.session, turn.turn_id()).unwrap();
    let events = collect(turn.events());
    assert!(matches!(events.last(), Some(AgentEvent::TurnCompleted { finish_reason: FinishReason::Cancelled, .. })), "{events:#?}");
    assert!(!events.iter().any(|e| matches!(e, AgentEvent::Error { .. })), "{events:#?}");
    let events = events_after(&mut backend, "crash");
    assert!(matches!(events.last(), Some(AgentEvent::TurnCompleted { finish_reason: FinishReason::Failed, .. })), "{events:#?}");
    let error = backend.bridge.send_message(&backend.session, UserMessage::text("after crash")).unwrap_err();
    assert_eq!(error.code, ErrorCode::Transport);

    let mut adapter = AntigravityAdapter::spawn(config(home.path())).unwrap();
    for mode in [AuthMode::ApiKey, AuthMode::CloudCredentials, AuthMode::None] {
        let error = adapter.start_session(SessionConfig { auth_mode: mode, ..SessionConfig::default() }).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidConfig);
    }
    let mut missing = config(home.path());
    missing.extra_args.push("--fixture-auth-required".into());
    let error = AntigravityAdapter::spawn(missing).unwrap()
        .start_session(SessionConfig { auth_mode: AuthMode::HostSignIn, ..SessionConfig::default() }).unwrap_err();
    assert_eq!(error.code, ErrorCode::AuthRequired);
    assert!(!error.message.contains("Authentication required"), "provider text must not be forwarded");
    // The runtime home may not contain or be inside the game project.
    let inside = antigravity_backend(config(&dir.path().join("agy")), None, dir.path().into(), None).err().unwrap();
    assert!(inside.contains("separate directories"), "{inside}");
    assert!(!dir.path().join("agy").exists());
}

fn events_after(backend: &mut ChatBackend, prompt: &str) -> Vec<AgentEvent> {
    events(backend, prompt)
}

#[test]
#[ignore = "requires TAUROS_LIVE_AGY=1 and Antigravity CLI signed in with a Google subscription; original synthetic data only"]
fn live_antigravity_mixed_turn_on_original_synthetic_data() {
    assert_eq!(std::env::var("TAUROS_LIVE_AGY").as_deref(), Ok("1"));
    let dir = fixture();
    let home = common::TempDir::new("agy-live-home");
    let baseline = git(dir.path(), &["rev-parse", "HEAD"]);
    let connector = Arc::new(Mutex::new(Box::new(connector()) as Box<dyn Connector + Send>));
    let server = TaurosServer::with_shared_connector(Arc::clone(&connector)).with_operation_gate(Arc::new(Mutex::new(())));
    let mut config = AntigravityConfig::new(home.path());
    if let Some(binary) = std::env::var_os("TAUROS_AGY_BIN") {
        config.binary = binary.into();
    }
    let mut backend = antigravity_backend(config, Some(server), dir.path().into(), std::env::var("TAUROS_AGY_MODEL").ok()).unwrap();
    let events = events(&mut backend, "This project is original synthetic data. Use the project MCP tool tauros_read_file to read scene.c, \
then tauros_write_file to replace its contents with exactly 'synthetic\\n', then call the Tauros MCP tool create_trainer with id \
TRAINER_AGY_LIVE, name Gemini, class Youngster and party [{species: SPECIES_TEST, level: 7}]. Do not use any other tool.");
    completed(&events);
    for name in ["mcp__project__tauros_read_file", "mcp__project__tauros_write_file", "mcp__tauros__create_trainer"] {
        assert!(succeeded(&events, name), "{name}: {events:#?}");
    }
    assert!(events.iter().any(|e| matches!(e, AgentEvent::TextDelta { .. })), "no streamed text");
    assert_eq!(fs::read_to_string(dir.path().join("scene.c")).unwrap(), "synthetic\n");
    assert!(connector.lock().unwrap().load().unwrap().trainers.contains_key(&TrainerId::from("TRAINER_AGY_LIVE")));
    assert_eq!(list_history(dir.path(), 1).unwrap()[0].origin, ChangeOrigin::AgentGenerated);
    undo_to(dir.path(), &baseline).unwrap();
    assert_eq!(fs::read_to_string(dir.path().join("scene.c")).unwrap(), "original\n");
}

#[test]
fn app_shutdown_closes_active_antigravity_session_with_chat_retained() {
    let home = common::TempDir::new("agy-shutdown");
    let dir = fixture();
    let backend = antigravity_backend(config(home.path()), None, dir.path().into(), None).unwrap();
    let slot = Mutex::new(Some(backend));
    let chat = Arc::new(tauros_app::chat::Chat::new(move || Ok(slot.lock().unwrap().take().unwrap())));
    let retained = chat.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    chat.send("wait", move |event| { let _ = tx.send(event); }).unwrap();
    assert!(matches!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), AgentEvent::TurnStarted { .. }));
    chat.shutdown();
    chat.shutdown();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while chat.busy() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(retained.send("restart", |_| {}).unwrap_err().contains("shutting down"));
}
