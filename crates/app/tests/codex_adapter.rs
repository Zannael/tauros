//! AGT-006: production protocol, namespace, MCP and host transaction boundaries.
#[path = "../../connector-gen3/tests/common/mod.rs"]
mod common;
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauros_agent_bridge::codex::{CodexAdapter, CodexConfig};
use tauros_agent_bridge::{
    AgentBridge, AgentEvent, AuthMode, ErrorCode, FinishReason, SessionConfig, UserMessage,
};
use tauros_app::chat::{codex_backend, ChatBackend};
use tauros_app::codex_tools::ProjectCodexTools;
use tauros_app::history::{list_history, undo_to, ChangeOrigin};
use tauros_mcp::TaurosServer;
use tauros_model::memory::MemoryConnector;
use tauros_model::{
    Capabilities, Connector, GameFamily, GameInfo, ProjectSnapshot, Species, SpeciesId, TrainerId,
};

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().into()
}
fn fixture() -> common::TempDir {
    let dir = common::TempDir::new("codex-game");
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["config", "user.name", "Tauros Test"]);
    git(
        dir.path(),
        &["config", "user.email", "test@users.noreply.github.com"],
    );
    fs::write(dir.path().join("scene.c"), "original\n").unwrap();
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-qm", "initial"]);
    dir
}
fn connector() -> MemoryConnector {
    let mut species = BTreeMap::new();
    species.insert(
        SpeciesId::from("SPECIES_TEST"),
        Species {
            id: SpeciesId::from("SPECIES_TEST"),
            name: "Test".into(),
            national_dex: None,
        },
    );
    MemoryConnector::new(ProjectSnapshot {
        game: GameInfo {
            family: GameFamily::Gen3,
            engine: "original-synthetic-fixture".into(),
            capabilities: Capabilities {
                max_party_size: 6,
                max_moves: 4,
                min_level: 1,
                max_level: 100,
                held_items: true,
                abilities: true,
            },
        },
        trainers: BTreeMap::new(),
        maps: BTreeMap::new(),
        scenes: BTreeMap::new(),
        species,
        flags: BTreeMap::new(),
    })
}
fn config(home: &Path) -> CodexConfig {
    let mut config = CodexConfig::new(home);
    config.binary =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex_app_server.py");
    config
}
fn events(backend: &mut ChatBackend, prompt: &str) -> Vec<AgentEvent> {
    let turn = backend
        .bridge
        .send_message(&backend.session, UserMessage::text(prompt))
        .unwrap();
    let mut events = Vec::new();
    loop {
        let event = turn
            .events()
            .recv_timeout(Duration::from_secs(30))
            .expect("Codex fixture timed out");
        let terminal = matches!(event, AgentEvent::TurnCompleted { .. });
        events.push(event);
        if terminal {
            return events;
        }
    }
}
fn completed(events: &[AgentEvent]) {
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentEvent::Error { .. })),
        "{events:#?}"
    );
    assert!(
        matches!(
            events.last(),
            Some(AgentEvent::TurnCompleted {
                finish_reason: FinishReason::Completed,
                ..
            })
        ),
        "{events:#?}"
    );
}

