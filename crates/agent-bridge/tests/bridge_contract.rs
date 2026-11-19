use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::json;
use tauros_agent_bridge::fake::{FakeAdapter, FakeStep, FakeTurnScript};
use tauros_agent_bridge::{
    AgentBridge, AgentEvent, Attachment, AuthMode, ErrorCode, FinishReason, McpServerConfig,
    SessionConfig, SessionId, TokenUsage, ToolCallId, TurnId, UserMessage,
};

#[test]
fn trait_is_object_safe_and_streams_text_and_tool_calls() {
    let mut adapter = FakeAdapter::new();
    adapter.push_script(FakeTurnScript {
        steps: vec![
            FakeStep::Status("Preparing edit".to_owned()),
            FakeStep::Thinking("Need to add a trainer".to_owned()),
            FakeStep::ToolCall {
                call_id: ToolCallId::new("call-1"),
                name: "create_trainer".to_owned(),
                arguments: json!({"id": "TRAINER_DIRECTOR"}),
                is_error: false,
                output: json!({"ok": true}),
            },
            FakeStep::Text("Created ".to_owned()),
            FakeStep::Text("TRAINER_DIRECTOR.".to_owned()),
        ],
        usage: Some(TokenUsage {
            input_tokens: 120,
            output_tokens: 45,
        }),
    });

    let mut bridge: Box<dyn AgentBridge> = Box::new(adapter);
    let caps = bridge.capabilities();
    assert_eq!(caps.provider_name, "fake");
    assert!(caps.supports_streaming);
    assert!(caps.supports_tool_calls);
    assert!(caps.supports_cancel);

    let mut mcp_servers = BTreeMap::new();
    mcp_servers.insert(
        "tauros".to_owned(),
        McpServerConfig::Stdio {
            command: "tauros-mcp".to_owned(),
            args: Vec::new(),
            env: BTreeMap::new(),
        },
    );
    let session_id = bridge
        .start_session(SessionConfig {
            working_dir: Some(PathBuf::from("/tmp/project")),
            mcp_servers,
            auth_mode: AuthMode::None,
            ..SessionConfig::default()
        })
        .expect("start session");

    let turn = bridge
        .send_message(
            &session_id,
            UserMessage::text("Add the director trainer").with_attachment(Attachment::Context {
                category: "selected_map".to_owned(),
                data: json!({"map": "MAP_ROUTE101"}),
            }),
        )
        .expect("send message");
    let turn_id = turn.turn_id().clone();
    let (_, stream) = turn.into_parts();
    let events = stream.collect_all();

    assert_eq!(
        events,
        vec![
            AgentEvent::TurnStarted {
                turn_id: turn_id.clone(),
            },
            AgentEvent::Status {
                turn_id: turn_id.clone(),
                message: "Preparing edit".to_owned(),
            },
            AgentEvent::ThinkingDelta {
                turn_id: turn_id.clone(),
                delta: "Need to add a trainer".to_owned(),
            },
            AgentEvent::ToolCallStarted {
                turn_id: turn_id.clone(),
                call_id: ToolCallId::new("call-1"),
                name: "create_trainer".to_owned(),
                arguments: json!({"id": "TRAINER_DIRECTOR"}),
            },
            AgentEvent::ToolCallCompleted {
                turn_id: turn_id.clone(),
                call_id: ToolCallId::new("call-1"),
                name: "create_trainer".to_owned(),
                is_error: false,
                output: json!({"ok": true}),
            },
            AgentEvent::TextDelta {
                turn_id: turn_id.clone(),
                delta: "Created ".to_owned(),
            },
            AgentEvent::TextDelta {
                turn_id: turn_id.clone(),
                delta: "TRAINER_DIRECTOR.".to_owned(),
            },
            AgentEvent::TurnCompleted {
                turn_id,
                finish_reason: FinishReason::Completed,
                usage: Some(TokenUsage {
                    input_tokens: 120,
                    output_tokens: 45,
                }),
            },
        ]
    );

    bridge.close_session(&session_id).expect("close session");
}

