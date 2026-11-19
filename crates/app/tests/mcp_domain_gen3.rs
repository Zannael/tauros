//! MCP-002 contract tests: every domain tool (`create_trainer`, `place_npc`,
//! `add_scene`, `build`, `run_test`) exercised over MCP JSON-RPC against a real
//! `Gen3Connector` on a `git archive` copy of the pinned expansion, including
//! G3-001 snapshot-guard verification.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{json, Value};
use tauros_connector_gen3::toolchain::Toolchain;
use tauros_connector_gen3::Gen3Connector;
use tauros_mcp::{serve_io, TaurosServer};
use tauros_model::{Connector, Event, FlagId, MapId, ScriptLabel, TrainerId};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "tauros-mcp-gen3-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn pinned_engine() -> Option<PathBuf> {
    let data = std::env::var_os("TAUROS_DATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share/tauros")))?;
    let engines = data.join("engines");
    let entries = fs::read_dir(&engines).ok()?;
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir()
            && p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("pokeemerald-expansion-") && !n.ends_with("-base"))
        {
            return Some(p);
        }
    }
    None
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn gen3_fixture(engine: &Path, tag: &str) -> TempDir {
    let dir = TempDir::new(tag);
    let mut archive = Command::new("git")
        .args(["-C", engine.to_str().unwrap(), "archive", "HEAD"])
        .args(["src", "include", "data", "tools/trainerproc"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let status = Command::new("tar")
        .args(["-x", "-C", dir.path().to_str().unwrap()])
        .stdin(archive.stdout.take().unwrap())
        .status()
        .unwrap();
    assert!(archive.wait().unwrap().success(), "git archive failed");
    assert!(status.success(), "tar extract failed");
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["config", "user.name", "Tauros Test"]);
    git(dir.path(), &["config", "user.email", "test@users.noreply.github.com"]);
    git(dir.path(), &["config", "core.autocrlf", "false"]);
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-q", "-m", "initial fixture"]);
    dir
}

struct McpClient {
    writer: tokio::io::DuplexStream,
    reader: BufReader<tokio::io::DuplexStream>,
    next_id: u64,
    task: tokio::task::JoinHandle<()>,
}

impl McpClient {
    async fn start(server: TaurosServer) -> Self {
        let (c2s, s_read) = tokio::io::duplex(128 * 1024);
        let (s_write, s2c) = tokio::io::duplex(128 * 1024);
        let task = tokio::spawn(async move {
            let _ = serve_io(server, s_read, s_write).await;
        });
        let mut client = Self {
            writer: c2s,
            reader: BufReader::new(s2c),
            next_id: 1,
            task,
        };
        let init = client
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "clientInfo": { "name": "tauros-gen3-contract", "version": "0.1.0" }
                }),
            )
            .await;
        assert_eq!(init["result"]["serverInfo"]["name"], "tauros");
        client.notify("notifications/initialized", json!({})).await;
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
            assert!(n > 0, "server exited while waiting for {method}");
            let v: Value = serde_json::from_str(&line).unwrap();
            if v.get("id").and_then(Value::as_u64) == Some(id) {
                return v;
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
        let _ = self.task.await;
    }
}

fn commit_count(dir: &Path) -> usize {
    git(dir, &["rev-list", "--count", "HEAD"])
        .trim()
        .parse()
        .unwrap()
}

fn head_message(dir: &Path) -> String {
    git(dir, &["log", "-1", "--format=%B"])
}

