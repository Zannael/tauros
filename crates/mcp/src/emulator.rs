//! Emulator MCP tools and `VER-001` RAM-based verification engine (`MCP-003`).
//!
//! Operates strictly through `tauros_emu::Emulator` and an optional injected
//! [`BootHook`] so `tauros-mcp` stays independent of game-specific connectors.
//! Following `docs/design/verification-specs.md`:
//! - Verification specs are inert JSON data.
//! - The sampling driver never sees assertions or expected values.
//! - The evaluator computes `PROVEN` / `FAILED` / `INCONCLUSIVE` / `REJECTED`
//!   outside the driver, including mandatory counterfactuals.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use base64::Engine as _;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauros_emu::{Button, Buttons, Emulator, Frame};

/// Game-specific boot hook injected by Layer 1 (e.g. `tauros-connector-gen3::boot`).
pub trait BootHook: Send {
    fn boot_to(
        &mut self,
        emu: &mut dyn Emulator,
        map: &str,
        x: i16,
        y: i16,
        flags: &[String],
    ) -> Result<u32, String>;
}

/// Global symbol table parsed from a build's `.sym` file (`addr scope size name`).
#[derive(Debug, Clone, Default)]
pub struct SymbolTable {
    entries: BTreeMap<String, u32>,
    sha256: Option<String>,
}

impl SymbolTable {
    /// Parses a `.sym` file and records its content sha256.
    pub fn parse(text: &str) -> Self {
        let mut entries = BTreeMap::new();
        for line in text.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                if let Ok(addr) = u32::from_str_radix(parts[0], 16) {
                    entries.entry(parts[3].to_owned()).or_insert(addr);
                }
            }
        }
        Self {
            entries,
            sha256: Some(sha256_hex(text.as_bytes())),
        }
    }

    /// Builds a symbol table directly from a name-to-address map (useful for tests).
    pub fn from_map(entries: BTreeMap<String, u32>) -> Self {
        let mut canonical = String::new();
        for (name, addr) in &entries {
            canonical.push_str(&format!("{addr:08x} g 00000000 {name}\n"));
        }
        let sha256 = Some(sha256_hex(canonical.as_bytes()));
        Self { entries, sha256 }
    }

    pub fn get(&self, name: &str) -> Option<u32> {
        self.entries.get(name).copied()
    }

    pub fn sha256(&self) -> Option<&str> {
        self.sha256.as_deref()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug, Clone)]
struct SavedSlot {
    rom_sha256: String,
    bytes: Vec<u8>,
}

/// Stateful emulator session attached to [`crate::TaurosServer`].
pub struct EmuSession {
    emu: Box<dyn Emulator + Send>,
    rom: Option<Vec<u8>>,
    rom_sha256: Option<String>,
    base_rom: Option<Vec<u8>>,
    symbols: SymbolTable,
    constants: BTreeMap<String, i64>,
    boot_hook: Option<Box<dyn BootHook + Send>>,
    savestates: BTreeMap<String, SavedSlot>,
}

impl EmuSession {
    /// Wraps an [`Emulator`] core with no ROM loaded yet.
    pub fn new<E: Emulator + Send + 'static>(emu: E) -> Self {
        Self {
            emu: Box::new(emu),
            rom: None,
            rom_sha256: None,
            base_rom: None,
            symbols: SymbolTable::default(),
            constants: BTreeMap::new(),
            boot_hook: None,
            savestates: BTreeMap::new(),
        }
    }

    /// Loads a ROM image into the core and records its sha256.
    pub fn with_rom(mut self, rom: Vec<u8>) -> Result<Self, String> {
        self.load_rom(rom)?;
        Ok(self)
    }

    /// Replaces the loaded ROM image and resets the core.
    pub fn load_rom(&mut self, rom: Vec<u8>) -> Result<String, String> {
        self.emu.load_rom(&rom).map_err(|e| e.to_string())?;
        let hash = sha256_hex(&rom);
        self.rom = Some(rom);
        self.rom_sha256 = Some(hash.clone());
        Ok(hash)
    }

    /// Attaches an unmodified base ROM for `VER-001` `base_rom` counterfactuals.
    pub fn with_base_rom(mut self, rom: Vec<u8>) -> Self {
        self.base_rom = Some(rom);
        self
    }

    /// Attaches the build's paired [`SymbolTable`].
    pub fn with_symbols(mut self, symbols: SymbolTable) -> Self {
        self.symbols = symbols;
        self
    }

    /// Replaces the paired [`SymbolTable`] in place (e.g. after a rebuild).
    pub fn set_symbols(&mut self, symbols: SymbolTable) {
        self.symbols = symbols;
    }

    /// Registers named integer constants (e.g. `B_OUTCOME_LOST`, `MAP_ROUTE101`).
    pub fn with_constants(mut self, constants: BTreeMap<String, i64>) -> Self {
        self.constants = constants;
        self
    }

    /// Replaces the registered named integer constants in place.
    pub fn set_constants(&mut self, constants: BTreeMap<String, i64>) {
        self.constants = constants;
    }

    /// Attaches a game-specific [`BootHook`] for `boot_to`.
    pub fn with_boot_hook<B: BootHook + Send + 'static>(mut self, hook: B) -> Self {
        self.boot_hook = Some(Box::new(hook));
        self
    }

    pub fn rom_sha256(&self) -> Option<&str> {
        self.rom_sha256.as_deref()
    }

    /// Whether a ROM is loaded.
    pub fn has_rom(&self) -> bool {
        self.rom.is_some()
    }

    /// The emulator core, for a host that also plays the session (the app's window).
    pub fn emulator_mut(&mut self) -> &mut (dyn Emulator + Send) {
        self.emu.as_mut()
    }

    /// The emulator core, read-only (for example to copy the current frame).
    pub fn emulator(&self) -> &(dyn Emulator + Send) {
        self.emu.as_ref()
    }
}