#[test]
fn mixed_native_edit_real_mcp_history_undo_continuity_and_snapshot_rejection() {
    let dir = fixture();
    let home = common::TempDir::new("codex-home");
    let snapshot = git(dir.path(), &["rev-parse", "HEAD"]);
    let connector = Arc::new(Mutex::new(
        Box::new(connector()) as Box<dyn Connector + Send>
    ));
    let server = TaurosServer::with_shared_connector(Arc::clone(&connector))
        .with_operation_gate(Arc::new(Mutex::new(())));
    let mut backend = codex_backend(
        config(home.path()),
        Some(server),
        dir.path().into(),
        AuthMode::HostSignIn,
        None,
    )
    .unwrap();
    let result = events(&mut backend, &format!("edit:{}", dir.path().display()));
    completed(&result);
    for name in [
        "tauros_read_file",
        "tauros_write_file",
        "mcp__tauros__create_trainer",
    ] {
        assert!(result.iter().any(|event| matches!(event,AgentEvent::ToolCallCompleted { name:actual,is_error:false,..} if actual == name)),"{result:#?}");
    }
    assert!(connector
        .lock()
        .unwrap()
        .load()
        .unwrap()
        .trainers
        .contains_key(&TrainerId::from("TRAINER_CODEX_FIXTURE")));
    assert_eq!(
        fs::read_to_string(dir.path().join("scene.c")).unwrap(),
        "edited by Codex fixture\n"
    );
    assert_eq!(
        list_history(dir.path(), 1).unwrap()[0].origin,
        ChangeOrigin::AgentGenerated
    );
    completed(&events(&mut backend, "continue"));
    undo_to(dir.path(), &snapshot).unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("scene.c")).unwrap(),
        "original\n"
    );
    // A dirty human edit forces a snapshot commit; a failing hook must reject
    // the write before the provider gets a successful tool result.
    fs::write(dir.path().join("scene.c"), "pending human change\n").unwrap();
    fs::write(
        dir.path().join(".git/hooks/pre-commit"),
        "#!/bin/sh\nexit 1\n",
    )
    .unwrap();
    fs::set_permissions(
        dir.path().join(".git/hooks/pre-commit"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    completed(&events(
        &mut backend,
        &format!("reject:{}", dir.path().display()),
    ));
    assert_eq!(
        fs::read_to_string(dir.path().join("scene.c")).unwrap(),
        "pending human change\n"
    );
    backend.bridge.close_session(&backend.session).unwrap();
}

#[test]
fn cancellation_disconnect_and_auth_refusal_are_terminal() {
    let home = common::TempDir::new("codex-home-cancel");
    let dir = fixture();
    let server = TaurosServer::default().with_operation_gate(Arc::new(Mutex::new(())));
    let mut backend = codex_backend(
        config(home.path()),
        Some(server),
        dir.path().into(),
        AuthMode::HostSignIn,
        None,
    )
    .unwrap();
    let turn = backend
        .bridge
        .send_message(&backend.session, UserMessage::text("wait"))
        .unwrap();
    assert!(matches!(
        turn.events().recv_timeout(Duration::from_secs(5)).unwrap(),
        AgentEvent::TurnStarted { .. }
    ));
    backend
        .bridge
        .cancel(&backend.session, turn.turn_id())
        .unwrap();
    let events = turn.events().collect_all_for_test();
    assert!(
        matches!(
            events.last(),
            Some(AgentEvent::TurnCompleted {
                finish_reason: FinishReason::Cancelled,
                ..
            })
        ),
        "{events:?}"
    );
    let events = events_fn(&mut backend, "crash");
    assert!(matches!(
        events.last(),
        Some(AgentEvent::TurnCompleted {
            finish_reason: FinishReason::Failed,
            ..
        })
    ));
    let error = backend
        .bridge
        .send_message(&backend.session, UserMessage::text("after crash"))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Transport);
    // API-key refusal happens before account/login/start when no key is supplied.
    // Run this assertion only in environments without a user's actual API key.
    if std::env::var_os("OPENAI_API_KEY").is_none() {
        let mut adapter = CodexAdapter::spawn(
            config(home.path()),
            Arc::new(ProjectCodexTools::new(dir.path().into(), None)),
        )
        .unwrap();
        let error = adapter
            .start_session(SessionConfig {
                auth_mode: AuthMode::ApiKey,
                ..SessionConfig::default()
            })
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::AuthRequired);
    }
}
fn events_fn(backend: &mut ChatBackend, prompt: &str) -> Vec<AgentEvent> {
    events(backend, prompt)
}

trait BoundedEvents {
    fn collect_all_for_test(&self) -> Vec<AgentEvent>;
}
impl BoundedEvents for tauros_agent_bridge::EventStream {
    fn collect_all_for_test(&self) -> Vec<AgentEvent> {
        let mut events = vec![];
        loop {
            let event = self.recv_timeout(Duration::from_secs(30)).unwrap();
            let done = matches!(event, AgentEvent::TurnCompleted { .. });
            events.push(event);
            if done {
                return events;
            }
        }
    }
}

