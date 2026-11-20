//! MCP-003 tests: emulator tools (`emu_boot`, `emu_press_input`, `emu_screenshot`,
//! `emu_save_state`, `emu_load_state`, `emu_assert_ram`, `run_verification`) over
//! real MCP JSON-RPC (`tokio::io::duplex`), including VER-001 counterfactuals
//! and a real libmgba test when the base ROM is present under `$TAUROS_DATA`.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use base64::Engine as _;
use serde_json::{json, Value};
use tauros_emu::{Button, Buttons, EmuError, Emulator, Frame, MemoryRegion, Mgba};
use tauros_mcp::emulator::{BootHook, EmuSession, SymbolTable};
use tauros_mcp::{serve_io, TaurosServer};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const EWRAM_START: u32 = 0x0200_0000;
const IWRAM_START: u32 = 0x0300_0000;
const SYM_BATTLE_OUTCOME: u32 = EWRAM_START + 0x0040;
const SYM_MAIN_FRAME: u32 = IWRAM_START + 0x0020;
const SYM_SAVEBLOCK1_PTR: u32 = IWRAM_START + 0x0010;
const SB1_BASE_INIT: u32 = EWRAM_START + 0x0100;
const SB1_OFFSET_MONEY: u32 = 0x0490;
const SB1_OFFSET_FLAGS: u32 = 0x0500;
const B_OUTCOME_LOST: u8 = 2;

/// Deterministic in-memory emulator simulating GBA EWRAM/IWRAM, a moving
/// `gSaveBlock1Ptr`, `gBattleOutcome`, `money`, and a `flags` bitfield.
struct MockEmulator {
    rom: Option<Vec<u8>>,
    frame_count: u32,
    buttons: Buttons,
    ewram: Vec<u8>,
    iwram: Vec<u8>,
}

impl MockEmulator {
    fn new() -> Self {
        Self {
            rom: None,
            frame_count: 0,
            buttons: Buttons::NONE,
            ewram: vec![0; 0x1000],
            iwram: vec![0; 0x0200],
        }
    }

    fn init_ram(&mut self) {
        self.ewram.fill(0);
        self.iwram.fill(0);
        // Point gSaveBlock1Ptr -> SB1_BASE_INIT
        let ptr_off = (SYM_SAVEBLOCK1_PTR - IWRAM_START) as usize;
        self.iwram[ptr_off..ptr_off + 4].copy_from_slice(&SB1_BASE_INIT.to_le_bytes());
        // Initial money = 3000
        let money_off = (SB1_BASE_INIT - EWRAM_START + SB1_OFFSET_MONEY) as usize;
        self.ewram[money_off..money_off + 4].copy_from_slice(&3000u32.to_le_bytes());
    }

    fn read_u32_iwram(&self, addr: u32) -> u32 {
        let off = (addr - IWRAM_START) as usize;
        u32::from_le_bytes(self.iwram[off..off + 4].try_into().unwrap())
    }
}

impl Emulator for MockEmulator {
    fn load_rom(&mut self, rom: &[u8]) -> Result<(), EmuError> {
        self.rom = Some(rom.to_vec());
        self.reset()
    }

    fn reset(&mut self) -> Result<(), EmuError> {
        if self.rom.is_none() {
            return Err(EmuError::NoRom);
        }
        self.frame_count = 0;
        self.buttons = Buttons::NONE;
        self.init_ram();
        Ok(())
    }

    fn run_frame(&mut self) -> Result<(), EmuError> {
        if self.rom.is_none() {
            return Err(EmuError::NoRom);
        }
        self.frame_count += 1;
        let main_off = (SYM_MAIN_FRAME - IWRAM_START) as usize;
        self.iwram[main_off..main_off + 4].copy_from_slice(&self.frame_count.to_le_bytes());

        // Pressing A triggers the simulated lost battle (only on the hacked ROM,
        // byte 0 == 0xAA) and moves gSaveBlock1Ptr by +0x20 to test deref re-read.
        if self.buttons.contains(Button::A) && self.rom.as_deref() == Some(&[0xAA, 0xBB, 0xCC, 0xDD]) {
            let new_sb1 = SB1_BASE_INIT + 0x20;
            let ptr_off = (SYM_SAVEBLOCK1_PTR - IWRAM_START) as usize;
            self.iwram[ptr_off..ptr_off + 4].copy_from_slice(&new_sb1.to_le_bytes());

            // gBattleOutcome = B_OUTCOME_LOST (2)
            self.ewram[(SYM_BATTLE_OUTCOME - EWRAM_START) as usize] = B_OUTCOME_LOST;

            // Money stays 3000 (no whiteout loss) at moved SaveBlock1
            let money_off = (new_sb1 - EWRAM_START + SB1_OFFSET_MONEY) as usize;
            self.ewram[money_off..money_off + 4].copy_from_slice(&3000u32.to_le_bytes());

            // Set flag bit 0x20 (byte 4, bit 0) in moved SaveBlock1
            let flag_byte = (new_sb1 - EWRAM_START + SB1_OFFSET_FLAGS + (0x20 / 8)) as usize;
            self.ewram[flag_byte] |= 1 << (0x20 % 8);
        }
        Ok(())
    }