/// Structured error returned by emulator MCP tools.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmuToolError {
    pub ok: bool,
    pub kind: String,
    pub message: String,
}

impl EmuToolError {
    pub fn new(kind: &str, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            kind: kind.to_owned(),
            message: message.into(),
        }
    }

    pub fn no_emulator() -> Self {
        Self::new(
            "no_emulator",
            "no emulator session is attached to this MCP server",
        )
    }
}

/// Console button name accepted by `emu_press_input` and verification timelines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "UPPERCASE")]
pub enum ButtonInput {
    A,
    B,
    Select,
    Start,
    Right,
    Left,
    Up,
    Down,
    R,
    L,
}

impl From<ButtonInput> for Button {
    fn from(b: ButtonInput) -> Self {
        match b {
            ButtonInput::A => Button::A,
            ButtonInput::B => Button::B,
            ButtonInput::Select => Button::Select,
            ButtonInput::Start => Button::Start,
            ButtonInput::Right => Button::Right,
            ButtonInput::Left => Button::Left,
            ButtonInput::Up => Button::Up,
            ButtonInput::Down => Button::Down,
            ButtonInput::R => Button::R,
            ButtonInput::L => Button::L,
        }
    }
}

fn buttons_from_slice(list: &[ButtonInput]) -> Buttons {
    list.iter().copied().map(Button::from).collect()
}

/// Boot mode for `emu_boot`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BootKind {
    /// Cold-reset the loaded ROM and advance `frames` frames.
    ColdBoot,
    /// Boot directly to a map, coordinates and flag set via the game's boot hook (`EMU-003`).
    BootTo,
}

/// Arguments for `emu_boot`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EmuBootRequest {
    /// Boot mode (`cold_boot` or `boot_to`).
    pub kind: BootKind,
    /// Number of frames to advance after a `cold_boot` (default 1).
    #[serde(default = "default_one")]
    pub frames: u32,
    /// Target map constant for `boot_to`.
    #[serde(default)]
    pub map: Option<String>,
    /// Target X metatile coordinate for `boot_to`.
    #[serde(default)]
    pub x: Option<i16>,
    /// Target Y metatile coordinate for `boot_to`.
    #[serde(default)]
    pub y: Option<i16>,
    /// Flags to set before loading the map for `boot_to`.
    #[serde(default)]
    pub flags: Vec<String>,
}

fn default_one() -> u32 {
    1
}

/// Result of `emu_boot`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmuBootResult {
    pub ok: bool,
    pub mode: String,
    pub frame_count: u32,
    pub rom_sha256: Option<String>,
}

/// Arguments for `emu_press_input`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EmuPressInputRequest {
    /// Buttons to hold during `frames`.
    #[serde(default)]
    pub buttons: Vec<ButtonInput>,
    /// Number of frames to hold the buttons (default 1).
    #[serde(default = "default_one")]
    pub frames: u32,
    /// Optional number of neutral frames to run after releasing buttons.
    #[serde(default)]
    pub release_frames: u32,
}

/// Result of `emu_press_input`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmuPressInputResult {
    pub ok: bool,
    pub frame_count: u32,
    pub frames_advanced: u32,
}

/// Arguments for `emu_screenshot`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EmuScreenshotRequest {
    /// Optional filesystem path where the PNG should also be written.
    #[serde(default)]
    pub save_path: Option<String>,
    /// Include PNG pixels (default true). The MCP surface emits a native image
    /// block; this internal result retains base64 for serialization/storage.
    #[serde(default = "default_true")]
    pub include_base64: bool,
}

fn default_true() -> bool {
    true
}

/// Result of `emu_screenshot`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmuScreenshotResult {
    pub ok: bool,
    pub width: u32,
    pub height: u32,
    pub frame_count: u32,
    pub png_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub png_base64: Option<String>,
}

/// Arguments for `emu_save_state` and `emu_load_state`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EmuStateRequest {
    /// Named savestate slot.
    pub name: String,
}

/// Result of `emu_save_state` / `emu_load_state`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmuStateResult {
    pub ok: bool,
    pub name: String,
    pub frame_count: u32,
    pub byte_len: usize,
    pub rom_sha256: String,
}

/// Memory value type for RAM probes and assertions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    U8,
    U16,
    U32,
    S8,
    S16,
    S32,
    Ptr,
    Bytes,
    /// Reads a single bit at index `flag_bit` relative to `base + offset`.
    FlagBit,
}

/// Comparison operator for RAM assertions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CompareOp {
    #[serde(rename = "==")]
    Eq,
    #[serde(rename = "!=")]
    Ne,
    #[serde(rename = "<")]
    Lt,
    #[serde(rename = "<=")]
    Le,
    #[serde(rename = ">")]
    Gt,
    #[serde(rename = ">=")]
    Ge,
}

/// Expected value supplied to `emu_assert_ram` or a verification assertion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ExpectedValue {
    Int(i64),
    Constant(String),
    Bytes(Vec<u8>),
}

/// Observed value sampled from RAM.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ObservedValue {
    Int(i64),
    Bytes(Vec<u8>),
}

/// Arguments for `emu_assert_ram`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EmuAssertRamRequest {
    /// Symbol name from the paired `.sym` table (raw addresses are rejected).
    pub symbol: String,
    /// Re-read a 32-bit pointer at `symbol` before applying `offset` (required for save blocks).
    #[serde(default)]
    pub deref: bool,
    /// Byte offset added after symbol resolution (and optional deref).
    #[serde(default)]
    pub offset: u32,
    /// Type to decode from RAM.
    #[serde(rename = "type")]
    pub value_type: ValueType,
    /// Required byte length when `type` is `bytes`.
    #[serde(default)]
    pub length: Option<usize>,
    /// Bit index when `type` is `flag_bit`.
    #[serde(default)]
    pub flag_bit: Option<u32>,
    /// Comparison operator.
    pub op: CompareOp,
    /// Expected integer, named constant, or byte array.
    pub expected: ExpectedValue,
}

