//! MCP-004 Phase 3 end-to-end director's scene test:
//! drives the full director's scene strictly through MCP JSON-RPC tools on a
//! real pokeemerald-expansion 1.16.4 project:
//!   1. `create_trainer` (3 Pokemon, Lv 30)
//!   2. `add_scene` (Poryscript with `trainerbattle_earlyrival` no-whiteout + atomic flag allocation)
//!   3. `place_npc` (Route 101 at (10, 9) facing down)
//!   4. `build` (hardened Docker incremental build + automatic emulator reload)
//!   5. `emu_boot` (`boot_to` MAP_ROUTE101 at (10, 10)) + `emu_save_state`
//!   6. `run_verification` (VER-001 inert spec proving lost battle without whiteout:
//!      `gBattleOutcome == B_OUTCOME_LOST`, `gSaveBlock1Ptr` deref map == `MAP_ROUTE101`,
//!      allocated scene flag set, counterfactuals `shift`/`wrong_value`/`cold_boot` all fail -> `PROVEN`)
//!   7. `emu_screenshot` (valid 240x160 PNG)
//!
//! Run with:
//!   TAUROS_DOCKER_TESTS=1 cargo test -p tauros-app --test phase3_director_scene -- --nocapture
//!
//! Optional: set `TAUROS_DIRECTOR_PROJECT_DIR=/path/to/dir` to keep the built
//! project, screenshot and verification report on disk for desktop preview.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{json, Value};
use tauros_app::mcp_host::build_project_mcp_server;
use tauros_connector_gen3::toolchain::Toolchain;
use tauros_connector_gen3::Gen3Connector;
use tauros_mcp::{serve_io, TaurosServer};
use tauros_model::{Connector, FlagId};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

struct WorkspaceDir {
    path: PathBuf,
    cleanup: bool,
}