    fn frame_count(&self) -> u32 {
        self.frame_count
    }

    fn set_buttons(&mut self, buttons: Buttons) {
        self.buttons = buttons;
    }

    fn frame(&self) -> Frame {
        let mut rgba = vec![0u8; 4 * 4 * 4];
        let red = if self.buttons.contains(Button::A) { 255 } else { 0 };
        for px in rgba.chunks_exact_mut(4) {
            px[0] = red;
            px[1] = 64;
            px[2] = 128;
            px[3] = 255;
        }
        Frame {
            width: 4,
            height: 4,
            rgba,
        }
    }

    fn audio_rate(&self) -> u32 {
        32768
    }

    fn drain_audio(&mut self, _out: &mut Vec<i16>) -> usize {
        0
    }

    fn memory_regions(&self) -> Vec<MemoryRegion> {
        vec![
            MemoryRegion {
                id: "wram".to_owned(),
                name: "EWRAM".to_owned(),
                start: EWRAM_START,
                size: self.ewram.len() as u32,
                writable: true,
            },
            MemoryRegion {
                id: "iwram".to_owned(),
                name: "IWRAM".to_owned(),
                start: IWRAM_START,
                size: self.iwram.len() as u32,
                writable: true,
            },
        ]
    }

    fn read_memory(&self, addr: u32, len: usize) -> Result<Vec<u8>, EmuError> {
        if addr >= EWRAM_START && (addr - EWRAM_START) as usize + len <= self.ewram.len() {
            let start = (addr - EWRAM_START) as usize;
            return Ok(self.ewram[start..start + len].to_vec());
        }
        if addr >= IWRAM_START && (addr - IWRAM_START) as usize + len <= self.iwram.len() {
            let start = (addr - IWRAM_START) as usize;
            return Ok(self.iwram[start..start + len].to_vec());
        }
        Err(EmuError::Memory(format!("unmapped read 0x{addr:08x}+{len}")))
    }

    fn write_memory(&mut self, addr: u32, bytes: &[u8]) -> Result<(), EmuError> {
        if addr >= EWRAM_START && (addr - EWRAM_START) as usize + bytes.len() <= self.ewram.len() {
            let start = (addr - EWRAM_START) as usize;
            self.ewram[start..start + bytes.len()].copy_from_slice(bytes);
            return Ok(());
        }
        if addr >= IWRAM_START && (addr - IWRAM_START) as usize + bytes.len() <= self.iwram.len() {
            let start = (addr - IWRAM_START) as usize;
            self.iwram[start..start + bytes.len()].copy_from_slice(bytes);
            return Ok(());
        }
        Err(EmuError::Memory(format!("unmapped write 0x{addr:08x}")))
    }

    fn save_state(&mut self) -> Result<Vec<u8>, EmuError> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.frame_count.to_le_bytes());
        out.extend_from_slice(&self.ewram);
        out.extend_from_slice(&self.iwram);
        let _ = self.read_u32_iwram(SYM_SAVEBLOCK1_PTR);
        Ok(out)
    }

    fn load_state(&mut self, state: &[u8]) -> Result<(), EmuError> {
        if state.len() != 4 + self.ewram.len() + self.iwram.len() {
            return Err(EmuError::State("invalid state size".to_owned()));
        }
        self.frame_count = u32::from_le_bytes(state[0..4].try_into().unwrap());
        let ew_end = 4 + self.ewram.len();
        self.ewram.copy_from_slice(&state[4..ew_end]);
        self.iwram.copy_from_slice(&state[ew_end..]);
        Ok(())
    }
}

struct MockBootHook;