/// Result of `emu_assert_ram`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmuAssertRamResult {
    pub ok: bool,
    pub passed: bool,
    pub symbol: String,
    pub resolved_address: String,
    pub observed: ObservedValue,
    pub expected: ObservedValue,
    pub op: CompareOp,
}

// ---------------------------------------------------------------------------
// VER-001 inert verification spec, driver, evaluator and counterfactuals
// ---------------------------------------------------------------------------

/// Start condition of a `VER-001` verification spec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SpecStart {
    ColdBoot,
    Savestate {
        name: String,
        rom_sha256: String,
    },
    BootTo {
        map: String,
        x: i16,
        y: i16,
        #[serde(default)]
        flags: Vec<String>,
    },
}

/// One timeline step in a `VER-001` verification spec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TimelineStep {
    pub name: String,
    pub frames: u32,
    #[serde(default)]
    pub keys: Vec<ButtonInput>,
    /// Optional driver strategy for RNG-driven steps (e.g. `"lose-fast"`, `"mash-a"`).
    #[serde(default)]
    pub policy: Option<String>,
}

/// Sampling schedule for a probe.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProbeSchedule {
    #[serde(default)]
    pub every: Option<u32>,
    #[serde(default)]
    pub at_step_start: Vec<String>,
    #[serde(default)]
    pub at_step_end: Vec<String>,
}

/// Memory probe in a `VER-001` verification spec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProbeSpec {
    pub id: String,
    pub why: String,
    pub symbol: String,
    #[serde(default)]
    pub deref: bool,
    #[serde(default)]
    pub offset: u32,
    #[serde(rename = "type")]
    pub value_type: ValueType,
    #[serde(default)]
    pub length: Option<usize>,
    #[serde(default)]
    pub flag_bit: Option<u32>,
    #[serde(default)]
    pub sample: ProbeSchedule,
    #[serde(default)]
    pub supporting: bool,
}

/// Assertion in a `VER-001` verification spec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AssertSpec {
    pub check: String,
    pub probe: String,
    #[serde(default)]
    pub at: Option<String>,
    #[serde(default)]
    pub op: Option<CompareOp>,
    #[serde(default)]
    pub value: Option<ExpectedValue>,
    #[serde(default)]
    pub supporting: bool,
}

/// Counterfactual kind per `VER-001`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CounterfactualKind {
    Shift,
    WrongValue,
    BaseRom,
    ColdBoot,
}

/// Inert JSON verification spec (`VER-001`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VerificationSpec {
    pub schema: u32,
    pub name: String,
    pub description: String,
    pub start: SpecStart,
    pub timeline: Vec<TimelineStep>,
    pub probes: Vec<ProbeSpec>,
    #[serde(rename = "assert")]
    pub assertions: Vec<AssertSpec>,
    pub counterfactuals: Vec<CounterfactualKind>,
}

/// Verdict returned by the `VER-001` evaluator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Verdict {
    Proven,
    Failed,
    Inconclusive,
    Rejected,
}

/// Per-assertion outcome recorded in a [`VerificationReport`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssertionOutcome {
    pub probe: String,
    pub check: String,
    pub passed: bool,
    pub discriminating: bool,
    pub detail: String,
}

/// Outcome of one counterfactual run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CounterfactualOutcome {
    pub kind: CounterfactualKind,
    pub failed_as_expected: bool,
    pub discriminating_failed: bool,
    pub assertions: Vec<AssertionOutcome>,
}

/// Complete report returned by `run_verification`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationReport {
    pub ok: bool,
    pub name: String,
    pub verdict: Verdict,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejection_reasons: Vec<String>,
    pub rom_sha256: Option<String>,
    pub sym_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assertions: Vec<AssertionOutcome>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub counterfactuals: Vec<CounterfactualOutcome>,
}

/// Raw sample emitted by the assertion-free driver.
#[derive(Debug, Clone)]
struct SampleRecord {
    probe_id: String,
    step_end: Option<String>,
    value: ObservedValue,
}

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

pub fn emu_boot(session: &mut EmuSession, req: EmuBootRequest) -> Result<EmuBootResult, EmuToolError> {
    if session.rom.is_none() {
        return Err(EmuToolError::new("no_rom", "no ROM is loaded in the emulator"));
    }
    match req.kind {
        BootKind::ColdBoot => {
            session
                .emu
                .reset()
                .map_err(|e| EmuToolError::new("emu_error", e.to_string()))?;
            session.emu.set_buttons(Buttons::NONE);
            for _ in 0..req.frames {
                session
                    .emu
                    .run_frame()
                    .map_err(|e| EmuToolError::new("emu_error", e.to_string()))?;
            }
            Ok(EmuBootResult {
                ok: true,
                mode: "cold_boot".to_owned(),
                frame_count: session.emu.frame_count(),
                rom_sha256: session.rom_sha256.clone(),
            })
        }
        BootKind::BootTo => {
            let map = req.map.ok_or_else(|| {
                EmuToolError::new("invalid_argument", "boot_to requires 'map'")
            })?;
            let x = req.x.ok_or_else(|| {
                EmuToolError::new("invalid_argument", "boot_to requires 'x'")
            })?;
            let y = req.y.ok_or_else(|| {
                EmuToolError::new("invalid_argument", "boot_to requires 'y'")
            })?;
            let hook = session.boot_hook.as_mut().ok_or_else(|| {
                EmuToolError::new(
                    "no_boot_hook",
                    "boot_to requires a game-specific boot hook to be attached",
                )
            })?;
            let frames = hook
                .boot_to(session.emu.as_mut(), &map, x, y, &req.flags)
                .map_err(|e| EmuToolError::new("boot_failed", e))?;
            Ok(EmuBootResult {
                ok: true,
                mode: format!("boot_to:{map}({x},{y})"),
                frame_count: frames,
                rom_sha256: session.rom_sha256.clone(),
            })
        }
    }
}

