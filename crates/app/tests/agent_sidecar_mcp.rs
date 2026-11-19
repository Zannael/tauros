//! AGT-002 integration tests: launches the compiled TypeScript Agent SDK sidecar
//! via `ClaudeSidecarAdapter`, connects it to Tauros's in-process `TaurosServer`
//! over a private local Unix domain socket (`InProcessMcpHost` + `mcp-bridge.js`),
//! and verifies domain tool execution, streaming events, RES-002 auth enforcement
//! and mid-stream cancellation.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;
use tauros_agent_bridge::claude_sidecar::{ClaudeSidecarAdapter, ClaudeSidecarConfig};
use tauros_agent_bridge::{
    AgentBridge, AgentEvent, AuthMode, ErrorCode, FinishReason, SessionConfig, UserMessage,
};
use tauros_app::mcp_host::InProcessMcpHost;
use tauros_mcp::TaurosServer;
use tauros_model::memory::MemoryConnector;
use tauros_model::{
    Capabilities, Connector, GameFamily, GameInfo, ProjectSnapshot, Species, SpeciesId, TrainerId,
};

fn sample_connector() -> MemoryConnector {
    let mut snapshot = ProjectSnapshot {
        game: GameInfo {
            family: GameFamily::Gen3,
            engine: "pokeemerald-expansion".to_owned(),
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
        species: BTreeMap::new(),
        flags: BTreeMap::new(),
    };
    for (id, name, dex) in [
        ("SPECIES_POOCHYENA", "Poochyena", 261),
        ("SPECIES_ZIGZAGOON", "Zigzagoon", 263),
        ("SPECIES_WURMPLE", "Wurmple", 265),
    ] {
        snapshot.species.insert(
            SpeciesId::from(id),
            Species {
                id: SpeciesId::from(id),
                name: name.to_owned(),
                national_dex: Some(dex),
            },
        );
    }
    MemoryConnector::new(snapshot)
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn sidecar_config() -> ClaudeSidecarConfig {
    ClaudeSidecarConfig::from_repo_root(&repo_root())
}

#[test]
fn sidecar_executes_domain_tool_on_in_process_mcp_and_streams_events() {
    let cfg = sidecar_config();
    assert!(
        cfg.entry_script.is_file(),
        "compiled sidecar missing at {} — run `npm --prefix sidecar/claude-agent-sdk run build`",
        cfg.entry_script.display()
    );

    let shared_connector: Arc<Mutex<Box<dyn Connector + Send>>> =
        Arc::new(Mutex::new(Box::new(sample_connector())));
    let server = TaurosServer::with_shared_connector(Arc::clone(&shared_connector));
    let mcp_host = InProcessMcpHost::start(server).expect("in-process MCP host should start");

    let mut adapter = ClaudeSidecarAdapter::spawn(cfg.clone()).expect("sidecar should start");
    assert!(adapter.is_alive());

    let caps = adapter.capabilities();
    assert_eq!(caps.provider_name, "claude-agent-sdk");
    assert!(caps.supports_streaming);
    assert!(caps.supports_tool_calls);
    assert!(caps.supports_cancel);
    assert_eq!(caps.supported_auth, vec![AuthMode::HostSignIn]);

    // RES-002: AGT-002 personal-use sidecar rejects non-HostSignIn auth modes.
    let bad_auth = adapter.start_session(SessionConfig {
        auth_mode: AuthMode::ApiKey,
        ..SessionConfig::default()
    });
    assert_eq!(bad_auth.unwrap_err().code, ErrorCode::InvalidConfig);

    let mut mcp_servers = BTreeMap::new();
    mcp_servers.insert(
        "tauros".to_owned(),
        mcp_host.mcp_server_config(&cfg.node_binary, &cfg.mcp_bridge_script()),
    );
    let mut extra = BTreeMap::new();
    extra.insert("test_driver".to_owned(), json!("direct_mcp"));

    let session_id = adapter
        .start_session(SessionConfig {
            working_dir: Some(repo_root()),
            mcp_servers,
            auth_mode: AuthMode::HostSignIn,
            extra,
            ..SessionConfig::default()
        })
        .expect("session should start");

    // 1. Call domain tool `create_trainer` with 3 Pokemon through the sidecar -> in-process MCP.
    let directive = json!({
        "tool": "create_trainer",
        "arguments": {
            "id": "TRAINER_SIDECAR_DIRECTOR",
            "name": "DIRECTOR",
            "class": "TRAINER_CLASS_LASS",
            "party": [
                { "species": "SPECIES_POOCHYENA", "level": 10 },
                { "species": "SPECIES_ZIGZAGOON", "level": 11 },
                { "species": "SPECIES_WURMPLE", "level": 12 }
            ]
        }
    });

    let turn = adapter
        .send_message(&session_id, UserMessage::text(directive.to_string()))
        .expect("send_message should succeed");
    let turn_id = turn.turn_id().clone();
    let events = turn.into_parts().1.collect_all();

    assert!(matches!(
        events.first(),
        Some(AgentEvent::TurnStarted { turn_id: tid }) if tid == &turn_id
    ));
    assert!(events.iter().any(|e| matches!(
        e,
        AgentEvent::ToolCallStarted { name, .. } if name == "create_trainer"
    )));
    assert!(events.iter().any(|e| matches!(
        e,
        AgentEvent::ToolCallCompleted { name, is_error: false, .. } if name == "create_trainer"
    )));
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::TextDelta { delta, .. } if delta.contains("create_trainer"))));
    assert!(matches!(
        events.last(),
        Some(AgentEvent::TurnCompleted {
            finish_reason: FinishReason::Completed,
            ..
        })
    ));

    // Confirm the mutation landed directly in our in-process ReferenceConnector!
    let snapshot = shared_connector.lock().unwrap().load().unwrap();
    let trainer = snapshot
        .trainers
        .get(&TrainerId::from("TRAINER_SIDECAR_DIRECTOR"))
        .expect("trainer created via sidecar should exist in the in-process connector");
    assert_eq!(trainer.party.members.len(), 3);

    // 2. Verify mid-stream cancellation stops a delayed turn cleanly.
    let cancel_directive = json!({
        "tool": "tauros_status",
        "arguments": {},
        "delay_ms": 5000
    });
    let slow_turn = adapter
        .send_message(&session_id, UserMessage::text(cancel_directive.to_string()))
        .expect("delayed turn should start");
    let slow_turn_id = slow_turn.turn_id().clone();
    let (tid, stream) = slow_turn.into_parts();
    assert_eq!(tid, slow_turn_id);

    // Wait for TurnStarted / Status before cancelling.
    let first = stream
        .recv_timeout(Duration::from_secs(3))
        .expect("TurnStarted should arrive");
    assert!(matches!(first, AgentEvent::TurnStarted { .. }));
    let second = stream
        .recv_timeout(Duration::from_secs(3))
        .expect("Status should arrive");
    assert!(matches!(second, AgentEvent::Status { .. }));

    adapter
        .cancel(&session_id, &slow_turn_id)
        .expect("cancel should succeed");
    let remaining = stream.collect_all();
    assert!(matches!(
        remaining.last(),
        Some(AgentEvent::TurnCompleted {
            finish_reason: FinishReason::Cancelled,
            ..
        })
    ));

    adapter
        .close_session(&session_id)
        .expect("close_session should succeed");
}