#[tokio::test]
async fn domain_tools_contract_against_gen3_connector() {
    let Some(engine) = pinned_engine() else {
        eprintln!("skipping Gen 3 MCP contract test: pinned engine not found under $TAUROS_DATA");
        return;
    };
    let dir = gen3_fixture(&engine, "contract");
    let connector = Gen3Connector::open(dir.path(), None).unwrap();
    let mut client = McpClient::start(TaurosServer::with_connector(connector)).await;

    let initial_commits = commit_count(dir.path());

    // 1. create_trainer (creates a brand-new constant via G3-009 + snapshot commit via G3-001)
    let (is_err, body) = client
        .call_tool(
            "create_trainer",
            json!({
                "id": "TRAINER_TAUROS_MCP",
                "name": "Director",
                "class": "Leader",
                "extensions": { "Pic": "Leader Roxanne", "Gender": "Female", "Music": "Female" },
                "party": [
                    { "species": "SPECIES_GEODUDE", "level": 12, "held_item": "ITEM_ORAN_BERRY" }
                ],
                "provenance": "ai-generated"
            }),
        )
        .await;
    assert!(!is_err, "create_trainer failed: {body}");
    assert_eq!(body["ok"], true);
    assert!(body["commit"].as_str().is_some());
    assert_eq!(commit_count(dir.path()), initial_commits + 1);
    let msg = head_message(dir.path());
    assert!(msg.contains("Tauros-Provenance: ai-generated"), "{msg}");
    assert!(msg.contains("Tauros-Snapshot:"), "{msg}");

    // 2. add_scene (assembly scene + atomic flag allocation, snapshot guarded)
    let (is_err, body) = client
        .call_tool(
            "add_scene",
            json!({
                "label": "Route101_EventScript_TaurosMcp",
                "map": "MAP_ROUTE101",
                "language": "assembly",
                "source": "Route101_EventScript_TaurosMcp::\n\tsetflag FLAG_TAUROS_MCP_SEEN\n\trelease\n\tend\n",
                "allocate_flags": ["FLAG_TAUROS_MCP_SEEN"],
                "provenance": "ai-assisted"
            }),
        )
        .await;
    assert!(!is_err, "add_scene failed: {body}");
    assert_eq!(body["ok"], true);
    assert_eq!(commit_count(dir.path()), initial_commits + 2);
    let msg = head_message(dir.path());
    assert!(msg.contains("Tauros-Provenance: ai-assisted"), "{msg}");
    assert!(msg.contains("Tauros-Snapshot:"), "{msg}");

    // 3. place_npc on MAP_ROUTE101 wired to the scene and allocated flag
    let (is_err, body) = client
        .call_tool(
            "place_npc",
            json!({
                "map": "MAP_ROUTE101",
                "x": 12,
                "y": 10,
                "elevation": 3,
                "graphics": "OBJ_EVENT_GFX_BOY_1",
                "movement": "MOVEMENT_TYPE_FACE_DOWN",
                "script": "Route101_EventScript_TaurosMcp",
                "visibility_flag": "FLAG_TAUROS_MCP_SEEN"
            }),
        )
        .await;
    assert!(!is_err, "place_npc failed: {body}");
    assert_eq!(body["ok"], true);
    assert_eq!(commit_count(dir.path()), initial_commits + 3);
    let msg = head_message(dir.path());
    assert!(msg.contains("Tauros-Provenance: ai-generated"), "{msg}");

    // 4. Validation rejections must not write or create commits
    let commits_before_reject = commit_count(dir.path());
    let (is_err, body) = client
        .call_tool(
            "place_npc",
            json!({
                "map": "MAP_DOES_NOT_EXIST",
                "x": 1,
                "y": 1,
                "graphics": "OBJ_EVENT_GFX_BOY_1"
            }),
        )
        .await;
    assert!(is_err, "unknown map must fail: {body}");
    assert_eq!(body["kind"], "validation_rejected");
    assert_eq!(commit_count(dir.path()), commits_before_reject);

    // 5. build and run_test without a configured toolchain return structured errors
    let (is_err, body) = client.call_tool("build", json!({})).await;
    assert!(is_err, "build without toolchain must fail: {body}");
    assert_eq!(body["kind"], "build_failed");

    let (is_err, body) = client
        .call_tool("run_test", json!({ "filter": "Trainer Party" }))
        .await;
    assert!(is_err, "run_test without toolchain must fail: {body}");
    assert_eq!(body["kind"], "build_failed");

    client.close().await;

    // Verify all three mutations persisted cleanly through Gen3Connector::load
    let reloaded = Gen3Connector::open(dir.path(), None).unwrap().load().unwrap();
    assert!(reloaded.trainers.contains_key(&TrainerId::from("TRAINER_TAUROS_MCP")));
    assert!(reloaded.flags.contains_key(&FlagId::from("FLAG_TAUROS_MCP_SEEN")));
    assert!(reloaded
        .scenes
        .contains_key(&ScriptLabel::from("Route101_EventScript_TaurosMcp")));
    let route101 = &reloaded.maps[&MapId::from("MAP_ROUTE101")];
    assert!(route101.events.iter().any(|e| matches!(
        e,
        Event::Npc {
            script: Some(s),
            visibility_flag: Some(f),
            ..
        } if s.0 == "Route101_EventScript_TaurosMcp" && f.0 == "FLAG_TAUROS_MCP_SEEN"
    )));
}