pub fn emu_press_input(
    session: &mut EmuSession,
    req: EmuPressInputRequest,
) -> Result<EmuPressInputResult, EmuToolError> {
    if session.rom.is_none() {
        return Err(EmuToolError::new("no_rom", "no ROM is loaded in the emulator"));
    }
    if req.frames == 0 && req.release_frames == 0 {
        return Err(EmuToolError::new(
            "invalid_argument",
            "frames or release_frames must be > 0",
        ));
    }
    let held = buttons_from_slice(&req.buttons);
    session.emu.set_buttons(held);
    for _ in 0..req.frames {
        session
            .emu
            .run_frame()
            .map_err(|e| EmuToolError::new("emu_error", e.to_string()))?;
    }
    if req.release_frames > 0 {
        session.emu.set_buttons(Buttons::NONE);
        for _ in 0..req.release_frames {
            session
                .emu
                .run_frame()
                .map_err(|e| EmuToolError::new("emu_error", e.to_string()))?;
        }
    }
    Ok(EmuPressInputResult {
        ok: true,
        frame_count: session.emu.frame_count(),
        frames_advanced: req.frames + req.release_frames,
    })
}

pub fn emu_screenshot(
    session: &mut EmuSession,
    req: EmuScreenshotRequest,
) -> Result<EmuScreenshotResult, EmuToolError> {
    if session.rom.is_none() {
        return Err(EmuToolError::new("no_rom", "no ROM is loaded in the emulator"));
    }
    let frame = session.emu.frame();
    let png_bytes = encode_png(&frame)?;
    let png_sha256 = sha256_hex(&png_bytes);
    let saved_path = if let Some(path_str) = req.save_path {
        let path = Path::new(&path_str);
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .map_err(|e| EmuToolError::new("io_error", e.to_string()))?;
            }
        }
        fs::write(path, &png_bytes).map_err(|e| EmuToolError::new("io_error", e.to_string()))?;
        Some(path_str)
    } else {
        None
    };
    let png_base64 = req
        .include_base64
        .then(|| base64::engine::general_purpose::STANDARD.encode(&png_bytes));
    Ok(EmuScreenshotResult {
        ok: true,
        width: frame.width,
        height: frame.height,
        frame_count: session.emu.frame_count(),
        png_sha256,
        saved_path,
        png_base64,
    })
}

pub fn emu_save_state(
    session: &mut EmuSession,
    req: EmuStateRequest,
) -> Result<EmuStateResult, EmuToolError> {
    let rom_sha256 = session
        .rom_sha256
        .clone()
        .ok_or_else(|| EmuToolError::new("no_rom", "no ROM is loaded in the emulator"))?;
    let bytes = session
        .emu
        .save_state()
        .map_err(|e| EmuToolError::new("state_error", e.to_string()))?;
    let byte_len = bytes.len();
    let frame = session.emu.frame_count();
    session.savestates.insert(
        req.name.clone(),
        SavedSlot {
            rom_sha256: rom_sha256.clone(),
            bytes,
        },
    );
    Ok(EmuStateResult {
        ok: true,
        name: req.name,
        frame_count: frame,
        byte_len,
        rom_sha256,
    })
}

pub fn emu_load_state(
    session: &mut EmuSession,
    req: EmuStateRequest,
) -> Result<EmuStateResult, EmuToolError> {
    let current_sha = session
        .rom_sha256
        .clone()
        .ok_or_else(|| EmuToolError::new("no_rom", "no ROM is loaded in the emulator"))?;
    let slot = session.savestates.get(&req.name).ok_or_else(|| {
        EmuToolError::new("not_found", format!("unknown savestate '{}'", req.name))
    })?;
    if slot.rom_sha256 != current_sha {
        return Err(EmuToolError::new(
            "rom_mismatch",
            format!(
                "savestate '{}' belongs to ROM {}, but loaded ROM is {}",
                req.name, slot.rom_sha256, current_sha
            ),
        ));
    }
    let bytes = slot.bytes.clone();
    let byte_len = bytes.len();
    session
        .emu
        .load_state(&bytes)
        .map_err(|e| EmuToolError::new("state_error", e.to_string()))?;
    Ok(EmuStateResult {
        ok: true,
        name: req.name,
        frame_count: session.emu.frame_count(),
        byte_len,
        rom_sha256: current_sha,
    })
}

pub fn emu_assert_ram(
    session: &EmuSession,
    req: EmuAssertRamRequest,
) -> Result<EmuAssertRamResult, EmuToolError> {
    if session.rom.is_none() {
        return Err(EmuToolError::new("no_rom", "no ROM is loaded in the emulator"));
    }
    if looks_like_raw_address(&req.symbol) {
        return Err(EmuToolError::new(
            "invalid_argument",
            format!("raw addresses are forbidden ('{}'); supply a symbol name", req.symbol),
        ));
    }
    if is_save_block_pointer(&req.symbol) && !req.deref {
        return Err(EmuToolError::new(
            "invalid_argument",
            format!(
                "save-block pointer '{}' must be read with deref: true",
                req.symbol
            ),
        ));
    }
    let (addr, observed) = read_typed_value(
        session.emu.as_ref(),
        &session.symbols,
        &req.symbol,
        req.deref,
        req.offset,
        req.value_type,
        req.length,
        req.flag_bit,
        0,
    )?;
    let expected = resolve_expected(&req.expected, &session.constants)?;
    let passed = compare_values(&observed, req.op, &expected)?;
    Ok(EmuAssertRamResult {
        ok: true,
        passed,
        symbol: req.symbol,
        resolved_address: format!("0x{addr:08x}"),
        observed,
        expected,
        op: req.op,
    })
}