impl BootHook for MockBootHook {
    fn boot_to(
        &mut self,
        emu: &mut dyn Emulator,
        _map: &str,
        _x: i16,
        _y: i16,
        _flags: &[String],
    ) -> Result<u32, String> {
        emu.reset().map_err(|e| e.to_string())?;
        for _ in 0..5 {
            emu.run_frame().map_err(|e| e.to_string())?;
        }
        Ok(emu.frame_count())
    }
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
                    "clientInfo": { "name": "tauros-emu-mcp-test", "version": "0.1.0" }
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

fn mock_session() -> EmuSession {
    let mut syms = BTreeMap::new();
    syms.insert("gBattleOutcome".to_owned(), SYM_BATTLE_OUTCOME);
    syms.insert("gMain".to_owned(), SYM_MAIN_FRAME);
    syms.insert("gSaveBlock1Ptr".to_owned(), SYM_SAVEBLOCK1_PTR);

    let mut constants = BTreeMap::new();
    constants.insert("B_OUTCOME_LOST".to_owned(), B_OUTCOME_LOST as i64);

    EmuSession::new(MockEmulator::new())
        .with_rom(vec![0xAA, 0xBB, 0xCC, 0xDD])
        .unwrap()
        .with_base_rom(vec![0x00, 0x00, 0x00, 0x00])
        .with_symbols(SymbolTable::from_map(syms))
        .with_constants(constants)
        .with_boot_hook(MockBootHook)
}

#[tokio::test]
async fn emulator_tools_detect_known_state_and_negation_via_mcp() {
    let server = TaurosServer::default().with_emulator(mock_session());
    let mut client = McpClient::start(server).await;

    // 1. Discover all emulator tools
    let listed = client.request("tools/list", json!({})).await;
    let names: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for expected in [
        "emu_boot",
        "emu_press_input",
        "emu_screenshot",
        "emu_save_state",
        "emu_load_state",
        "emu_assert_ram",
        "run_verification",
    ] {
        assert!(names.contains(&expected), "missing tool {expected}");
    }

    // 2. Boot to map via BootHook
    let (is_err, body) = client
        .call_tool(
            "emu_boot",
            json!({ "kind": "boot_to", "map": "MAP_ROUTE101", "x": 10, "y": 10 }),
        )
        .await;
    assert!(!is_err, "emu_boot failed: {body}");
    assert_eq!(body["frame_count"], 5);

    // 3. Save state before input
    let (is_err, body) = client
        .call_tool("emu_save_state", json!({ "name": "before_battle" }))
        .await;
    assert!(!is_err, "emu_save_state failed: {body}");
    assert_eq!(body["frame_count"], 5);

    // Before pressing A: gBattleOutcome == 0 (known initial state), == B_OUTCOME_LOST is false (negation)
    let (is_err, body) = client
        .call_tool(
            "emu_assert_ram",
            json!({
                "symbol": "gBattleOutcome",
                "type": "u8",
                "op": "==",
                "expected": "B_OUTCOME_LOST"
            }),
        )
        .await;
    assert!(!is_err);
    assert_eq!(body["passed"], false);

    // 4. Press A to trigger the lost battle, moving gSaveBlock1Ptr and setting flag 0x20
    let (is_err, body) = client
        .call_tool(
            "emu_press_input",
            json!({ "buttons": ["A"], "frames": 4, "release_frames": 2 }),
        )
        .await;
    assert!(!is_err, "emu_press_input failed: {body}");
    assert_eq!(body["frame_count"], 11);

    // 5. Screenshot produces a valid PNG
    let response = client.request("tools/call", json!({
        "name":"emu_screenshot", "arguments":{"include_base64":true}
    })).await;
    let result = &response["result"];
    assert_ne!(result["isError"], true);
    let body: Value = serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(body.get("png_base64").is_none());
    assert_eq!(result["content"][1]["type"], "image");
    assert_eq!(result["content"][1]["mimeType"], "image/png");
    assert_eq!(body["width"], 4);
    assert_eq!(body["height"], 4);
    let png_bytes = base64::engine::general_purpose::STANDARD
        .decode(result["content"][1]["data"].as_str().unwrap())
        .unwrap();
    assert!(png_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));

    // 6. RAM assertions detect the known state AND its negation (including deref + flag_bit + money)
    let (is_err, body) = client
        .call_tool(
            "emu_assert_ram",
            json!({
                "symbol": "gBattleOutcome",
                "type": "u8",
                "op": "==",
                "expected": "B_OUTCOME_LOST"
            }),
        )
        .await;
    assert!(!is_err);
    assert_eq!(body["passed"], true);

