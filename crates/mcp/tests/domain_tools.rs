//! Protocol-level MCP tests for the MCP-002 domain tools over an in-memory
//! connector (`tokio::io::duplex`, real rmcp JSON-RPC framing).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{json, Value};
use tauros_mcp::{serve_io, TaurosServer};
use tauros_model::memory::MemoryConnector;
use tauros_model::{
    Applied, BuildOutput, Capabilities, ChangeSet, Connector, ConnectorError, GameFamily, GameInfo,
    LayoutId, Map, MapGeometry, MapId, ProjectSnapshot, ScriptLabel, Species, SpeciesId,
    TestCaseOutput, TestOutput, Violation,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

struct TestConnector {
    inner: MemoryConnector,
    fail_next_build: bool,
}

impl TestConnector {
    fn sample() -> Self {
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
        snapshot.species.insert(
            SpeciesId::from("SPECIES_GEODUDE"),
            Species {
                id: SpeciesId::from("SPECIES_GEODUDE"),
                name: "Geodude".to_owned(),
                national_dex: Some(74),
            },
        );
        snapshot.maps.insert(
            MapId::from("MAP_ROUTE101"),
            Map {
                id: MapId::from("MAP_ROUTE101"),
                name: "Route101".to_owned(),
                geometry: MapGeometry::Grid {
                    layout: LayoutId::from("LAYOUT_ROUTE101"),
                    width: 20,
                    height: 20,
                },
                events: Vec::new(),
                connections: Vec::new(),
                properties: BTreeMap::new(),
            },
        );
        Self {
            inner: MemoryConnector::new(snapshot),
            fail_next_build: false,
        }
    }
}

impl Connector for TestConnector {
    fn info(&self) -> GameInfo {
        self.inner.info()
    }

    fn load(&self) -> Result<ProjectSnapshot, ConnectorError> {
        self.inner.load()
    }

    fn validate(&self, changes: &ChangeSet) -> Vec<Violation> {
        self.inner.validate(changes)
    }

    fn apply(&mut self, changes: &ChangeSet) -> Result<Applied, ConnectorError> {
        let mut applied = self.inner.apply(changes)?;
        applied.commit = Some(format!("mem-commit-{}", self.inner.history.len()));
        applied.touched = vec!["virtual".to_owned()];
        Ok(applied)
    }

    fn build(&mut self) -> Result<BuildOutput, ConnectorError> {
        if self.fail_next_build {
            return Err(ConnectorError::Build(vec![tauros_model::Diagnostic {
                file: Some("src/main.c".to_owned()),
                line: Some(42),
                message: "error: simulated build error".to_owned(),
                severity: tauros_model::Severity::Error,
            }]));
        }
        Ok(BuildOutput {
            rom: PathBuf::from("pokeemerald.gba"),
            symbols: Some(PathBuf::from("pokeemerald.sym")),
            rom_sha256: "deadbeef".to_owned(),
            diagnostics: Vec::new(),
        })
    }

    fn run_tests(&mut self, filter: Option<&str>) -> Result<TestOutput, ConnectorError> {
        let name = filter.unwrap_or("All Tests").to_owned();
        let mut totals = BTreeMap::new();
        totals.insert("PASSED".to_owned(), 1);
        totals.insert("TOTAL".to_owned(), 1);
        Ok(TestOutput {
            success: true,
            cases: vec![TestCaseOutput {
                name,
                result: "PASS".to_owned(),
                passed: true,
                location: None,
                output: String::new(),
            }],
            totals,
            diagnostics: Vec::new(),
        })
    }

    fn fingerprint(&self) -> Result<String, ConnectorError> {
        self.inner.fingerprint()
    }
}

struct McpTestClient {
    writer: tokio::io::DuplexStream,
    reader: BufReader<tokio::io::DuplexStream>,
    next_id: u64,
    server_task: tokio::task::JoinHandle<()>,
}

impl McpTestClient {
    async fn start(server: TaurosServer) -> Self {
        let (client_to_server, server_read) = tokio::io::duplex(64 * 1024);
        let (server_write, server_to_client) = tokio::io::duplex(64 * 1024);
        let server_task = tokio::spawn(async move {
            let _ = serve_io(server, server_read, server_write).await;
        });
        let mut client = Self {
            writer: client_to_server,
            reader: BufReader::new(server_to_client),
            next_id: 1,
            server_task,
        };
        let init = client
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "clientInfo": { "name": "tauros-test", "version": "0.1.0" }
                }),
            )
            .await;
        assert_eq!(init["result"]["serverInfo"]["name"], "tauros");
        client
            .notify("notifications/initialized", json!({}))
            .await;
        client
    }

    async fn notify(&mut self, method: &str, params: Value) {
        let msg = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        self.writer
            .write_all(format!("{msg}\n").as_bytes())
            .await
            .unwrap();
        self.writer.flush().await.unwrap();
    }

    async fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        self.writer
            .write_all(format!("{msg}\n").as_bytes())
            .await
            .unwrap();
        self.writer.flush().await.unwrap();
        loop {
            let mut line = String::new();
            let n = self.reader.read_line(&mut line).await.unwrap();
            assert!(n > 0, "server closed before responding to {method}");
            let value: Value = serde_json::from_str(&line).unwrap();
            if value.get("id").and_then(Value::as_u64) == Some(id) {
                return value;
            }
        }
    }

    async fn call_tool(&mut self, name: &str, arguments: Value) -> (bool, Value) {
        let resp = self
            .request("tools/call", json!({ "name": name, "arguments": arguments }))
            .await;
        let result = &resp["result"];
        let is_error = result.get("isError").and_then(Value::as_bool).unwrap_or(false);
        let text = result["content"][0]["text"].as_str().unwrap_or("");
        let parsed = serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_owned()));
        (is_error, parsed)
    }

    async fn close(self) {
        drop(self.writer);
        let _ = self.server_task.await;
    }
}