/// Runs a `VER-001` verification spec: loader rejection rules -> assertion-free
/// driver sampling -> evaluator + counterfactuals -> verdict.
pub fn run_verification(
    session: &mut EmuSession,
    spec: VerificationSpec,
) -> Result<VerificationReport, EmuToolError> {
    let rejection_reasons = validate_spec(session, &spec);
    if !rejection_reasons.is_empty() {
        return Ok(VerificationReport {
            ok: false,
            name: spec.name,
            verdict: Verdict::Rejected,
            rejection_reasons,
            rom_sha256: session.rom_sha256.clone(),
            sym_sha256: session.symbols.sha256().map(str::to_owned),
            assertions: Vec::new(),
            counterfactuals: Vec::new(),
        });
    }

    // Preserve the caller's live emulator state so verification does not clobber it.
    let saved_live = session.emu.save_state().ok();

    // 1. Real run (driver never sees spec.assertions).
    let real_samples = drive_spec(session, &spec.start, &spec.timeline, &spec.probes, 0, false)?;
    let real_outcomes = evaluate_assertions(&spec, &real_samples, &session.constants, false)?;
    let real_passed = real_outcomes.iter().all(|a| a.passed);

    let mut cf_outcomes = Vec::new();
    let verdict = if !real_passed {
        Verdict::Failed
    } else {
        for &kind in &spec.counterfactuals {
            let outcome = run_counterfactual(session, &spec, kind)?;
            cf_outcomes.push(outcome);
        }
        let all_cf_failed = cf_outcomes
            .iter()
            .all(|cf| cf.failed_as_expected && cf.discriminating_failed);
        if all_cf_failed {
            Verdict::Proven
        } else {
            Verdict::Inconclusive
        }
    };

    if let Some(bytes) = saved_live {
        let _ = session.emu.load_state(&bytes);
    }

    Ok(VerificationReport {
        ok: verdict == Verdict::Proven,
        name: spec.name,
        verdict,
        rejection_reasons: Vec::new(),
        rom_sha256: session.rom_sha256.clone(),
        sym_sha256: session.symbols.sha256().map(str::to_owned),
        assertions: real_outcomes,
        counterfactuals: cf_outcomes,
    })
}

fn validate_spec(session: &EmuSession, spec: &VerificationSpec) -> Vec<String> {
    let mut reasons = Vec::new();
    if spec.schema != 1 {
        reasons.push(format!("unsupported schema {}, expected 1", spec.schema));
    }
    if session.rom.is_none() {
        reasons.push("no ROM is loaded in the emulator".to_owned());
    }
    // Rule 1: assert non-empty and not all supporting.
    let probe_map: BTreeMap<&str, &ProbeSpec> =
        spec.probes.iter().map(|p| (p.id.as_str(), p)).collect();
    if spec.assertions.is_empty() {
        reasons.push("rule 1: assert list must not be empty".to_owned());
    } else if spec.assertions.iter().all(|a| {
        a.supporting || probe_map.get(a.probe.as_str()).is_some_and(|p| p.supporting)
    }) {
        reasons.push("rule 1: at least one non-supporting assertion is required".to_owned());
    }

    // Rule 2 & 3: probe why, no raw addresses, save-block pointer deref.
    for probe in &spec.probes {
        if probe.why.trim().is_empty() {
            reasons.push(format!("rule 2: probe '{}' must include a non-empty 'why'", probe.id));
        }
        if looks_like_raw_address(&probe.symbol) {
            reasons.push(format!(
                "rule 2: probe '{}' uses raw address '{}' instead of a symbol",
                probe.id, probe.symbol
            ));
        } else if session.symbols.get(&probe.symbol).is_none() {
            reasons.push(format!(
                "rule 2: probe '{}' references unknown symbol '{}'",
                probe.id, probe.symbol
            ));
        }
        if is_save_block_pointer(&probe.symbol) && !probe.deref {
            reasons.push(format!(
                "rule 2/3: save-block pointer '{}' on probe '{}' requires deref: true",
                probe.symbol, probe.id
            ));
        }
    }

    // Rule 5: savestate start ROM hash must match loaded ROM.
    if let SpecStart::Savestate { name, rom_sha256 } = &spec.start {
        if let Some(loaded) = &session.rom_sha256 {
            if loaded != rom_sha256 {
                reasons.push(format!(
                    "rule 5: savestate '{}' expects ROM {rom_sha256}, loaded ROM is {loaded}",
                    name
                ));
            }
        }
        if !session.savestates.contains_key(name) {
            reasons.push(format!("rule 5: savestate '{}' is not loaded", name));
        }
    }

    // Rule 6: counterfactuals non-empty and applicable.
    if spec.counterfactuals.is_empty() {
        reasons.push("rule 6: at least one counterfactual is required".to_owned());
    }
    for &cf in &spec.counterfactuals {
        if cf == CounterfactualKind::ColdBoot && !matches!(spec.start, SpecStart::Savestate { .. })
        {
            reasons.push("rule 6: cold_boot counterfactual applies only to savestate specs".to_owned());
        }
        if cf == CounterfactualKind::BaseRom && session.base_rom.is_none() {
            reasons.push("rule 6: base_rom counterfactual requires a configured base ROM".to_owned());
        }
    }

    // Rule 7: every asserted probe must exist and have samples covering its target.
    let step_names: Vec<&str> = spec.timeline.iter().map(|s| s.name.as_str()).collect();
    for assertion in &spec.assertions {
        let Some(probe) = probe_map.get(assertion.probe.as_str()) else {
            reasons.push(format!(
                "rule 7: assertion references unknown probe '{}'",
                assertion.probe
            ));
            continue;
        };
        let has_any_schedule = probe.sample.every.is_some_and(|e| e > 0)
            || !probe.sample.at_step_start.is_empty()
            || !probe.sample.at_step_end.is_empty();
        if !has_any_schedule {
            reasons.push(format!(
                "rule 7: probe '{}' has no sampling schedule",
                probe.id
            ));
        }
        if let Some(at) = &assertion.at {
            if at != "first" && at != "last" && at != "all" && at != "any" {
                if !step_names.contains(&at.as_str()) {
                    reasons.push(format!(
                        "rule 7: assertion on '{}' targets unknown step '{at}'",
                        assertion.probe
                    ));
                } else if !probe.sample.at_step_end.iter().any(|s| s == at)
                    && probe.sample.every.is_none()
                {
                    reasons.push(format!(
                        "rule 7: probe '{}' is not sampled at step '{at}'",
                        probe.id
                    ));
                }
            }
        }
    }

    reasons
}