    let (is_err, body) = client
        .call_tool(
            "emu_assert_ram",
            json!({
                "symbol": "gSaveBlock1Ptr",
                "deref": true,
                "offset": SB1_OFFSET_FLAGS,
                "type": "flag_bit",
                "flag_bit": 0x20,
                "op": "==",
                "expected": 1
            }),
        )
        .await;
    assert!(!is_err, "flag_bit assert failed: {body}");
    assert_eq!(body["passed"], true);

    let (is_err, body) = client
        .call_tool(
            "emu_assert_ram",
            json!({
                "symbol": "gSaveBlock1Ptr",
                "deref": true,
                "offset": SB1_OFFSET_FLAGS,
                "type": "flag_bit",
                "flag_bit": 0x20,
                "op": "==",
                "expected": 0
            }),
        )
        .await;
    assert!(!is_err);
    assert_eq!(body["passed"], false);

    let (is_err, body) = client
        .call_tool(
            "emu_assert_ram",
            json!({
                "symbol": "gSaveBlock1Ptr",
                "deref": true,
                "offset": SB1_OFFSET_MONEY,
                "type": "u32",
                "op": "==",
                "expected": 3000
            }),
        )
        .await;
    assert!(!is_err);
    assert_eq!(body["passed"], true);

    // 7. Reject raw addresses and save-block pointers without deref
    let (is_err, body) = client
        .call_tool(
            "emu_assert_ram",
            json!({
                "symbol": "0x02000040",
                "type": "u8",
                "op": "==",
                "expected": 2
            }),
        )
        .await;
    assert!(is_err);
    assert_eq!(body["kind"], "invalid_argument");

    let (is_err, body) = client
        .call_tool(
            "emu_assert_ram",
            json!({
                "symbol": "gSaveBlock1Ptr",
                "deref": false,
                "type": "u32",
                "op": "==",
                "expected": 0
            }),
        )
        .await;
    assert!(is_err);
    assert_eq!(body["kind"], "invalid_argument");

    // 8. Load state restores frame 5 and clears gBattleOutcome back to 0
    let (is_err, body) = client
        .call_tool("emu_load_state", json!({ "name": "before_battle" }))
        .await;
    assert!(!is_err, "emu_load_state failed: {body}");
    assert_eq!(body["frame_count"], 5);

    let (is_err, body) = client
        .call_tool(
            "emu_assert_ram",
            json!({
                "symbol": "gBattleOutcome",
                "type": "u8",
                "op": "==",
                "expected": 0
            }),
        )
        .await;
    assert!(!is_err);
    assert_eq!(body["passed"], true);

    client.close().await;
}

