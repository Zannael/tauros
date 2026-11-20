//! Native/MCP transaction and recovery checks, using disposable game projects.
#[path = "../../connector-gen3/tests/common/mod.rs"]
mod common;

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{symlink, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use tauros_app::history::{list_history, undo_to, ChangeOrigin};
use tauros_app::native_edit::{source_path, NativeEditHost};
use tauros_connector_gen3::Gen3Connector;
use tauros_model::{Change, ChangeSet, Connector, Provenance, TrainerId};

fn git(root: &Path, args: &[&str]) -> String {
    let result = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8_lossy(&result.stdout).trim().to_owned()
}

fn fixture() -> common::TempDir {
    let dir = common::TempDir::new("native-guard");
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

fn read(reader: &mut BufReader<UnixStream>) -> Value {
    let mut line = String::new();
    assert!(reader.read_line(&mut line).unwrap() > 0);
    serde_json::from_str(&line).unwrap()
}

fn begin(
    host: &NativeEditHost,
    tool: &str,
    path: &str,
) -> (UnixStream, BufReader<UnixStream>, Value) {
    let mut stream = UnixStream::connect(host.socket_path()).unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    writeln!(stream, "{}", json!({ "tool": tool, "path": path })).unwrap();
    let result = read(&mut reader);
    (stream, reader, result)
}

#[test]
fn native_edits_are_serialized_recorded_and_reversible_even_after_disconnect() {
    let dir = fixture();
    let gate = Arc::new(Mutex::new(()));
    let host = NativeEditHost::start(dir.path(), Arc::clone(&gate)).unwrap();
    let original = git(dir.path(), &["rev-parse", "HEAD"]);
    let (mut stream, mut reader, start) = begin(&host, "Edit", "scene.c");
    assert_eq!(start["ok"], true);
    assert!(gate.try_lock().is_err());
    let (_, _, busy) = begin(&host, "Write", "other.c");
    assert_eq!(busy["ok"], false);
    fs::write(dir.path().join("scene.c"), "native edit\n").unwrap();
    writeln!(stream, "{{}}").unwrap();
    assert_eq!(read(&mut reader)["ok"], true);
    drop(stream);
    let history = list_history(dir.path(), 10).unwrap();
    assert_eq!(history[0].origin, ChangeOrigin::AgentGenerated);
    assert_eq!(
        history[0].snapshot_commit.as_deref(),
        Some(original.as_str())
    );
    undo_to(dir.path(), &original).unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("scene.c")).unwrap(),
        "original\n"
    );

    let (stream, reader, start) = begin(&host, "Bash", "");
    assert_eq!(start["ok"], true);
    fs::write(dir.path().join("scene.c"), "partial command result\n").unwrap();
    drop(reader);
    drop(stream);
    // Dropping the host joins active transactions, including EOF recovery.
    drop(host);
    assert_eq!(
        list_history(dir.path(), 1).unwrap()[0].origin,
        ChangeOrigin::AgentGenerated
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("scene.c")).unwrap(),
        "partial command result\n"
    );
}

#[test]
fn failed_snapshot_and_escaping_paths_cannot_authorize_a_write() {
    let dir = fixture();
    let host = NativeEditHost::start(dir.path(), Arc::new(Mutex::new(()))).unwrap();
    fs::write(dir.path().join("scene.c"), "pending human edit\n").unwrap();
    let hook = dir.path().join(".git/hooks/pre-commit");
    fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    let (_, _, denied) = begin(&host, "Edit", "scene.c");
    assert_eq!(denied["ok"], false);
    assert!(denied["message"]
        .as_str()
        .unwrap()
        .contains("snapshot failed"));
    assert_eq!(
        fs::read_to_string(dir.path().join("scene.c")).unwrap(),
        "pending human edit\n"
    );
    for file in ["../outside.c", ".git/config", ".gitignore", "/etc/passwd"] {
        assert!(source_path(dir.path(), Path::new(file)).is_err(), "{file}");
    }
    symlink("/tmp", dir.path().join("escape")).unwrap();
    assert!(source_path(dir.path(), Path::new("escape/native.c")).is_err());
    fs::remove_file(hook).unwrap();
    let (mut stream, mut reader, accepted) = begin(&host, "Write", "new.c");
    assert_eq!(
        accepted["ok"], true,
        "the session recovers after fixing the snapshot failure"
    );
    writeln!(stream, "{{}}").unwrap();
    assert_eq!(read(&mut reader)["ok"], true);
}