/// Assertion-free emulator driver: only sees start, timeline and probes.
fn drive_spec(
    session: &mut EmuSession,
    start: &SpecStart,
    timeline: &[TimelineStep],
    probes: &[ProbeSpec],
    shift_bytes: i32,
    use_base_rom: bool,
) -> Result<Vec<SampleRecord>, EmuToolError> {
    let original_rom = if use_base_rom {
        let base = session
            .base_rom
            .clone()
            .ok_or_else(|| EmuToolError::new("no_base_rom", "base_rom not configured"))?;
        let orig = session.rom.clone();
        session
            .emu
            .load_rom(&base)
            .map_err(|e| EmuToolError::new("emu_error", e.to_string()))?;
        orig
    } else {
        None
    };

    let run_result = (|| -> Result<Vec<SampleRecord>, EmuToolError> {
        match start {
            SpecStart::ColdBoot => {
                session
                    .emu
                    .reset()
                    .map_err(|e| EmuToolError::new("emu_error", e.to_string()))?;
            }
            SpecStart::Savestate { name, .. } => {
                let bytes = session
                    .savestates
                    .get(name)
                    .ok_or_else(|| EmuToolError::new("not_found", format!("missing state {name}")))?
                    .bytes
                    .clone();
                session
                    .emu
                    .load_state(&bytes)
                    .map_err(|e| EmuToolError::new("state_error", e.to_string()))?;
            }
            SpecStart::BootTo { map, x, y, flags } => {
                let hook = session
                    .boot_hook
                    .as_mut()
                    .ok_or_else(|| EmuToolError::new("no_boot_hook", "no boot hook attached"))?;
                hook.boot_to(session.emu.as_mut(), map, *x, *y, flags)
                    .map_err(|e| EmuToolError::new("boot_failed", e))?;
            }
        }

        let mut samples = Vec::new();
        let mut elapsed: u32 = 0;
        for step in timeline {
            for probe in probes {
                if probe.sample.at_step_start.iter().any(|s| s == &step.name) {
                    let (_, val) = read_typed_value(
                        session.emu.as_ref(),
                        &session.symbols,
                        &probe.symbol,
                        probe.deref,
                        probe.offset,
                        probe.value_type,
                        probe.length,
                        probe.flag_bit,
                        shift_bytes,
                    )?;
                    samples.push(SampleRecord {
                        probe_id: probe.id.clone(),
                        step_end: None,
                        value: val,
                    });
                }
            }

            let policy = step.policy.as_deref();
            if policy.is_none() {
                session.emu.set_buttons(buttons_from_slice(&step.keys));
            }
            for frame_idx in 0..step.frames {
                if let Some(pol) = policy {
                    let buttons = match pol {
                        "lose-fast" | "mash-a" => {
                            if (frame_idx % 20) < 6 {
                                buttons_from_slice(&[ButtonInput::A])
                            } else {
                                Buttons::default()
                            }
                        }
                        other => {
                            return Err(EmuToolError::new(
                                "invalid_policy",
                                format!("unsupported timeline policy `{other}`"),
                            ));
                        }
                    };
                    session.emu.set_buttons(buttons);
                }
                session
                    .emu
                    .run_frame()
                    .map_err(|e| EmuToolError::new("emu_error", e.to_string()))?;
                elapsed += 1;
                for probe in probes {
                    if let Some(every) = probe.sample.every {
                        if every > 0 && elapsed.is_multiple_of(every) {
                            let (_, val) = read_typed_value(
                                session.emu.as_ref(),
                                &session.symbols,
                                &probe.symbol,
                                probe.deref,
                                probe.offset,
                                probe.value_type,
                                probe.length,
                                probe.flag_bit,
                                shift_bytes,
                            )?;
                            samples.push(SampleRecord {
                                probe_id: probe.id.clone(),
                                step_end: None,
                                value: val,
                            });
                        }
                    }
                }
            }

            for probe in probes {
                if probe.sample.at_step_end.iter().any(|s| s == &step.name) {
                    let (_, val) = read_typed_value(
                        session.emu.as_ref(),
                        &session.symbols,
                        &probe.symbol,
                        probe.deref,
                        probe.offset,
                        probe.value_type,
                        probe.length,
                        probe.flag_bit,
                        shift_bytes,
                    )?;
                    samples.push(SampleRecord {
                        probe_id: probe.id.clone(),
                        step_end: Some(step.name.clone()),
                        value: val,
                    });
                }
            }
        }
        Ok(samples)
    })();

    if let Some(orig) = original_rom {
        let _ = session.emu.load_rom(&orig);
    }
    run_result
}