#[tokio::test]
async fn ver_001_verification_specs_enforce_proven_failed_inconclusive_and_rejected() {
    let server = TaurosServer::default().with_emulator(mock_session());
    let mut client = McpClient::start(server).await;

    // 1. Valid discriminating spec with shift + wrong_value + base_rom counterfactuals -> PROVEN
    let (is_err, body) = client
        .call_tool(
            "run_verification",
            json!({
                "schema": 1,
                "name": "director-loss-check",
                "description": "Pressing A triggers B_OUTCOME_LOST and sets flag 0x20 on hacked ROM only.",
                "start": { "kind": "boot_to", "map": "MAP_ROUTE101", "x": 10, "y": 10 },
                "timeline": [
                    { "name": "battle", "frames": 4, "keys": ["A"] },
                    { "name": "settle", "frames": 2, "keys": [] }
                ],
                "probes": [
                    {
                        "id": "outcome",
                        "why": "Check battle outcome is B_OUTCOME_LOST.",
                        "symbol": "gBattleOutcome",
                        "type": "u8",
                        "sample": { "at_step_end": ["settle"] }
                    },
                    {
                        "id": "alive",
                        "why": "Liveness: frame counter advances.",
                        "symbol": "gMain",
                        "type": "u32",
                        "sample": { "every": 2 },
                        "supporting": true
                    }
                ],
                "assert": [
                    { "check": "compare", "probe": "outcome", "at": "settle", "op": "==", "value": "B_OUTCOME_LOST" },
                    { "check": "strictly_increases", "probe": "alive", "supporting": true }
                ],
                "counterfactuals": ["shift", "wrong_value", "base_rom"]
            }),
        )
        .await;
    assert!(!is_err, "run_verification error: {body}");
    assert_eq!(body["verdict"], "PROVEN", "{body}");

    // 2. Wrong expected value -> FAILED
    let (is_err, body) = client
        .call_tool(
            "run_verification",
            json!({
                "schema": 1,
                "name": "wrong-outcome",
                "description": "Expects outcome 99 instead of 2.",
                "start": { "kind": "cold_boot" },
                "timeline": [{ "name": "battle", "frames": 4, "keys": ["A"] }],
                "probes": [{
                    "id": "outcome",
                    "why": "Battle outcome",
                    "symbol": "gBattleOutcome",
                    "type": "u8",
                    "sample": { "at_step_end": ["battle"] }
                }],
                "assert": [{ "check": "compare", "probe": "outcome", "at": "battle", "op": "==", "value": 99 }],
                "counterfactuals": ["shift"]
            }),
        )
        .await;
    assert!(!is_err);
    assert_eq!(body["verdict"], "FAILED", "{body}");

    // 3. Weak assertion (`!= 99`) survives shift counterfactual -> INCONCLUSIVE
    let (is_err, body) = client
        .call_tool(
            "run_verification",
            json!({
                "schema": 1,
                "name": "weak-assertion",
                "description": "Weak != assertion survives counterfactuals.",
                "start": { "kind": "cold_boot" },
                "timeline": [{ "name": "battle", "frames": 4, "keys": ["A"] }],
                "probes": [{
                    "id": "outcome",
                    "why": "Battle outcome",
                    "symbol": "gBattleOutcome",
                    "type": "u8",
                    "sample": { "at_step_end": ["battle"] }
                }],
                "assert": [{ "check": "compare", "probe": "outcome", "at": "battle", "op": "!=", "value": 99 }],
                "counterfactuals": ["shift"]
            }),
        )
        .await;
    assert!(!is_err);
    assert_eq!(body["verdict"], "INCONCLUSIVE", "{body}");

    // 4. Spec violating loader rules (missing why, no counterfactuals, save-block without deref) -> REJECTED
    let (is_err, body) = client
        .call_tool(
            "run_verification",
            json!({
                "schema": 1,
                "name": "bad-spec",
                "description": "Violates VER-001 loader rules.",
                "start": { "kind": "cold_boot" },
                "timeline": [{ "name": "step", "frames": 2, "keys": [] }],
                "probes": [{
                    "id": "sb",
                    "why": "",
                    "symbol": "gSaveBlock1Ptr",
                    "deref": false,
                    "type": "u32",
                    "sample": { "at_step_end": ["step"] }
                }],
                "assert": [{ "check": "compare", "probe": "sb", "at": "step", "op": "==", "value": 1 }],
                "counterfactuals": []
            }),
        )
        .await;
    assert!(!is_err);
    assert_eq!(body["verdict"], "REJECTED", "{body}");
    assert!(body["rejection_reasons"].as_array().unwrap().len() >= 3);

    client.close().await;
}

fn base_rom_and_sym() -> Option<(PathBuf, PathBuf)> {
    let data = std::env::var_os("TAUROS_DATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share/tauros")))?;
    let engines = data.join("engines");
    for entry in fs::read_dir(&engines).ok()?.flatten() {
        let p = entry.path();
        if p.is_dir()
            && p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with("-base"))
        {
            let rom = p.join("pokeemerald.gba");
            let sym = p.join("pokeemerald.sym");
            if rom.is_file() && sym.is_file() {
                return Some((rom, sym));
            }
        }
    }
    None
}