#[tokio::test]
async fn snapshot_guard_aborts_mcp_write_when_commit_fails() {
    let Some(engine) = pinned_engine() else {
        return;
    };
    let dir = gen3_fixture(&engine, "guard-fail");
    let connector = Gen3Connector::open(dir.path(), None).unwrap();
    let fingerprint_before = connector.fingerprint().unwrap();
    let commits_before = commit_count(dir.path());

    // Install a failing pre-commit hook so G3-001 snapshot/commit must abort and restore.
    let hook = dir.path().join(".git/hooks/pre-commit");
    fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let mut client = McpClient::start(TaurosServer::with_connector(connector)).await;
    let (is_err, body) = client
        .call_tool(
            "create_trainer",
            json!({
                "id": "TRAINER_TAUROS_ABORTED",
                "name": "Aborted",
                "class": "Leader",
                "extensions": { "Pic": "Leader Roxanne", "Gender": "Female", "Music": "Female" },
                "party": [{ "species": "SPECIES_GEODUDE", "level": 12 }]
            }),
        )
        .await;
    assert!(is_err, "failing snapshot guard must abort write: {body}");
    assert_eq!(body["kind"], "snapshot_failed");
    client.close().await;

    let check = Gen3Connector::open(dir.path(), None).unwrap();
    assert_eq!(check.fingerprint().unwrap(), fingerprint_before);
    assert_eq!(commit_count(dir.path()), commits_before);
}

#[tokio::test]
async fn domain_build_and_run_test_in_docker() {
    if std::env::var_os("TAUROS_DOCKER_TESTS").is_none() {
        return;
    }
    let engine = pinned_engine().expect("pinned engine required");
    let base = engine
        .parent()
        .unwrap()
        .join(format!("{}-base", engine.file_name().unwrap().to_str().unwrap()));
    assert!(base.is_dir(), "base build required at {}", base.display());

    let dir = TempDir::new("docker-build");
    let status = Command::new("cp")
        .args(["-a", &format!("{}/.", base.display()), dir.path().to_str().unwrap()])
        .status()
        .unwrap();
    assert!(status.success(), "cp -a base build failed");
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["config", "user.name", "Tauros Test"]);
    git(dir.path(), &["config", "user.email", "test@users.noreply.github.com"]);
    git(dir.path(), &["config", "core.autocrlf", "false"]);
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-q", "-m", "base build fixture"]);

    let tc = Toolchain::for_repository(&repo_root()).unwrap();
    let connector = Gen3Connector::open(dir.path(), Some(tc)).unwrap();
    let mut client = McpClient::start(TaurosServer::with_connector(connector)).await;

    let (is_err, body) = client
        .call_tool(
            "create_trainer",
            json!({
                "id": "TRAINER_TAUROS_MCP",
                "name": "Director",
                "class": "Leader",
                "extensions": { "Pic": "Leader Roxanne", "Gender": "Female", "Music": "Female" },
                "party": [{ "species": "SPECIES_GEODUDE", "level": 12 }]
            }),
        )
        .await;
    assert!(!is_err, "create_trainer failed: {body}");

    let (is_err, body) = client.call_tool("build", json!({})).await;
    assert!(!is_err, "build failed: {body}");
    assert_eq!(body["ok"], true);
    assert!(!body["rom_sha256"].as_str().unwrap_or("").is_empty());

    let (is_err, body) = client
        .call_tool("run_test", json!({ "filter": "Trainer Party" }))
        .await;
    assert!(!is_err, "run_test failed: {body}");
    assert_eq!(body["ok"], true);
    assert!(body["cases"].as_array().is_some_and(|c| !c.is_empty()));

    client.close().await;
}