impl WorkspaceDir {
    fn create(tag: &str) -> Self {
        if let Ok(custom) = std::env::var("TAUROS_DIRECTOR_PROJECT_DIR") {
            let dir = PathBuf::from(custom);
            if std::env::var("TAUROS_REUSE_BUILT_DIRECTOR").as_deref() != Ok("1") {
                let _ = fs::remove_dir_all(&dir);
                fs::create_dir_all(&dir).unwrap();
            }
            return Self {
                path: dir,
                cleanup: false,
            };
        }
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "tauros-mcp004-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self {
            path: dir,
            cleanup: true,
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for WorkspaceDir {
    fn drop(&mut self) {
        if self.cleanup {
            let _ = fs::remove_dir_all(&self.path);
        }
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

fn tauros_data_dir() -> Option<PathBuf> {
    std::env::var_os("TAUROS_DATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share/tauros")))
}

fn pinned_base_build() -> Option<PathBuf> {
    let engines = tauros_data_dir()?.join("engines");
    for entry in fs::read_dir(&engines).ok()?.flatten() {
        let p = entry.path();
        if p.is_dir()
            && p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("pokeemerald-expansion-") && n.ends_with("-base"))
            && p.join("pokeemerald.gba").is_file()
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

fn seed_from_base_build(base_build: &Path, dest: &Path) {
    // Copy with timestamps preserved (`cp -a`) so incremental `make` rebuilds only changed files.
    let status = Command::new("cp")
        .args(["-a", &format!("{}/.", base_build.display()), &dest.display().to_string()])
        .status()
        .unwrap();
    assert!(status.success(), "cp -a from base build failed");

    // Enable pokeemerald-expansion's native no-whiteout flag (`B_FLAG_NO_WHITEOUT`)
    // in project config using built-in temporary flag `FLAG_TEMP_1`.
    // (In expansion 1.16.4, `trainerbattle_earlyrival` has a bitwise `1 & 3` bug in
    // src/battle_setup.c:1328 that triggers FRLG's Oak tutorial handler on Emerald.)
    let battle_cfg = dest.join("include/config/battle.h");
    let cfg_text = fs::read_to_string(&battle_cfg).unwrap();
    let updated_cfg = cfg_text.replacen(
        "#define B_FLAG_NO_WHITEOUT          0",
        "#define B_FLAG_NO_WHITEOUT          FLAG_TEMP_1",
        1,
    );
    assert_ne!(cfg_text, updated_cfg, "failed to configure B_FLAG_NO_WHITEOUT in include/config/battle.h");
    fs::write(&battle_cfg, updated_cfg).unwrap();

    // Initialize a clean local git repo for G3-001 snapshot commits (build outputs are gitignored).
    let _ = fs::remove_dir_all(dest.join(".git"));
    git(dest, &["init", "-q"]);
    git(dest, &["config", "user.name", "Tauros Director Test"]);
    git(dest, &["config", "user.email", "test@users.noreply.github.com"]);
    git(dest, &["config", "core.autocrlf", "false"]);
    git(dest, &["add", "-A"]);
    git(dest, &["commit", "-q", "-m", "initial base build fixture"]);
}

struct McpClient {
    writer: tokio::io::DuplexStream,
    reader: BufReader<tokio::io::DuplexStream>,
    next_id: u64,
    task: tokio::task::JoinHandle<()>,
}

impl McpClient {
    async fn start(server: TaurosServer) -> Self {
        let (c2s, s_read) = tokio::io::duplex(512 * 1024);
        let (s_write, s2c) = tokio::io::duplex(512 * 1024);
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
                    "clientInfo": { "name": "tauros-phase3-director", "version": "0.1.0" }
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

#[tokio::test]
async fn phase3_director_scene_end_to_end_via_mcp() {
    if std::env::var("TAUROS_DOCKER_TESTS").as_deref() != Ok("1") {
        eprintln!("skipping phase3_director_scene (set TAUROS_DOCKER_TESTS=1 to run)");
        return;
    }

    let base_build = pinned_base_build()
        .expect("pinned base build missing under $TAUROS_DATA/engines — run setup_toolchain.py");
    let base_rom = fs::read(base_build.join("pokeemerald.gba")).unwrap();
    let toolchain = Toolchain::for_repository(&repo_root()).unwrap();

    let reuse = std::env::var("TAUROS_REUSE_BUILT_DIRECTOR").as_deref() == Ok("1");
    let project = WorkspaceDir::create("director");
    if !reuse {
        seed_from_base_build(&base_build, project.path());
    }

    let server = build_project_mcp_server(project.path(), Some(toolchain), Some(base_rom))
        .expect("build_project_mcp_server should succeed");
    let mut mcp = McpClient::start(server).await;

    if !reuse {
        // 1. Create the Director trainer with 3 strong Pokemon via MCP.
        let (err, res) = mcp
            .call_tool(
                "create_trainer",
                json!({
                    "id": "TRAINER_TAUROS_DIRECTOR",
                    "name": "Director",
                    "class": "Leader",
                    "extensions": { "Pic": "Leader Roxanne", "Gender": "Female", "Music": "Female" },
                    "party": [
                        { "species": "SPECIES_POOCHYENA", "level": 30, "moves": ["MOVE_CRUNCH"] },
                        { "species": "SPECIES_ZIGZAGOON", "level": 30, "moves": ["MOVE_HEADBUTT"] },
                        { "species": "SPECIES_MIGHTYENA", "level": 30, "moves": ["MOVE_CRUNCH"] }
                    ],
                    "summary": "Create 3-Pokemon director trainer for Phase 3 scene"
                }),
            )
            .await;
        assert!(!err, "create_trainer failed: {res:#}");
        eprintln!("[MCP create_trainer] {res}");

        // 2. Add the Poryscript director's scene with no-whiteout battle (`trainerbattle_earlyrival`)
        //    and atomic allocation of `FLAG_TAUROS_DIRECTOR_SEEN`.
        let poryscript = r#"script PetalburgCity_EventScript_TaurosDirector {
    lock
    faceplayer
    if (flag(FLAG_TAUROS_DIRECTOR_SEEN)) {
        msgbox(PetalburgCity_Text_DirectorAfter, MSGBOX_DEFAULT)
        release
        end
    }
    if (!flag(FLAG_SYS_POKEMON_GET)) {
        givemon(SPECIES_WURMPLE, 2)
        setflag(FLAG_SYS_POKEMON_GET)
    }
    msgbox(PetalburgCity_Text_DirectorIntro, MSGBOX_DEFAULT)
    setflag(FLAG_TEMP_1)
    trainerbattle_no_intro(TRAINER_TAUROS_DIRECTOR, PetalburgCity_Text_DirectorDefeat)
    clearflag(FLAG_TEMP_1)
    special(HealPlayerParty)
    setflag(FLAG_TAUROS_DIRECTOR_SEEN)
    msgbox(PetalburgCity_Text_DirectorAfter, MSGBOX_DEFAULT)
    release
    end
}

text PetalburgCity_Text_DirectorIntro {
    "Let us test your resolve!"
}

text PetalburgCity_Text_DirectorDefeat {
    "Impressive!"
}

text PetalburgCity_Text_DirectorAfter {
    "No whiteout! You stayed in Petalburg!"
}
"#;

        let (err, res) = mcp
            .call_tool(
                "add_scene",
                json!({
                    "label": "PetalburgCity_EventScript_TaurosDirector",
                    "map": "MAP_PETALBURG_CITY",
                    "language": "poryscript",
                    "source": poryscript,
                    "allocate_flags": ["FLAG_TAUROS_DIRECTOR_SEEN"],
                    "summary": "Add Poryscript director scene with no-whiteout battle"
                }),
            )
            .await;
        assert!(!err, "add_scene failed: {res:#}");
        eprintln!("[MCP add_scene] {res}");

        // 3. Place the Director NPC directly in front of the player at (15, 11) facing up
        //    (player spawns at (15, 10) facing down on MAP_PETALBURG_CITY).
        let (err, res) = mcp
            .call_tool(
                "place_npc",
                json!({
                    "map": "MAP_PETALBURG_CITY",
                    "x": 15,
                    "y": 11,
                    "elevation": 3,
                    "graphics": "OBJ_EVENT_GFX_MAN_1",
                    "movement": "MOVEMENT_TYPE_FACE_UP",
                    "script": "PetalburgCity_EventScript_TaurosDirector",
                    "summary": "Place Director NPC in Petalburg City at (15, 11)"
                }),
            )
            .await;
        assert!(!err, "place_npc failed: {res:#}");
        eprintln!("[MCP place_npc] {res}");

        // 4. Build the project via MCP (hardened Docker incremental build + auto-reload into EmuSession).
        let (err, res) = mcp.call_tool("build", json!({})).await;
        assert!(!err, "build failed: {res:#}");
        eprintln!("[MCP build] ok={}", res["ok"]);
    }

    // Resolve the allocated flag's numeric ID from the project snapshot for RAM verification.
    let allocated_flag_id = {
        let conn = Gen3Connector::open(project.path(), None).unwrap();
        let snap = conn.load().unwrap();
        snap.flags
            .get(&FlagId::from("FLAG_TAUROS_DIRECTOR_SEEN"))
            .and_then(|f| f.value)
            .expect("FLAG_TAUROS_DIRECTOR_SEEN should have an allocated numeric value")
    };
    eprintln!("[Allocated flag] FLAG_TAUROS_DIRECTOR_SEEN = {allocated_flag_id} (0x{allocated_flag_id:x})");

    // 5. Boot headless mGBA directly to MAP_PETALBURG_CITY (15, 10) facing the Director NPC.
    let (err, boot_res) = mcp
        .call_tool(
            "emu_boot",
            json!({
                "kind": "boot_to",
                "map": "MAP_PETALBURG_CITY",
                "x": 15,
                "y": 10
            }),
        )
        .await;
    assert!(!err, "emu_boot failed: {boot_res:#}");
    let rom_sha256 = boot_res["rom_sha256"]
        .as_str()
        .expect("emu_boot should report rom_sha256")
        .to_owned();
    eprintln!("[MCP emu_boot] frames={} rom_sha256={rom_sha256}", boot_res["frames"]);

    // Save a ROM-bound savestate right before talking to the Director.
    let (err, ss_res) = mcp
        .call_tool("emu_save_state", json!({ "name": "before_director" }))
        .await;
    assert!(!err, "emu_save_state failed: {ss_res:#}");

    let before_shot = project.path().join("build/tauros/director-before.png");
    let _ = fs::create_dir_all(before_shot.parent().unwrap());
    let _ = mcp
        .call_tool(
            "emu_screenshot",
            json!({
                "save_path": before_shot.display().to_string(),
                "include_base64": false
            }),
        )
        .await;

    // 6. Run VER-001 inert verification spec proving:
    //    - Player loses the 3-Pokemon trainer battle (`gBattleOutcome == B_OUTCOME_LOST`)
    //    - No whiteout: player remains on `MAP_PETALBURG_CITY` (`gSaveBlock1Ptr` deref + 4)
    //    - Scene completion flag `FLAG_TAUROS_DIRECTOR_SEEN` is set (`gSaveBlock1Ptr` deref + 0x1270)
    //    - Counterfactuals (`shift`, `wrong_value`, `cold_boot`) all fail -> verdict `PROVEN`.
    let spec = json!({
        "schema": 1,
        "name": "director-loss-no-whiteout",
        "description": "Talking to the Petalburg City Director starts a 3-Pokemon trainer battle; losing sets gBattleOutcome=B_OUTCOME_LOST without whiteout, keeping the player on MAP_PETALBURG_CITY with FLAG_TAUROS_DIRECTOR_SEEN set.",
        "start": {
            "kind": "savestate",
            "name": "before_director",
            "rom_sha256": rom_sha256
        },
        "timeline": [
            { "name": "face_director", "frames": 6, "keys": ["DOWN"] },
            { "name": "pause", "frames": 10, "keys": [] },
            { "name": "talk_and_lose", "frames": 3000, "policy": "lose-fast" },
            { "name": "settle", "frames": 120, "keys": [] }
        ],
        "probes": [
            {
                "id": "outcome",
                "why": "Battle outcome must be B_OUTCOME_LOST after losing to the Director.",
                "symbol": "gBattleOutcome",
                "type": "u8",
                "sample": { "at_step_end": ["settle"] }
            },
            {
                "id": "map",
                "why": "SaveBlock1 location (dereferenced) must remain MAP_PETALBURG_CITY, proving no whiteout.",
                "symbol": "gSaveBlock1Ptr",
                "deref": true,
                "offset": 4,
                "type": "u16",
                "sample": { "at_step_end": ["settle"] }
            },
            {
                "id": "pos",
                "why": "SaveBlock1 pos (dereferenced, offset 0, packed u16 x=15 | u16 y=10 << 16 = 655375) discriminates against zero/whiteout.",
                "symbol": "gSaveBlock1Ptr",
                "deref": true,
                "offset": 0,
                "type": "u32",
                "sample": { "at_step_end": ["settle"] }
            },
            {
                "id": "seen_flag",
                "why": "Allocated FLAG_TAUROS_DIRECTOR_SEEN bit in SaveBlock1 flags must be set after the scene.",
                "symbol": "gSaveBlock1Ptr",
                "deref": true,
                "offset": 4720,
                "type": "flag_bit",
                "flag_bit": allocated_flag_id,
                "sample": { "at_step_end": ["settle"] }
            }
        ],
        "assert": [
            {
                "check": "compare",
                "probe": "outcome",
                "at": "settle",
                "op": "==",
                "value": "B_OUTCOME_LOST"
            },
            {
                "check": "compare",
                "probe": "map",
                "at": "settle",
                "op": "==",
                "value": "MAP_PETALBURG_CITY"
            },
            {
                "check": "compare",
                "probe": "pos",
                "at": "settle",
                "op": "==",
                "value": 655375
            },
            {
                "check": "compare",
                "probe": "seen_flag",
                "at": "settle",
                "op": "==",
                "value": 1
            }
        ],
        "counterfactuals": ["shift", "wrong_value", "cold_boot"]
    });

    let (err, ver_res) = mcp.call_tool("run_verification", spec).await;
    assert!(!err, "run_verification returned tool error: {ver_res:#}");
    eprintln!(
        "[MCP run_verification] {}",
        serde_json::to_string_pretty(&ver_res).unwrap()
    );
    // 7. Capture a screenshot of the post-battle Petalburg City state.
    let screenshot_path = project.path().join("build/tauros/director-scene-post-battle.png");
    let _ = fs::create_dir_all(screenshot_path.parent().unwrap());
    let (err, shot_res) = mcp
        .call_tool(
            "emu_screenshot",
            json!({
                "save_path": screenshot_path.display().to_string(),
                "include_base64": false
            }),
        )
        .await;
    assert!(!err, "emu_screenshot failed: {shot_res:#}");
    assert_eq!(shot_res["width"], 240);
    assert_eq!(shot_res["height"], 160);
    assert!(screenshot_path.is_file());

    // Record the verification report beside the project for inspection if kept.
    let report_path = project.path().join("build/tauros/director-verification-report.json");
    fs::write(&report_path, serde_json::to_string_pretty(&ver_res).unwrap()).unwrap();

    assert_eq!(
        ver_res["verdict"], "PROVEN",
        "expected PROVEN verdict, got: {ver_res:#}"
    );

    mcp.close().await;
}