#[test]
#[ignore = "requires an active host Claude Code sign-in (`claude auth login`) and TAUROS_LIVE_CLAUDE=1"]
fn live_claude_sdk_sidecar_calls_in_process_mcp() {
    assert_eq!(
        std::env::var("TAUROS_LIVE_CLAUDE").as_deref(),
        Ok("1"),
        "set TAUROS_LIVE_CLAUDE=1 to run the live Claude Agent SDK sidecar test"
    );

    let cfg = sidecar_config();
    let shared_connector: Arc<Mutex<Box<dyn Connector + Send>>> =
        Arc::new(Mutex::new(Box::new(sample_connector())));
    let server = TaurosServer::with_shared_connector(Arc::clone(&shared_connector));
    let mcp_host = InProcessMcpHost::start(server).expect("in-process MCP host should start");

    let mut adapter = ClaudeSidecarAdapter::spawn(cfg.clone()).expect("sidecar should start");
    let mut mcp_servers = BTreeMap::new();
    mcp_servers.insert(
        "tauros".to_owned(),
        mcp_host.mcp_server_config(&cfg.node_binary, &cfg.mcp_bridge_script()),
    );

    let session_id = adapter
        .start_session(SessionConfig {
            working_dir: Some(repo_root()),
            mcp_servers,
            auth_mode: AuthMode::HostSignIn,
            ..SessionConfig::default()
        })
        .expect("session should start");

    let turn = adapter
        .send_message(
            &session_id,
            UserMessage::text(
                "Call the Tauros MCP tool `create_trainer` once to create trainer `TRAINER_CLAUDE_LIVE` \
                 named `LIVE` of class `TRAINER_CLASS_LASS` with a single level 5 `SPECIES_POOCHYENA`, \
                 then reply with the tool result.",
            ),
        )
        .expect("send_message should succeed");

    let events = turn.into_parts().1.collect_all();
    assert!(
        events.iter().any(|e| matches!(
            e,
            AgentEvent::ToolCallCompleted { name, is_error: false, .. }
                if name.ends_with("create_trainer")
        )),
        "expected a successful create_trainer tool call in events: {events:#?}"
    );
    let snapshot = shared_connector.lock().unwrap().load().unwrap();
    assert!(snapshot
        .trainers
        .contains_key(&TrainerId::from("TRAINER_CLAUDE_LIVE")));
    adapter.close_session(&session_id).unwrap();
}
