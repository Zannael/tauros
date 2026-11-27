//! APP-003: the app's project session builds and boots at a chosen map, and the
//! agent's MCP emulator tools drive that same session (what the window shows).
//!
//! Run with (Docker and the pinned base build from the setup script):
//!   TAUROS_DOCKER_TESTS=1 cargo test -p tauros-app --test build_and_play -- --nocapture

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};
use tauros_app::mcp_host::ProjectSession;
use tauros_connector_gen3::toolchain::Toolchain;
use tauros_mcp::{serve_io, TaurosServer};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn pinned_base_build() -> Option<PathBuf> {
    let data = std::env::var_os("TAUROS_DATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share/tauros")))?;
    fs::read_dir(data.join("engines")).ok()?.flatten().map(|e| e.path()).find(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("pokeemerald-expansion-") && n.ends_with("-base"))
            && p.join("pokeemerald.gba").is_file()
    })
}

fn run(dir: &Path, program: &str, args: &[&str]) {
    let status = Command::new(program).current_dir(dir).args(args).status().unwrap();
    assert!(status.success(), "{program} {args:?} failed");
}

/// A git-tracked copy of the base build, timestamps kept so `make` stays incremental.
fn seed_project(base: &Path, dest: &Path) {
    run(Path::new("."), "cp", &["-a", &format!("{}/.", base.display()), &dest.display().to_string()]);
    let _ = fs::remove_dir_all(dest.join(".git"));
    run(dest, "git", &["init", "-q"]);
    run(dest, "git", &["config", "user.name", "Tauros Build And Play Test"]);
    run(dest, "git", &["config", "user.email", "test@users.noreply.github.com"]);
    run(dest, "git", &["add", "-A"]);
    run(dest, "git", &["commit", "-q", "-m", "base build fixture"]);
}

struct Mcp {
    writer: DuplexStream,
    reader: BufReader<DuplexStream>,
    next_id: u64,
}

impl Mcp {
    async fn start(server: TaurosServer) -> Self {
        let (writer, server_read) = tokio::io::duplex(512 * 1024);
        let (server_write, reader) = tokio::io::duplex(512 * 1024);
        tokio::spawn(async move {
            let _ = serve_io(server, server_read, server_write).await;
        });
        let mut mcp = Self { writer, reader: BufReader::new(reader), next_id: 1 };
        let params = json!({ "protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": { "name": "build-and-play", "version": "0" } });
        mcp.request("initialize", params).await;
        mcp.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized", "params": {} })).await;
        mcp
    }

    async fn send(&mut self, message: Value) {
        self.writer.write_all(format!("{message}\n").as_bytes()).await.unwrap();
        self.writer.flush().await.unwrap();
    }

    async fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).await;
        loop {
            let mut line = String::new();
            assert!(self.reader.read_line(&mut line).await.unwrap() > 0, "server exited during {method}");
            let reply: Value = serde_json::from_str(&line).unwrap();
            if reply["id"].as_u64() == Some(id) {
                return reply;
            }
        }
    }

    /// Calls a tool that must fail and returns its error text.
    async fn call_failing(&mut self, name: &str, arguments: Value) -> String {
        let reply = self.request("tools/call", json!({ "name": name, "arguments": arguments })).await;
        assert_eq!(reply["result"]["isError"], json!(true), "{name} should have failed: {reply}");
        reply["result"]["content"][0]["text"].as_str().unwrap_or_default().to_owned()
    }

    /// Calls a tool and returns its JSON result, failing on a tool error.
    async fn call(&mut self, name: &str, arguments: Value) -> Value {
        let reply = self.request("tools/call", json!({ "name": name, "arguments": arguments })).await;
        let text = reply["result"]["content"][0]["text"].as_str().unwrap_or_default().to_owned();
        assert_ne!(reply["result"]["isError"], json!(true), "{name} failed: {text}");
        serde_json::from_str(&text).unwrap()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn build_and_boot_shares_the_emulator_with_the_agent() {
    if std::env::var("TAUROS_DOCKER_TESTS").as_deref() != Ok("1") {
        eprintln!("skipping build_and_play (set TAUROS_DOCKER_TESTS=1 to run)");
        return;
    }
    let base = pinned_base_build().expect("pinned base build missing under $TAUROS_DATA/engines");
    let base_rom = fs::read(base.join("pokeemerald.gba")).unwrap();
    let dir = std::env::temp_dir().join(format!("tauros-app003-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    seed_project(&base, &dir);

    let toolchain = Toolchain::for_repository(&repo_root()).unwrap();
    let project = ProjectSession::open(&dir, Some(toolchain), None).unwrap();
    assert!(project.maps().unwrap().iter().any(|m| m == "MAP_PETALBURG_CITY"));
    assert!(project.emulator().lock().unwrap().has_rom(), "the built base ROM is loaded on open");

    // The agent changes the project through its tools on the same session.
    let mut agent = Mcp::start(project.mcp_server()).await;
    let party = json!([{ "species": "SPECIES_POOCHYENA", "level": 5 }]);
    let created = agent
        .call("create_trainer", json!({
            "id": "TRAINER_APP003", "name": "Play", "class": "Leader", "party": party,
            "extensions": { "Pic": "Leader Roxanne", "Gender": "Female", "Music": "Female" }
        }))
        .await;
    assert_eq!(created["ok"], json!(true), "{created}");

    // One action builds and boots at the chosen point.
    let report = project.build_and_boot("MAP_PETALBURG_CITY", 15, 10, &[]).unwrap();
    assert!(report.boot_frames > 0);
    let built = fs::read(dir.join("pokeemerald.gba")).unwrap();
    assert_ne!(built, base_rom, "the build includes the agent's trainer");
    assert_eq!(project.emulator().lock().unwrap().rom_sha256(), Some(report.rom_sha256.as_str()));

    // The agent's own boot tool follows the same rules: Petalburg's gym wall is
    // refused with the nearest walkable tile, and a Kanto map is refused outright.
    let wall = agent.call_failing("emu_boot", json!({ "kind": "boot_to", "map": "MAP_PETALBURG_CITY", "x": 0, "y": 0 })).await;
    assert!(wall.contains("the tile is") && wall.contains("nearest walkable tile is"), "{wall}");
    let kanto = agent.call_failing("emu_boot", json!({ "kind": "boot_to", "map": "MAP_CELADON_CITY", "x": 24, "y": 20 })).await;
    assert!(kanto.contains("REGION_KANTO"), "{kanto}");

    // Frames the agent runs are frames of the window's session.
    let before = project.emulator().lock().unwrap().emulator().frame_count();
    let pressed = agent.call("emu_press_input", json!({ "buttons": [], "frames": 30 })).await;
    let after = project.emulator().lock().unwrap().emulator().frame_count();
    assert_eq!(after, before + 30, "{pressed}");
    assert_eq!(pressed["frame_count"], json!(after));

    drop(agent);
    let _ = fs::remove_dir_all(&dir);
}
