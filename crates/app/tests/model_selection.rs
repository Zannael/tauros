//! AGT-011: model listing and selection through each adapter and the chat,
//! against deterministic peers that report the model a turn actually uses.
#[path = "../../connector-gen3/tests/common/mod.rs"]
mod common;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use serde_json::json;
use tauros_agent_bridge::antigravity::AntigravityConfig;
use tauros_agent_bridge::claude_sidecar::ClaudeSidecarConfig;
use tauros_agent_bridge::codex::CodexConfig;
use tauros_agent_bridge::{AgentEvent, AuthMode, FinishReason, ModelOption};
use tauros_app::chat::{antigravity_backend, claude_backend, codex_backend, Chat};
use tauros_mcp::TaurosServer;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn project() -> common::TempDir {
    let dir = common::TempDir::new("model-game");
    for args in [&["init", "-q"][..], &["config", "user.name", "Tauros Test"], &["config", "user.email", "test@users.noreply.github.com"],
                 &["commit", "-q", "--allow-empty", "-m", "initial"]] {
        assert!(Command::new("git").current_dir(dir.path()).args(args).status().unwrap().success());
    }
    dir
}

/// Sends `text` and returns the turn's text plus status messages.
fn turn(chat: &Chat, text: &str) -> String {
    let (tx, rx) = mpsc::channel();
    chat.send(text, move |event| { let _ = tx.send(event); }).unwrap();
    let mut seen = String::new();
    loop {
        let event = rx.recv_timeout(Duration::from_secs(60)).expect("turn timed out");
        match event {
            AgentEvent::TextDelta { delta, .. } => seen.push_str(&delta),
            AgentEvent::Status { message, .. } => seen.push_str(&format!("[{message}]")),
            AgentEvent::TurnCompleted { finish_reason, .. } => {
                assert_eq!(finish_reason, FinishReason::Completed, "{seen}");
                break;
            }
            _ => {}
        }
    }
    while chat.busy() {
        std::thread::sleep(Duration::from_millis(10));
    }
    seen
}

fn current(models: &[ModelOption]) -> Vec<&str> {
    models.iter().filter(|m| m.current).map(|m| m.id.as_str()).collect()
}

fn ids(models: &[ModelOption]) -> Vec<&str> {
    models.iter().map(|m| m.id.as_str()).collect()
}

#[test]
fn antigravity_lists_and_switches_models_within_and_across_sessions() {
    let home = common::TempDir::new("model-agy-home");
    let dir = project();
    let (home_path, dir_path) = (home.path().to_owned(), dir.path().to_owned());
    let chat = Chat::new(move || {
        let mut config = AntigravityConfig::new(&home_path);
        config.binary = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/agy_acp_server.py");
        antigravity_backend(config, None, dir_path.clone(), None)
    });
    let models = chat.models().unwrap();
    assert_eq!(ids(&models), ["fixture-flash", "fixture-pro"]);
    assert_eq!(current(&models), ["fixture-flash"]);
    assert!(turn(&chat, "model?").contains("model fixture-flash"));
    assert!(chat.set_model("not-offered").is_err());
    chat.set_model("fixture-pro").unwrap();
    assert_eq!(current(&chat.models().unwrap()), ["fixture-pro"]);
    // Same conversation: the peer checks this is turn 2 of the session.
    assert!(turn(&chat, "continue").contains("turn 2"));
    assert!(turn(&chat, "model?").contains("model fixture-pro"));
    chat.reset();
    assert_eq!(current(&chat.models().unwrap()), ["fixture-pro"], "choice kept for a new conversation");
    assert!(turn(&chat, "model?").contains("model fixture-pro"));
}