#[test]
fn sdk_policy_hooks_use_the_real_native_host() {
    let dir = fixture();
    let host = NativeEditHost::start(dir.path(), Arc::new(Mutex::new(()))).unwrap();
    let result = Command::new("node")
        .arg(common::repo_root().join("sidecar/claude-agent-sdk/tests/native-tools.mjs"))
        .arg(dir.path())
        .arg(host.socket_path())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        list_history(dir.path(), 1).unwrap()[0].origin,
        ChangeOrigin::AgentGenerated
    );
}

#[test]
fn failed_recording_keeps_edits_and_recovers_their_provenance_after_restart() {
    let dir = fixture();
    let host = NativeEditHost::start(dir.path(), Arc::new(Mutex::new(()))).unwrap();
    let (mut stream, mut reader, start) = begin(&host, "Edit", "scene.c");
    assert_eq!(start["ok"], true);
    let hook = dir.path().join(".git/hooks/pre-commit");
    fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(dir.path().join("scene.c"), "recover me\n").unwrap();
    writeln!(stream, "{{}}").unwrap();
    assert_eq!(read(&mut reader)["ok"], false);
    assert_eq!(
        fs::read_to_string(dir.path().join("scene.c")).unwrap(),
        "recover me\n"
    );
    let guard = tauros_connector_gen3::snapshot::GitGuard::take_over(dir.path()).unwrap();
    assert!(guard.snapshot("domain write must wait").is_err());
    drop(host);
    fs::remove_file(hook).unwrap();
    let host = NativeEditHost::start(dir.path(), Arc::new(Mutex::new(()))).unwrap();
    let (mut stream, mut reader, recovered) = begin(&host, "Write", "next.c");
    assert_eq!(recovered["ok"], true);
    writeln!(stream, "{{}}").unwrap();
    assert_eq!(read(&mut reader)["ok"], true);
    assert_eq!(
        list_history(dir.path(), 1).unwrap()[0].origin,
        ChangeOrigin::AgentGenerated
    );
    assert!(!dir.path().join(".git/tauros-native-pending").exists());
}

#[test]
#[ignore = "requires TAUROS_LIVE_CLAUDE=1; sends only original synthetic test data, never decomp sources"]
fn live_sdk_hooks_on_original_synthetic_data_only() {
    use std::collections::BTreeMap;
    use std::time::Duration;
    use tauros_agent_bridge::claude_sidecar::ClaudeSidecarConfig;
    use tauros_agent_bridge::{AgentEvent, FinishReason, UserMessage};
    use tauros_app::chat::claude_backend;
    use tauros_mcp::TaurosServer;

    assert_eq!(std::env::var("TAUROS_LIVE_CLAUDE").as_deref(), Ok("1"));
    // This fixture is authored entirely in this test: it contains no decomp,
    // game assets, user source code, credentials or personal data.
    let dir = fixture();
    let server = TaurosServer::default().with_operation_gate(Arc::new(Mutex::new(())));
    let mut backend = claude_backend(
        ClaudeSidecarConfig::from_repo_root(&common::repo_root()),
        Some(server),
        dir.path().to_owned(),
        BTreeMap::new(),
    )
    .unwrap();
    let turn = backend
        .bridge
        .send_message(
            &backend.session,
            UserMessage::text(
                "This is an original synthetic fixture, not a game project. Sequentially: \
        use native Read on scene.c; use native Edit to replace original with synthetic; \
        use native Bash to run `printf 'shell fixture\\n' > shell.c`; \
        call the Tauros tauros_status MCP tool. Do not read anything else or ask questions.",
            ),
        )
        .unwrap();
    let (_, stream) = turn.into_parts();
    let mut tools = Vec::new();
    loop {
        match stream
            .recv_timeout(Duration::from_secs(120))
            .expect("synthetic SDK test timed out")
        {
            AgentEvent::ToolCallStarted { name, .. } => {
                eprintln!("synthetic tool: {name}");
                tools.push(name);
            }
            AgentEvent::ToolCallCompleted {
                name,
                is_error,
                output,
                ..
            } => assert!(
                !is_error,
                "{name}: {}",
                output.to_string().chars().take(1200).collect::<String>()
            ),
            AgentEvent::Error { message, .. } => panic!("{message}"),
            AgentEvent::TurnCompleted { finish_reason, .. } => {
                assert_eq!(finish_reason, FinishReason::Completed);
                break;
            }
            _ => (),
        }
    }
    for name in ["Read", "Edit", "Bash", "mcp__tauros__tauros_status"] {
        assert!(
            tools.iter().any(|actual| actual == name),
            "missing {name}: {tools:?}"
        );
    }
    assert_eq!(
        fs::read_to_string(dir.path().join("scene.c")).unwrap(),
        "synthetic\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("shell.c")).unwrap(),
        "shell fixture\n"
    );
    assert_eq!(
        list_history(dir.path(), 2)
            .unwrap()
            .iter()
            .filter(|entry| entry.origin == ChangeOrigin::AgentGenerated)
            .count(),
        2
    );
    backend.bridge.close_session(&backend.session).unwrap();
}