fn run_counterfactual(
    session: &mut EmuSession,
    spec: &VerificationSpec,
    kind: CounterfactualKind,
) -> Result<CounterfactualOutcome, EmuToolError> {
    let outcomes = match kind {
        CounterfactualKind::Shift => {
            let samples = drive_spec(session, &spec.start, &spec.timeline, &spec.probes, 16, false)?;
            evaluate_assertions(spec, &samples, &session.constants, false)?
        }
        CounterfactualKind::WrongValue => {
            let samples = drive_spec(session, &spec.start, &spec.timeline, &spec.probes, 0, false)?;
            evaluate_assertions(spec, &samples, &session.constants, true)?
        }
        CounterfactualKind::ColdBoot => {
            let samples = drive_spec(
                session,
                &SpecStart::ColdBoot,
                &spec.timeline,
                &spec.probes,
                0,
                false,
            )?;
            evaluate_assertions(spec, &samples, &session.constants, false)?
        }
        CounterfactualKind::BaseRom => {
            let samples = drive_spec(session, &spec.start, &spec.timeline, &spec.probes, 0, true)?;
            evaluate_assertions(spec, &samples, &session.constants, false)?
        }
    };
    let failed_as_expected = outcomes.iter().any(|a| !a.passed);
    let discriminating_failed = outcomes.iter().any(|a| a.discriminating && !a.passed);
    Ok(CounterfactualOutcome {
        kind,
        failed_as_expected,
        discriminating_failed,
        assertions: outcomes,
    })
}

fn evaluate_assertions(
    spec: &VerificationSpec,
    samples: &[SampleRecord],
    constants: &BTreeMap<String, i64>,
    nudge_expected: bool,
) -> Result<Vec<AssertionOutcome>, EmuToolError> {
    let probe_map: BTreeMap<&str, &ProbeSpec> =
        spec.probes.iter().map(|p| (p.id.as_str(), p)).collect();
    let mut outcomes = Vec::with_capacity(spec.assertions.len());

    for assertion in &spec.assertions {
        let probe_samples: Vec<&SampleRecord> = samples
            .iter()
            .filter(|s| s.probe_id == assertion.probe)
            .collect();
        let is_supporting = assertion.supporting
            || probe_map
                .get(assertion.probe.as_str())
                .is_some_and(|p| p.supporting);

        match assertion.check.as_str() {
            "compare" => {
                let op = assertion.op.ok_or_else(|| {
                    EmuToolError::new("invalid_spec", "compare assertion requires 'op'")
                })?;
                let expected_raw = assertion.value.as_ref().ok_or_else(|| {
                    EmuToolError::new("invalid_spec", "compare assertion requires 'value'")
                })?;
                let mut expected = resolve_expected(expected_raw, constants)?;
                let discriminating = !is_supporting && is_discriminating_compare(op, &expected);
                if nudge_expected {
                    expected = nudge_value(expected);
                }
                let selected = select_samples(&probe_samples, assertion.at.as_deref());
                let passed = !selected.is_empty()
                    && selected
                        .iter()
                        .all(|s| compare_values(&s.value, op, &expected).unwrap_or(false));
                let detail = if selected.is_empty() {
                    "no matching samples".to_owned()
                } else {
                    format!(
                        "observed {:?} {:?} expected {:?} ({} samples)",
                        selected.last().unwrap().value,
                        op,
                        expected,
                        selected.len()
                    )
                };
                outcomes.push(AssertionOutcome {
                    probe: assertion.probe.clone(),
                    check: assertion.check.clone(),
                    passed,
                    discriminating,
                    detail,
                });
            }
            "strictly_increases" => {
                let ints: Vec<i64> = probe_samples
                    .iter()
                    .filter_map(|s| match s.value {
                        ObservedValue::Int(v) => Some(v),
                        _ => None,
                    })
                    .collect();
                let passed = ints.len() >= 2 && ints.windows(2).all(|w| w[1] > w[0]);
                outcomes.push(AssertionOutcome {
                    probe: assertion.probe.clone(),
                    check: assertion.check.clone(),
                    passed,
                    discriminating: false,
                    detail: format!("{} integer samples", ints.len()),
                });
            }
            "constant" => {
                let passed = !probe_samples.is_empty()
                    && probe_samples
                        .windows(2)
                        .all(|w| w[0].value == w[1].value);
                outcomes.push(AssertionOutcome {
                    probe: assertion.probe.clone(),
                    check: assertion.check.clone(),
                    passed,
                    discriminating: false,
                    detail: format!("{} samples", probe_samples.len()),
                });
            }
            other => {
                return Err(EmuToolError::new(
                    "invalid_spec",
                    format!("unsupported assertion check '{other}'"),
                ));
            }
        }
    }
    Ok(outcomes)
}

fn select_samples<'a>(
    probe_samples: &[&'a SampleRecord],
    at: Option<&str>,
) -> Vec<&'a SampleRecord> {
    match at.unwrap_or("last") {
        "first" => probe_samples.first().copied().into_iter().collect(),
        "last" => probe_samples.last().copied().into_iter().collect(),
        "all" => probe_samples.to_vec(),
        step_name => probe_samples
            .iter()
            .copied()
            .filter(|s| s.step_end.as_deref() == Some(step_name))
            .collect(),
    }
}

fn is_discriminating_compare(op: CompareOp, expected: &ObservedValue) -> bool {
    if op != CompareOp::Eq {
        return false;
    }
    match expected {
        ObservedValue::Int(v) => *v != 0 && *v != 0xFF && *v != 0xFFFF,
        ObservedValue::Bytes(b) => !b.is_empty() && b.iter().any(|&x| x != 0 && x != 0xFF),
    }
}