#[test]
#[ignore = "requires TAUROS_LIVE_CODEX=1 and a dedicated TAUROS_CODEX_HOME already signed in; sends original synthetic text only"]
fn live_codex_mixed_turn_on_original_synthetic_data() {
    assert_eq!(std::env::var("TAUROS_LIVE_CODEX").as_deref(), Ok("1"));
    let home =
        std::env::var_os("TAUROS_CODEX_HOME").expect("dedicated signed-in Codex home required");
    let dir = fixture();
    let baseline = git(dir.path(), &["rev-parse", "HEAD"]);
    let server =
        TaurosServer::with_connector(connector()).with_operation_gate(Arc::new(Mutex::new(())));
    let mut config = CodexConfig::new(home);
    if let Some(binary) = std::env::var_os("TAUROS_CODEX_BIN") {
        config.binary = binary.into();
    }
    let mut backend = codex_backend(
        config,
        Some(server),
        dir.path().into(),
        AuthMode::HostSignIn,
        std::env::var("TAUROS_CODEX_MODEL").ok(),
    )
    .unwrap();
    let events = events(&mut backend,"This fixture is original synthetic data. Use tauros_read_file to read scene.c, tauros_write_file to replace its contents with exactly 'synthetic\\n', then call the create_trainer Tauros MCP tool with id TRAINER_CODEX_LIVE, name Codex, class Youngster, and party [{species: SPECIES_TEST, level: 7}]. Do not use shell commands or inspect anything else.");
    completed(&events);
    for name in [
        "tauros_read_file",
        "tauros_write_file",
        "mcp__tauros__create_trainer",
    ] {
        assert!(events.iter().any(|event| matches!(event,AgentEvent::ToolCallCompleted { name:actual,is_error:false,.. } if actual == name)),"{events:#?}");
    }
    assert_eq!(
        fs::read_to_string(dir.path().join("scene.c")).unwrap(),
        "synthetic\n"
    );
    assert_eq!(
        list_history(dir.path(), 1).unwrap()[0].origin,
        ChangeOrigin::AgentGenerated
    );
    undo_to(dir.path(), &baseline).unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("scene.c")).unwrap(),
        "original\n"
    );
}

#[test]
fn installed_codex_requires_its_own_login() {
    let binary = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|dir| dir.join("codex"))
        .find(|path| path.is_file());
    let Some(binary) = binary else {
        eprintln!("SKIPPED: no installed Codex runtime");
        return;
    };
    let home = common::TempDir::new("codex-empty-real-home");
    let mut config = CodexConfig::new(home.path());
    config.binary = binary;
    let mut adapter = CodexAdapter::spawn(
        config,
        Arc::new(ProjectCodexTools::new(home.path().into(), None)),
    )
    .unwrap();
    let error = adapter
        .start_session(SessionConfig {
            auth_mode: AuthMode::HostSignIn,
            ..SessionConfig::default()
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::AuthRequired);
}