#[test]
fn cancel_stops_in_flight_turn_before_remaining_steps() {
    let mut adapter = FakeAdapter::new();
    adapter.push_script(FakeTurnScript {
        steps: vec![
            FakeStep::Text("First chunk".to_owned()),
            FakeStep::WaitForGate,
            FakeStep::Text("Should not be emitted".to_owned()),
        ],
        usage: Some(TokenUsage {
            input_tokens: 10,
            output_tokens: 20,
        }),
    });

    let session_id = adapter
        .start_session(SessionConfig::default())
        .expect("start session");
    let turn = adapter
        .send_message(&session_id, UserMessage::text("Long running task"))
        .expect("send message");
    let (turn_id, stream) = turn.into_parts();

    assert_eq!(
        stream.recv_timeout(Duration::from_secs(2)).expect("started"),
        AgentEvent::TurnStarted {
            turn_id: turn_id.clone()
        }
    );
    assert_eq!(
        stream
            .recv_timeout(Duration::from_secs(2))
            .expect("first chunk"),
        AgentEvent::TextDelta {
            turn_id: turn_id.clone(),
            delta: "First chunk".to_owned(),
        }
    );

    adapter.cancel(&session_id, &turn_id).expect("cancel turn");

    let remaining = stream.collect_all();
    assert_eq!(
        remaining,
        vec![AgentEvent::TurnCompleted {
            turn_id,
            finish_reason: FinishReason::Cancelled,
            usage: None,
        }]
    );
}

#[test]
fn enforces_auth_and_session_lifecycle_errors() {
    let mut adapter = FakeAdapter::new().with_supported_auth(vec![AuthMode::ApiKey]);

    let auth_err = adapter
        .start_session(SessionConfig {
            auth_mode: AuthMode::HostSignIn,
            ..SessionConfig::default()
        })
        .expect_err("unsupported auth mode must fail");
    assert_eq!(auth_err.code, ErrorCode::AuthRequired);

    let session_id = adapter
        .start_session(SessionConfig {
            auth_mode: AuthMode::ApiKey,
            ..SessionConfig::default()
        })
        .expect("valid auth mode");

    let empty_err = adapter
        .send_message(&session_id, UserMessage::text("   "))
        .expect_err("blank message must fail");
    assert_eq!(empty_err.code, ErrorCode::InvalidConfig);

    let unknown_turn = adapter
        .cancel(&session_id, &TurnId::new("missing-turn"))
        .expect_err("unknown turn must fail");
    assert_eq!(unknown_turn.code, ErrorCode::TurnNotFound);

    adapter.close_session(&session_id).expect("close session");

    let closed_err = adapter
        .send_message(&session_id, UserMessage::text("hello"))
        .expect_err("closed session must fail");
    assert_eq!(closed_err.code, ErrorCode::SessionNotFound);

    let unknown_session = adapter
        .close_session(&SessionId::new("never-existed"))
        .expect_err("unknown session must fail");
    assert_eq!(unknown_session.code, ErrorCode::SessionNotFound);
}

#[test]
fn events_and_configs_roundtrip_as_json() {
    let event = AgentEvent::ToolCallStarted {
        turn_id: TurnId::new("t1"),
        call_id: ToolCallId::new("c1"),
        name: "place_npc".to_owned(),
        arguments: json!({"map": "Route101", "x": 10, "y": 12}),
    };
    let encoded = serde_json::to_string(&event).expect("serialize event");
    let decoded: AgentEvent = serde_json::from_str(&encoded).expect("deserialize event");
    assert_eq!(decoded, event);

    let mut cfg = SessionConfig {
        auth_mode: AuthMode::ApiKey,
        model: Some("test-model".to_owned()),
        ..SessionConfig::default()
    };
    cfg.mcp_servers.insert(
        "tauros".to_owned(),
        McpServerConfig::StreamableHttp {
            url: "http://127.0.0.1:8765/mcp".to_owned(),
        },
    );
    let cfg_json = serde_json::to_string(&cfg).expect("serialize config");
    let cfg_back: SessionConfig = serde_json::from_str(&cfg_json).expect("deserialize config");
    assert_eq!(cfg_back, cfg);
}