#[test]
#[ignore = "requires TAUROS_LIVE_CLAUDE=1, TAUROS_DOCKER_TESTS=1 and host Claude sign-in"]
fn live_agent_combines_native_c_edit_domain_tool_build_and_shared_preview() {
    use std::collections::BTreeMap;
    use std::time::Duration;
    use tauros_agent_bridge::claude_sidecar::ClaudeSidecarConfig;
    use tauros_agent_bridge::{AgentEvent, FinishReason, UserMessage};
    use tauros_app::chat::claude_backend;
    use tauros_app::mcp_host::ProjectSession;
    use tauros_connector_gen3::toolchain::Toolchain;

    assert_eq!(std::env::var("TAUROS_LIVE_CLAUDE").as_deref(), Ok("1"));
    assert_eq!(std::env::var("TAUROS_DOCKER_TESTS").as_deref(), Ok("1"));
    let engine = common::engine().expect("pinned engine required");
    let dir = common::built_fixture(&engine, "native-live-mixed").expect("base build required");
    let toolchain = Toolchain::for_repository(&common::repo_root()).unwrap();
    let project = ProjectSession::open(dir.path(), Some(toolchain), None).unwrap();
    let mut backend = claude_backend(
        ClaudeSidecarConfig::from_repo_root(&common::repo_root()),
        Some(project.mcp_server()),
        dir.path().to_owned(),
        BTreeMap::new(),
    )
    .unwrap();
    let prompt = "Execute this integration check in order, with tools sequentially. \
        1. Read src/battle_setup.c with native Read, then use native Edit to add \
        `volatile unsigned int gTaurosNativeProbe;` immediately before the definition of \
        `static void CB2_EndTrainerBattle(void)` (not its prototype), and add \
        `gTaurosNativeProbe = 0x54415552;` as the first statement of that function. \
        2. Call the Tauros create_trainer MCP tool with id TRAINER_NATIVE_MIXED, name Native, \
        class Leader, party [{species: SPECIES_POOCHYENA, level: 5}], \
        extensions {Pic: Leader Roxanne, Gender: Female, Music: Female}. \
        3. Call the Tauros build MCP tool. 4. Call emu_boot with kind boot_to, \
        map MAP_PETALBURG_CITY, x 15, y 10. 5. Call emu_screenshot without a file path. \
        Do not use shell commands or ask questions. Report failures accurately.";
    let turn = backend
        .bridge
        .send_message(&backend.session, UserMessage::text(prompt))
        .unwrap();
    let (_, stream) = turn.into_parts();
    let mut names = Vec::new();
    loop {
        let event = stream
            .recv_timeout(Duration::from_secs(240))
            .expect("live agent timed out");
        match event {
            AgentEvent::ToolCallStarted { name, .. } => {
                eprintln!("live tool: {name}");
            }
            AgentEvent::ToolCallCompleted {
                name,
                is_error,
                output,
                ..
            } => {
                // A denied exploratory read is recoverable agent feedback. Only
                // successful required operations count toward completion below.
                if is_error {
                    eprintln!("live tool rejected: {name}: {}", output.to_string().chars().take(600).collect::<String>());
                } else {
                    names.push(name);
                }
            }
            AgentEvent::Error { message, .. } => panic!("{message}"),
            AgentEvent::TurnCompleted { finish_reason, .. } => {
                assert_eq!(finish_reason, FinishReason::Completed);
                break;
            }
            _ => (),
        }
    }
    for name in [
        "Read",
        "Edit",
        "mcp__tauros__create_trainer",
        "mcp__tauros__build",
        "mcp__tauros__emu_boot",
        "mcp__tauros__emu_screenshot",
    ] {
        assert!(
            names.iter().any(|actual| actual == name),
            "missing {name}: {names:?}"
        );
    }
    assert!(fs::read_to_string(dir.path().join("pokeemerald.sym"))
        .unwrap()
        .contains("gTaurosNativeProbe"));
    assert!(
        fs::read_to_string(dir.path().join("src/data/trainers.party"))
            .unwrap()
            .contains("TRAINER_NATIVE_MIXED")
    );
    let session = project.emulator();
    let session = session.lock().unwrap();
    assert!(session.has_rom());
    assert!(session.emulator().frame_count() > 0);
    assert!(list_history(dir.path(), 10)
        .unwrap()
        .iter()
        .any(|e| e.summary == "tauros: native Edit" && e.origin == ChangeOrigin::AgentGenerated));
    backend.bridge.close_session(&backend.session).unwrap();
}