#[tokio::test]
async fn lists_and_executes_all_domain_tools_over_mcp() {
    let server = TaurosServer::with_connector(TestConnector::sample());
    let mut client = McpTestClient::start(server).await;

    let listed = client.request("tools/list", json!({})).await;
    let names: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for expected in [
        "tauros_status",
        "create_trainer",
        "place_npc",
        "add_scene",
        "build",
        "run_test",
    ] {
        assert!(names.contains(&expected), "missing tool {expected} in {names:?}");
    }

    // 1. create_trainer
    let (is_err, body) = client
        .call_tool(
            "create_trainer",
            json!({
                "id": "TRAINER_DIRECTOR",
                "name": "Director",
                "class": "Leader",
                "party": [{ "species": "SPECIES_GEODUDE", "level": 12 }]
            }),
        )
        .await;
    assert!(!is_err, "create_trainer failed: {body}");
    assert_eq!(body["ok"], true);
    assert_eq!(body["commit"], "mem-commit-1");

    // 2. add_scene with atomic flag allocation
    let (is_err, body) = client
        .call_tool(
            "add_scene",
            json!({
                "label": "Route101_EventScript_Director",
                "map": "MAP_ROUTE101",
                "language": "poryscript",
                "source": "script Route101_EventScript_Director { release end }",
                "allocate_flags": ["FLAG_DIRECTOR_SEEN"]
            }),
        )
        .await;
    assert!(!is_err, "add_scene failed: {body}");
    assert_eq!(body["ok"], true);
    assert_eq!(body["commit"], "mem-commit-2");

    // 3. place_npc wired to the created scene and flag
    let (is_err, body) = client
        .call_tool(
            "place_npc",
            json!({
                "map": "MAP_ROUTE101",
                "x": 10,
                "y": 10,
                "graphics": "OBJ_EVENT_GFX_BOY_1",
                "movement": "MOVEMENT_TYPE_FACE_DOWN",
                "script": "Route101_EventScript_Director",
                "visibility_flag": "FLAG_DIRECTOR_SEEN"
            }),
        )
        .await;
    assert!(!is_err, "place_npc failed: {body}");
    assert_eq!(body["ok"], true);
    assert_eq!(body["commit"], "mem-commit-3");

    // 4. Validation error: empty party must be rejected cleanly
    let (is_err, body) = client
        .call_tool(
            "create_trainer",
            json!({
                "id": "TRAINER_BAD",
                "name": "Bad",
                "party": []
            }),
        )
        .await;
    assert!(is_err, "empty party must fail: {body}");
    assert_eq!(body["kind"], "validation_rejected");

    // 5. build
    let (is_err, body) = client.call_tool("build", json!({})).await;
    assert!(!is_err, "build failed: {body}");
    assert_eq!(body["rom_sha256"], "deadbeef");

    // 6. run_test
    let (is_err, body) = client
        .call_tool("run_test", json!({ "filter": "Trainer Party" }))
        .await;
    assert!(!is_err, "run_test failed: {body}");
    assert_eq!(body["ok"], true);
    assert_eq!(body["cases"][0]["name"], "Trainer Party");
    assert_eq!(body["cases"][0]["passed"], true);

    // Reference check that ScriptLabel type compiles and matches
    let _ = ScriptLabel::from("Route101_EventScript_Director");

    client.close().await;
}

#[tokio::test]
async fn reports_no_connector_when_unattached() {
    let mut client = McpTestClient::start(TaurosServer::default()).await;
    let (is_err, body) = client.call_tool("build", json!({})).await;
    assert!(is_err);
    assert_eq!(body["kind"], "no_connector");
    client.close().await;
}