fn nudge_value(v: ObservedValue) -> ObservedValue {
    match v {
        ObservedValue::Int(x) => ObservedValue::Int(x.wrapping_add(1)),
        ObservedValue::Bytes(mut b) => {
            if let Some(first) = b.first_mut() {
                *first = first.wrapping_add(1);
            }
            ObservedValue::Bytes(b)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn read_typed_value(
    emu: &dyn Emulator,
    symbols: &SymbolTable,
    symbol: &str,
    deref: bool,
    offset: u32,
    value_type: ValueType,
    length: Option<usize>,
    flag_bit: Option<u32>,
    shift_bytes: i32,
) -> Result<(u32, ObservedValue), EmuToolError> {
    let sym_addr = symbols.get(symbol).ok_or_else(|| {
        EmuToolError::new("unknown_symbol", format!("symbol '{symbol}' not in symbol table"))
    })?;
    let base = if deref {
        emu.read_u32(sym_addr)
            .map_err(|e| EmuToolError::new("memory_error", e.to_string()))?
    } else {
        sym_addr
    };
    let raw_addr = base.wrapping_add(offset);
    let addr = apply_shift_in_region(emu, raw_addr, shift_bytes);

    let value = match value_type {
        ValueType::U8 => {
            let b = emu
                .read_memory(addr, 1)
                .map_err(|e| EmuToolError::new("memory_error", e.to_string()))?;
            ObservedValue::Int(b[0] as i64)
        }
        ValueType::U16 => {
            let v = emu
                .read_u16(addr)
                .map_err(|e| EmuToolError::new("memory_error", e.to_string()))?;
            ObservedValue::Int(v as i64)
        }
        ValueType::U32 | ValueType::Ptr => {
            let v = emu
                .read_u32(addr)
                .map_err(|e| EmuToolError::new("memory_error", e.to_string()))?;
            ObservedValue::Int(v as i64)
        }
        ValueType::S8 => {
            let b = emu
                .read_memory(addr, 1)
                .map_err(|e| EmuToolError::new("memory_error", e.to_string()))?;
            ObservedValue::Int((b[0] as i8) as i64)
        }
        ValueType::S16 => {
            let b = emu
                .read_memory(addr, 2)
                .map_err(|e| EmuToolError::new("memory_error", e.to_string()))?;
            ObservedValue::Int(i16::from_le_bytes([b[0], b[1]]) as i64)
        }
        ValueType::S32 => {
            let b = emu
                .read_memory(addr, 4)
                .map_err(|e| EmuToolError::new("memory_error", e.to_string()))?;
            ObservedValue::Int(i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as i64)
        }
        ValueType::Bytes => {
            let len = length.ok_or_else(|| {
                EmuToolError::new("invalid_argument", "bytes type requires 'length'")
            })?;
            let b = emu
                .read_memory(addr, len)
                .map_err(|e| EmuToolError::new("memory_error", e.to_string()))?;
            ObservedValue::Bytes(b)
        }
        ValueType::FlagBit => {
            let bit = flag_bit.ok_or_else(|| {
                EmuToolError::new("invalid_argument", "flag_bit type requires 'flag_bit'")
            })?;
            let byte_addr = addr.wrapping_add(bit / 8);
            let b = emu
                .read_memory(byte_addr, 1)
                .map_err(|e| EmuToolError::new("memory_error", e.to_string()))?;
            let is_set = ((b[0] >> (bit % 8)) & 1) as i64;
            ObservedValue::Int(is_set)
        }
    };
    Ok((addr, value))
}

fn apply_shift_in_region(emu: &dyn Emulator, addr: u32, shift: i32) -> u32 {
    if shift == 0 {
        return addr;
    }
    let shifted = (addr as i64 + shift as i64) as u32;
    for region in emu.memory_regions() {
        if region.contains(addr, 1) {
            if region.contains(shifted, 4) {
                return shifted;
            }
            let alt = (addr as i64 - shift as i64) as u32;
            if region.contains(alt, 4) {
                return alt;
            }
        }
    }
    shifted
}

fn resolve_expected(
    expected: &ExpectedValue,
    constants: &BTreeMap<String, i64>,
) -> Result<ObservedValue, EmuToolError> {
    match expected {
        ExpectedValue::Int(v) => Ok(ObservedValue::Int(*v)),
        ExpectedValue::Bytes(b) => Ok(ObservedValue::Bytes(b.clone())),
        ExpectedValue::Constant(name) => {
            if let Some(&val) = constants.get(name) {
                return Ok(ObservedValue::Int(val));
            }
            if let Some(hex) = name.strip_prefix("0x").or_else(|| name.strip_prefix("0X")) {
                if let Ok(v) = i64::from_str_radix(hex, 16) {
                    return Ok(ObservedValue::Int(v));
                }
            }
            Err(EmuToolError::new(
                "unknown_constant",
                format!("unknown constant '{name}'"),
            ))
        }
    }
}

fn compare_values(
    observed: &ObservedValue,
    op: CompareOp,
    expected: &ObservedValue,
) -> Result<bool, EmuToolError> {
    match (observed, expected) {
        (ObservedValue::Int(a), ObservedValue::Int(b)) => Ok(match op {
            CompareOp::Eq => a == b,
            CompareOp::Ne => a != b,
            CompareOp::Lt => a < b,
            CompareOp::Le => a <= b,
            CompareOp::Gt => a > b,
            CompareOp::Ge => a >= b,
        }),
        (ObservedValue::Bytes(a), ObservedValue::Bytes(b)) => match op {
            CompareOp::Eq => Ok(a == b),
            CompareOp::Ne => Ok(a != b),
            _ => Err(EmuToolError::new(
                "invalid_argument",
                "ordered comparison operators are not valid for byte arrays",
            )),
        },
        _ => Err(EmuToolError::new(
            "type_mismatch",
            "observed and expected RAM value types do not match",
        )),
    }
}

fn looks_like_raw_address(symbol: &str) -> bool {
    let s = symbol.trim();
    s.starts_with("0x") || s.starts_with("0X") || s.chars().all(|c| c.is_ascii_digit())
}

fn is_save_block_pointer(symbol: &str) -> bool {
    symbol.contains("SaveBlock") && symbol.ends_with("Ptr")
}

fn encode_png(frame: &Frame) -> Result<Vec<u8>, EmuToolError> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, frame.width, frame.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| EmuToolError::new("png_error", e.to_string()))?;
        writer
            .write_image_data(&frame.rgba)
            .map_err(|e| EmuToolError::new("png_error", e.to_string()))?;
    }
    Ok(out)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