#[test]
fn existing_connector_rereads_native_changes_before_applying_domain_edits() {
    let Some(engine) = common::engine() else {
        return;
    };
    let dir = common::fixture(
        &engine,
        "native-model-refresh",
        &["src", "include", "data", "tools/trainerproc"],
    );
    let mut connector = Gen3Connector::open(dir.path(), None).unwrap();
    let id = TrainerId::from("TRAINER_ROXANNE_1");
    let original = connector.load().unwrap().trainers[&id].clone();
    let host = NativeEditHost::start(dir.path(), Arc::new(Mutex::new(()))).unwrap();
    let path = dir.path().join("src/data/trainers.party");
    let (mut stream, mut reader, accepted) = begin(&host, "Edit", "src/data/trainers.party");
    assert_eq!(accepted["ok"], true);
    let text = fs::read_to_string(&path).unwrap();
    let marker = "=== TRAINER_ROXANNE_1 ===";
    // Select the named block; native edits must survive a later domain edit.
    let offset = text.find(marker).expect("trainer block");
    let tail = &text[offset..];
    let changed = tail.replacen("Level: 12", "Level: 14", 1);
    assert_ne!(tail, changed);
    fs::write(&path, format!("{}{changed}", &text[..offset])).unwrap();
    writeln!(stream, "{{}}").unwrap();
    assert_eq!(read(&mut reader)["ok"], true);
    let mut trainer = connector.load().unwrap().trainers[&id].clone();
    assert_eq!(trainer.party.members[0].level, 14);
    trainer.name = "Native".into();
    connector
        .apply(&ChangeSet {
            summary: "domain edit after native edit".into(),
            provenance: Provenance::AiGenerated,
            changes: vec![Change::UpsertTrainer { trainer }],
        })
        .unwrap();
    let updated = connector.load().unwrap().trainers[&id].clone();
    assert_eq!(updated.party.members[0].level, 14);
    assert_eq!(updated.name, "Native");
    assert_eq!(original.party.members[0].level, 12);
}