#[tokio::test]
async fn real_mgba_emerald_ram_checks_and_screenshot_via_mcp() {
    let Some((rom_path, sym_path)) = base_rom_and_sym() else {
        eprintln!("skipping real libmgba MCP test: base ROM not found under $TAUROS_DATA");
        return;
    };
    let rom = fs::read(&rom_path).unwrap();
    let sym_text = fs::read_to_string(&sym_path).unwrap();
    let symbols = SymbolTable::parse(&sym_text);

    let session = EmuSession::new(Mgba::gba().unwrap())
        .with_rom(rom)
        .unwrap()
        .with_symbols(symbols);
    let mut client = McpClient::start(TaurosServer::default().with_emulator(session)).await;

    // Cold boot 120 frames so gSaveBlock1Ptr is initialised
    let (is_err, body) = client
        .call_tool("emu_boot", json!({ "kind": "cold_boot", "frames": 120 }))
        .await;
    assert!(!is_err, "emu_boot failed: {body}");
    assert_eq!(body["frame_count"], 120);

    // Screenshot from real libmgba is a valid 240x160 PNG
    let response = client.request("tools/call", json!({
        "name":"emu_screenshot", "arguments":{"include_base64":true}
    })).await;
    let result = &response["result"];
    assert_ne!(result["isError"], true);
    let body: Value = serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(body.get("png_base64").is_none());
    assert_eq!(result["content"][1]["type"], "image");
    assert_eq!(result["content"][1]["mimeType"], "image/png");
    assert_eq!(body["width"], 240);
    assert_eq!(body["height"], 160);
    let png_bytes = base64::engine::general_purpose::STANDARD
        .decode(result["content"][1]["data"].as_str().unwrap())
        .unwrap();
    assert!(png_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));

    // Save state, press Start for 10 frames, reload state
    let (is_err, body) = client
        .call_tool("emu_save_state", json!({ "name": "f120" }))
        .await;
    assert!(!is_err, "emu_save_state failed: {body}");
    assert_eq!(body["byte_len"], 397312);

    let (is_err, _) = client
        .call_tool(
            "emu_press_input",
            json!({ "buttons": ["START"], "frames": 10 }),
        )
        .await;
    assert!(!is_err);

    let (is_err, body) = client
        .call_tool("emu_load_state", json!({ "name": "f120" }))
        .await;
    assert!(!is_err, "emu_load_state failed: {body}");
    assert_eq!(body["frame_count"], 120);

    // Read ROM-resident game code via symbol or gMain in RAM: gBattleOutcome == 0 at title screen
    let (is_err, body) = client
        .call_tool(
            "emu_assert_ram",
            json!({
                "symbol": "gBattleOutcome",
                "type": "u8",
                "op": "==",
                "expected": 0
            }),
        )
        .await;
    assert!(!is_err, "gBattleOutcome read failed: {body}");
    assert_eq!(body["passed"], true);

    // Negation: gBattleOutcome == 2 must be false at title screen
    let (is_err, body) = client
        .call_tool(
            "emu_assert_ram",
            json!({
                "symbol": "gBattleOutcome",
                "type": "u8",
                "op": "==",
                "expected": 2
            }),
        )
        .await;
    assert!(!is_err);
    assert_eq!(body["passed"], false);

    client.close().await;
}


#[tokio::test]
async fn screenshot_native_image_metadata_opt_out_and_errors() {
    let mut empty = McpClient::start(TaurosServer::default()
        .with_emulator(EmuSession::new(MockEmulator::new()))).await;
    // A missing loaded ROM remains a normal tool error, with no image block.
    let missing = empty.request("tools/call", json!({"name":"emu_screenshot", "arguments":{}})).await;
    assert_eq!(missing["result"]["isError"], true);
    assert_eq!(missing["result"]["content"].as_array().unwrap().len(), 1);
    empty.close().await;
    let mut client = McpClient::start(TaurosServer::default().with_emulator(mock_session())).await;
    let image = client.request("tools/call", json!({"name":"emu_screenshot", "arguments":{}})).await;
    assert_ne!(image["result"]["isError"], true);
    let blocks = image["result"]["content"].as_array().unwrap();
    assert_eq!(blocks.len(), 2);
    let metadata: Value = serde_json::from_str(blocks[0]["text"].as_str().unwrap()).unwrap();
    assert!(metadata.get("png_base64").is_none());
    assert_eq!(metadata["width"], 4);
    assert_eq!(blocks[1]["type"], "image");
    assert_eq!(blocks[1]["mimeType"], "image/png");
    let png = base64::engine::general_purpose::STANDARD.decode(blocks[1]["data"].as_str().unwrap()).unwrap();
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    let metadata_only = client.request("tools/call", json!({
        "name":"emu_screenshot", "arguments":{"include_base64":false}
    })).await;
    assert_ne!(metadata_only["result"]["isError"], true);
    assert_eq!(metadata_only["result"]["content"].as_array().unwrap().len(), 1);
    let mut absent = McpClient::start(TaurosServer::default()).await;
    let no_emulator = absent.request("tools/call", json!({"name":"emu_screenshot", "arguments":{}})).await;
    assert_eq!(no_emulator["result"]["isError"], true);
    absent.close().await;
    client.close().await;
}