#[test]
fn codex_shell_retains_failed_and_cancelled_edits_inside_the_project() {
    if std::env::var("TAUROS_NATIVE_SANDBOX_TESTS").as_deref() != Ok("1") {
        eprintln!(
            "SKIPPED: set TAUROS_NATIVE_SANDBOX_TESTS=1 for the real offline namespace check"
        );
        return;
    }
    use std::sync::atomic::{AtomicBool, Ordering};
    use tauros_agent_bridge::codex::CodexTools;
    use tauros_app::native_edit::NativeEditHost;
    let dir = fixture();
    let outside = common::TempDir::new("codex-outside");
    fs::write(outside.path().join("sentinel"), "untouched").unwrap();
    let before_git = fs::read(dir.path().join(".git/config")).unwrap();
    let before = git(dir.path(), &["rev-parse", "HEAD"]);
    let gate = Arc::new(Mutex::new(()));
    let host = NativeEditHost::start(dir.path(), Arc::clone(&gate)).unwrap();
    let tools = Arc::new(ProjectCodexTools::new(
        dir.path().into(),
        Some(host.socket_path().into()),
    ));
    let error = tools.call("tauros_shell",serde_json::json!({"command":format!(
        "printf 'partial\\n' > shell.c; printf bad > '{}'; printf bad > .git/config; exit 3",outside.path().join("sentinel").display())}), &AtomicBool::new(false)).unwrap_err();
    assert!(error.contains("command failed"), "{error}");
    assert_eq!(
        fs::read_to_string(dir.path().join("shell.c")).unwrap(),
        "partial\n"
    );
    assert_eq!(
        fs::read(outside.path().join("sentinel")).unwrap(),
        b"untouched"
    );
    assert_eq!(
        fs::read(dir.path().join(".git/config")).unwrap(),
        before_git
    );
    assert_eq!(
        list_history(dir.path(), 1).unwrap()[0].origin,
        ChangeOrigin::AgentGenerated
    );
    undo_to(dir.path(), &before).unwrap();
    assert!(!dir.path().join("shell.c").exists());
    let cancellation = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::clone(&cancellation);
    let worker_tools = Arc::clone(&tools);
    let worker = std::thread::spawn(move || {
        worker_tools.call(
            "tauros_shell",
            serde_json::json!({"command":"printf 'cancelled edit\\n' > shell.c; sleep 30"}),
            &cancelled,
        )
    });
    for _ in 0..250 {
        if dir.path().join("shell.c").exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(dir.path().join("shell.c").exists());
    assert!(gate.try_lock().is_err());
    cancellation.store(true, Ordering::SeqCst);
    assert!(worker.join().unwrap().unwrap_err().contains("cancelled"));
    assert!(gate.try_lock().is_ok());
    assert_eq!(
        list_history(dir.path(), 1).unwrap()[0].origin,
        ChangeOrigin::AgentGenerated
    );
}

#[test]
#[ignore = "requires OPENAI_API_KEY=tauros-local-protocol-fixture-key; talks only to the deterministic local peer"]
fn api_key_login_protocol_uses_only_the_documented_runtime_flow() {
    assert_eq!(
        std::env::var("OPENAI_API_KEY").as_deref(),
        Ok("tauros-local-protocol-fixture-key")
    );
    let home = common::TempDir::new("codex-api-protocol");
    let dir = fixture();
    let server =
        TaurosServer::with_connector(connector()).with_operation_gate(Arc::new(Mutex::new(())));
    let mut backend = codex_backend(
        config(home.path()),
        Some(server),
        dir.path().into(),
        AuthMode::ApiKey,
        None,
    )
    .unwrap();
    let result = events(&mut backend, &format!("edit:{}", dir.path().display()));
    completed(&result);
    assert!(!format!("{result:?}").contains("tauros-local-protocol-fixture-key"));
}

/// Real subscription-runtime acceptance: no game, source reads, writes or Git.
#[test]
#[ignore = "requires TAUROS_LIVE_CODEX=1 and the dedicated signed-in home; synthetic MCP status only"]
fn live_codex_mcp_status_on_synthetic_data() {
    use tauros_app::mcp_host::InProcessMcpHost;
    assert_eq!(std::env::var("TAUROS_LIVE_CODEX").as_deref(), Ok("1"));
    let home = std::env::var_os("TAUROS_CODEX_HOME").expect("dedicated signed-in home required");
    let dir = common::TempDir::new("codex-status");
    let host = InProcessMcpHost::start(TaurosServer::with_connector(connector())).unwrap();
    let script = fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../sidecar/codex/mcp-bridge.mjs")).unwrap();
    let node = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|dir| dir.join("node")).find(|path| path.is_file())
        .and_then(|path| fs::canonicalize(path).ok()).expect("Node.js required");
    let mut config = CodexConfig::new(home);
    config.timeout = Duration::from_secs(60);
    config.mcp_mounts = vec![node.clone(), script.clone(), host.socket_path().parent().unwrap().into()];
    if let Some(binary) = std::env::var_os("TAUROS_CODEX_BIN") { config.binary = binary.into(); }
    let mut bridge = CodexAdapter::spawn(config,
        Arc::new(ProjectCodexTools::new(dir.path().into(), None))).unwrap();
    let session = bridge.start_session(SessionConfig {
        auth_mode: AuthMode::HostSignIn,
        model: std::env::var("TAUROS_CODEX_MODEL").ok(),
        system_prompt: Some("This is a synthetic Tauros transport acceptance check. Call the requested tool before answering; never inspect files or use other tools.".into()),
        mcp_servers: BTreeMap::from([("tauros".into(), host.mcp_server_config(&node, &script))]),
        ..SessionConfig::default()
    }).unwrap();
    eprintln!("Runtime model={:?}", bridge.session_model(&session));
    let catalog = bridge.mcp_server_status(&session, "tauros").unwrap();
    // Only public server state and tool names are printed, never raw protocol/auth.
    for server in catalog["data"].as_array().into_iter().flatten() {
        eprintln!("MCP name={} runtime={} discovery_error={} tools={:?}",
            server["name"], server["runtimeStatus"], !server["toolsError"].is_null(),
            server["tools"].as_object().map(|tools| tools.keys().collect::<Vec<_>>()));
    }
    eprintln!("MCP server count={}", catalog["data"].as_array().map_or(0, Vec::len));
    let turn = bridge.send_message(&session, UserMessage::text(
        "Call the Tauros MCP tauros_status tool with {} now, exactly once. Reply with its returned version only. Do not infer the version or claim success without calling it. No files, edits or shell commands." )).unwrap();
    let mut observed = Vec::new();
    loop {
        let event = turn.events().recv_timeout(Duration::from_secs(90)).expect("real Codex turn timed out");
        let terminal = matches!(event, AgentEvent::TurnCompleted { .. });
        eprintln!("{event:?}");
        observed.push(event);
        if terminal { break; }
    }
    completed(&observed);
    let calls: Vec<_> = observed.iter().filter_map(|event| match event {
        AgentEvent::ToolCallCompleted { name, is_error: false, output, .. }
            if name == "mcp__tauros__tauros_status" => Some(output),
        _ => None,
    }).collect();
    assert_eq!(calls.len(), 1,
        "MCP catalog was loaded, but the model did not execute exactly one successful Tauros status call");
    assert!(calls[0]["result"]["content"].as_array().unwrap().iter().any(|block|
        block["text"] == "Tauros 0.1.0 ready"));
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    bridge.close_session(&session).unwrap();
}