#[test]
fn local_sidecar_combines_native_c_edit_mcp_build_and_shared_preview() {
    use std::collections::BTreeMap;
    use std::time::Duration;
    use tauros_agent_bridge::claude_sidecar::ClaudeSidecarConfig;
    use tauros_agent_bridge::{AgentEvent, FinishReason, UserMessage};
    use tauros_app::chat::claude_backend;
    use tauros_app::mcp_host::ProjectSession;
    use tauros_connector_gen3::toolchain::Toolchain;

    if std::env::var("TAUROS_DOCKER_TESTS").as_deref() != Ok("1") {
        eprintln!("SKIPPED local mixed workflow: set TAUROS_DOCKER_TESTS=1");
        return;
    }
    let engine = common::engine().expect("pinned engine required");
    let dir = common::built_fixture(&engine, "native-local-mixed").expect("base build required");
    let project = ProjectSession::open(
        dir.path(),
        Some(Toolchain::for_repository(&common::repo_root()).unwrap()),
        None,
    )
    .unwrap();
    let extra = BTreeMap::from([("test_driver".into(), json!("direct_mcp"))]);
    let mut backend = claude_backend(
        ClaudeSidecarConfig::from_repo_root(&common::repo_root()),
        Some(project.mcp_server()),
        dir.path().to_owned(),
        extra,
    )
    .unwrap();
    let original = fs::read_to_string(dir.path().join("src/battle_setup.c")).unwrap();
    let edited = original.replacen("static void CB2_EndTrainerBattle(void)\n{",
        "volatile unsigned int gTaurosNativeProbe;\n\nstatic void CB2_EndTrainerBattle(void)\n{\n    gTaurosNativeProbe = 0x54415552;", 1);
    assert_ne!(original, edited);
    let directive = json!({ "steps": [
        { "native": true, "tool": "Write", "arguments": {
            "file_path": "src/battle_setup.c", "content": edited
        } },
        { "tool": "create_trainer", "arguments": {
            "id": "TRAINER_NATIVE_LOCAL", "name": "Native", "class": "Leader",
            "party": [{ "species": "SPECIES_POOCHYENA", "level": 5 }],
            "extensions": { "Pic": "Leader Roxanne", "Gender": "Female", "Music": "Female" }
        } },
        { "tool": "build" },
        { "tool": "emu_boot", "arguments": { "kind": "boot_to", "map": "MAP_PETALBURG_CITY", "x": 15, "y": 10 } },
        { "tool": "emu_screenshot", "arguments": {} }
    ] });
    let turn = backend
        .bridge
        .send_message(&backend.session, UserMessage::text(directive.to_string()))
        .unwrap();
    let (_, stream) = turn.into_parts();
    let mut tools = Vec::new();
    loop {
        let event = stream
            .recv_timeout(Duration::from_secs(240))
            .expect("mixed local workflow timed out");
        match event {
            AgentEvent::ToolCallStarted { name, .. } => {
                eprintln!("local tool: {name}");
                tools.push(name);
            }
            AgentEvent::ToolCallCompleted {
                name,
                is_error,
                output,
                ..
            } => assert!(
                !is_error,
                "{name}: {}",
                output.to_string().chars().take(1200).collect::<String>()
            ),
            AgentEvent::Error { message, .. } => panic!("{message}"),
            AgentEvent::TurnCompleted { finish_reason, .. } => {
                assert_eq!(finish_reason, FinishReason::Completed);
                break;
            }
            _ => (),
        }
    }
    assert_eq!(
        tools,
        [
            "Write",
            "create_trainer",
            "build",
            "emu_boot",
            "emu_screenshot"
        ]
    );
    assert!(fs::read_to_string(dir.path().join("pokeemerald.sym"))
        .unwrap()
        .contains("gTaurosNativeProbe"));
    assert!(
        fs::read_to_string(dir.path().join("src/data/trainers.party"))
            .unwrap()
            .contains("TRAINER_NATIVE_LOCAL")
    );
    let emu = project.emulator();
    let emu = emu.lock().unwrap();
    assert!(emu.has_rom());
    assert!(emu.emulator().frame_count() > 0);
    assert!(list_history(dir.path(), 10)
        .unwrap()
        .iter()
        .any(|e| e.summary == "tauros: native Write" && e.origin == ChangeOrigin::AgentGenerated));
    backend.bridge.close_session(&backend.session).unwrap();
}