#[test]
fn codex_lists_visible_models_and_overrides_following_turns() {
    let home = common::TempDir::new("model-codex-home");
    let dir = project();
    let (home_path, dir_path) = (home.path().to_owned(), dir.path().to_owned());
    let chat = Chat::new(move || {
        let mut config = CodexConfig::new(&home_path);
        config.binary = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex_app_server.py");
        let server = TaurosServer::default().with_operation_gate(Arc::new(Mutex::new(())));
        codex_backend(config, Some(server), dir_path.clone(), AuthMode::HostSignIn, None)
    });
    let models = chat.models().unwrap();
    assert_eq!(ids(&models), ["synthetic-code-mode-model", "synthetic-large"], "hidden models excluded, pages followed");
    assert_eq!(current(&models), ["synthetic-code-mode-model"]);
    chat.set_model("synthetic-large").unwrap();
    assert_eq!(current(&chat.models().unwrap()), ["synthetic-large"]);
    assert!(turn(&chat, "model?").contains("model synthetic-large"));
    chat.reset();
    assert!(turn(&chat, "model?").contains("model synthetic-large"), "choice kept for a new conversation");
}

#[test]
fn claude_sidecar_lists_and_applies_models_to_the_next_turn() {
    let sidecar = ClaudeSidecarConfig::from_repo_root(&repo_root());
    assert!(sidecar.entry_script.is_file(), "run `npm --prefix sidecar/claude-agent-sdk run build` first");
    let chat = Chat::new(move || {
        let extra = BTreeMap::from([("test_driver".to_owned(), json!("direct_mcp"))]);
        claude_backend(sidecar.clone(), Some(TaurosServer::default()), repo_root(), extra)
    });
    let status = json!({"tool": "tauros_status", "arguments": {}}).to_string();
    let models = chat.models().unwrap();
    assert_eq!(ids(&models), ["default", "fixture-large"]);
    assert_eq!(current(&models), ["default"]);
    assert!(turn(&chat, &status).contains("[model:default]"));
    chat.set_model("fixture-large").unwrap();
    assert_eq!(current(&chat.models().unwrap()), ["fixture-large"]);
    assert!(turn(&chat, &status).contains("[model:fixture-large]"));
    chat.reset();
    assert!(turn(&chat, &status).contains("[model:fixture-large]"), "choice kept for a new conversation");
    chat.set_model("default").unwrap();
    assert!(turn(&chat, &status).contains("[model:default]"));
}

/// Reads each real runtime's model list once; starts sessions but sends no message.
#[test]
#[ignore = "requires TAUROS_LIVE_MODELS=1, the host Claude sign-in, the signed-in Codex home and TAUROS_AGY_BIN; no inference"]
fn live_runtimes_list_models_without_inference() {
    assert_eq!(std::env::var("TAUROS_LIVE_MODELS").as_deref(), Ok("1"));
    let data = std::env::var_os("TAUROS_DATA").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap()).join(".local/share/tauros"));
    let dir = project();
    let report = |provider: &str, models: Vec<ModelOption>| {
        assert!(!models.is_empty(), "{provider}: empty model list");
        assert_eq!(models.iter().filter(|m| m.current).count(), 1, "{provider}: {models:#?}");
        eprintln!("{provider}: {:?}", models.iter().map(|m| format!("{}{}", m.id, if m.current { "*" } else { "" })).collect::<Vec<_>>());
    };
    let sidecar = ClaudeSidecarConfig::from_repo_root(&repo_root());
    let claude = Chat::new(move || claude_backend(sidecar.clone(), None, repo_root(), BTreeMap::new()));
    report("claude", claude.models().unwrap());
    let codex_home = std::env::var_os("TAUROS_CODEX_HOME").map(PathBuf::from).unwrap_or_else(|| data.join("codex"));
    let codex_dir = dir.path().to_owned();
    let codex = Chat::new(move || codex_backend(CodexConfig::new(&codex_home), None, codex_dir.clone(), AuthMode::HostSignIn, None));
    report("codex", codex.models().unwrap());
    let agy_home = common::TempDir::new("model-live-agy");
    let (agy_path, agy_dir) = (agy_home.path().to_owned(), dir.path().to_owned());
    let antigravity = Chat::new(move || {
        let mut config = AntigravityConfig::new(&agy_path);
        config.binary = std::env::var_os("TAUROS_AGY_BIN").expect("TAUROS_AGY_BIN").into();
        antigravity_backend(config, None, agy_dir.clone(), None)
    });
    report("antigravity", antigravity.models().unwrap());
}