#[test]
fn mcp_readiness_uses_thread_catalog_with_code_mode_host_enabled() {
    let home = common::TempDir::new("codex-readiness-home");
    let dir = common::TempDir::new("codex-readiness-project");
    let mut bridge = CodexAdapter::spawn(config(home.path()),
        Arc::new(ProjectCodexTools::new(dir.path().into(), Some(dir.path().join("unused.sock"))))).unwrap();
    let session = bridge.start_session(SessionConfig {
        auth_mode: AuthMode::HostSignIn,
        mcp_servers: BTreeMap::from([("tauros".into(), tauros_agent_bridge::McpServerConfig::Stdio {
            command: "synthetic-unused-command".into(), args: vec![], env: BTreeMap::new()
        })]),
        ..SessionConfig::default()
    }).unwrap();
    assert_eq!(bridge.session_model(&session).as_deref(), Some("synthetic-code-mode-model"));
    bridge.close_session(&session).unwrap();
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[path = "fixtures/synthetic_framebuffer.rs"]
mod synthetic_framebuffer;

/// The prompt and tool metadata never disclose the pixel arrangement/answer.
#[test]
#[ignore = "requires TAUROS_LIVE_CODEX=1; real subscription vision on original synthetic pixels only"]
fn live_codex_screenshot_before_after_on_synthetic_pixels() {
    use synthetic_framebuffer::{picture, SyntheticFramebuffer};
    use tauros_mcp::emulator::EmuSession;
    assert_eq!(std::env::var("TAUROS_LIVE_CODEX").as_deref(), Ok("1"));
    let home = std::env::var_os("TAUROS_CODEX_HOME").expect("dedicated signed-in home required");
    let dir = common::TempDir::new("codex-vision");
    // Vary the answer between runs without disclosing it in the prompt or metadata.
    let right = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap().subsec_nanos() & 1 != 0;
    let pixels = Arc::new(Mutex::new(picture(right, false)));
    let mut emulator = EmuSession::new(SyntheticFramebuffer(Arc::clone(&pixels)));
    emulator.load_rom(vec![0]).unwrap(); // Synthetic session marker, never a game ROM.
    let server = TaurosServer::default().with_emulator(emulator);
    let mut config = CodexConfig::new(home);
    config.timeout = Duration::from_secs(60);
    if let Some(binary) = std::env::var_os("TAUROS_CODEX_BIN") { config.binary = binary.into(); }
    let mut backend = codex_backend(config, Some(server), dir.path().into(),
        AuthMode::HostSignIn, std::env::var("TAUROS_CODEX_MODEL").ok()).unwrap();
    let mut answers = Vec::new();
    let mut native_images = Vec::new();
    for after in [false, true] {
        if after { *pixels.lock().unwrap() = picture(!right, true); }
        let prompt = if after {
            "The synthetic framebuffer has changed. Call emu_screenshot with {} once and visually compare it with the previous screenshot. Return only JSON with large_red_square_side (left or right), red_shape_count (integer), blue_circle_position (top or bottom), and fragment_remaining (boolean indicating a small remnant at the former large square position). Do not infer success from tool metadata. Do not use files, shell, source inspection or manually decode PNG bytes."
        } else {
            "This is an original synthetic visual test, with no game or source files. Call emu_screenshot with {} once and inspect the image. Return only JSON with large_red_square_side (left or right), red_shape_count (integer), and blue_circle_position (top or bottom). Do not infer pixels from tool metadata. Do not use files, shell, source inspection or manually decode PNG bytes."
        };
        let turn = backend.bridge.send_message(&backend.session, UserMessage::text(prompt)).unwrap();
        let mut observed = Vec::new();
        let mut text = String::new();
        let mut image_count = 0;
        let mut screenshot_calls = 0;
        loop {
            let event = turn.events().recv_timeout(Duration::from_secs(120)).expect("vision turn timed out");
            match &event {
                AgentEvent::TextDelta { delta, .. } => text.push_str(delta),
                AgentEvent::ToolCallCompleted { name, is_error, output, .. } => {
                    eprintln!("tool={name} error={is_error}");
                    if name == "mcp__tauros__emu_screenshot" && !is_error {
                        screenshot_calls += 1;
                        for block in output["result"]["content"].as_array().into_iter().flatten() {
                            if block["type"] == "image" { image_count += 1; }
                        }
                    }
                }
                _ => {}
            }
            let terminal = matches!(event, AgentEvent::TurnCompleted { .. });
            observed.push(event);
            if terminal { break; }
        }
        // No event dump: screenshot bodies may contain large base64 payloads.
        assert!(matches!(observed.last(), Some(AgentEvent::TurnCompleted {
            finish_reason: FinishReason::Completed, .. })));
        assert_eq!(screenshot_calls, 1, "expected one actual successful screenshot call");
        eprintln!("phase={} native_images={image_count} observation={text}", if after {"after"} else {"before"});
        let json = text.find('{').zip(text.rfind('}'))
            .and_then(|(start, end)| serde_json::from_str::<serde_json::Value>(&text[start..=end]).ok());
        answers.push(json.unwrap_or(serde_json::Value::Null));
        native_images.push(image_count);
    }
    backend.bridge.close_session(&backend.session).unwrap();
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    assert_eq!(native_images, vec![1, 1], "screenshots must reach MCP as native image blocks");
    for (index, side) in [if right {"right"} else {"left"}, if right {"left"} else {"right"}].iter().enumerate() {
        assert_eq!(answers[index]["large_red_square_side"], *side, "{answers:?}");
        assert_eq!(answers[index]["red_shape_count"], index + 1, "{answers:?}");
        assert_eq!(answers[index]["blue_circle_position"], "top", "{answers:?}");
    }
    assert_eq!(answers[1]["fragment_remaining"], true, "{answers:?}");
}

#[test]
fn app_shutdown_closes_active_codex_session_with_chat_retained() {
    let home = common::TempDir::new("codex-shutdown");
    let dir = fixture();
    let server = TaurosServer::default().with_operation_gate(Arc::new(Mutex::new(())));
    let backend = codex_backend(config(home.path()), Some(server), dir.path().into(), AuthMode::HostSignIn, None).unwrap();
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
