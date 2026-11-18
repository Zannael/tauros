//! Layer 5: the emulator connector. It owns the emulator trait (so a melonDS
//! implementation can follow) and the libmgba-backed implementation (EMU-001, per
//! DEC-002). It depends on no other Tauros crate.
//!
//! The trait is deliberately small and synchronous: the caller owns the loop and
//! decides when to run a frame, what to show and where audio goes. Pacing, threads
//! and windows belong to the app (EMU-004). Savestates and direct memory access
//! (EMU-002) let tests and agent tools check the game through RAM instead of pixels.

mod mgba;

pub use mgba::Mgba;

use std::fmt;

/// A console button. GBA and GB use a subset; NDS adds X, Y and the touch screen
/// (not modelled until a melonDS implementation exists).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Button {
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

impl Button {
    pub const ALL: [Button; 10] = [
        Button::A,
        Button::B,
        Button::Select,
        Button::Start,
        Button::Right,
        Button::Left,
        Button::Up,
        Button::Down,
        Button::R,
        Button::L,
    ];
}

/// The set of buttons held down.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Buttons(u16);

impl Buttons {
    pub const NONE: Buttons = Buttons(0);

    pub fn with(self, b: Button) -> Self {
        Buttons(self.0 | 1 << b as u16)
    }

    pub fn contains(self, b: Button) -> bool {
        self.0 & 1 << b as u16 != 0
    }

    pub fn iter(self) -> impl Iterator<Item = Button> {
        Button::ALL.into_iter().filter(move |b| self.contains(*b))
    }
}

impl FromIterator<Button> for Buttons {
    fn from_iter<I: IntoIterator<Item = Button>>(iter: I) -> Self {
        iter.into_iter().fold(Buttons::NONE, Buttons::with)
    }
}

/// One video frame as 8-bit RGBA, row-major, alpha always 255.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Frame {
    /// The `[r, g, b]` of the pixel at `(x, y)`.
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 3] {
        let i = 4 * (y * self.width + x) as usize;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2]]
    }
}

/// A mapped region of the console's address space.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryRegion {
    /// Short stable identifier (`wram`, `iwram`, `io`, `cart0`, ...).
    pub id: String,
    /// Human-readable name (`EWRAM`, `IWRAM`, `MMIO`, `ROM`, ...).
    pub name: String,
    pub start: u32,
    pub size: u32,
    pub writable: bool,
}

impl MemoryRegion {
    /// Whether `[addr, addr + len)` lies inside this region.
    pub fn contains(&self, addr: u32, len: usize) -> bool {
        addr >= self.start && (addr - self.start) as u64 + len as u64 <= self.size as u64
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmuError {
    /// The core could not be created or initialised.
    Core(String),
    /// The bytes are not a ROM this core runs, or loading them failed.
    Rom(String),
    /// An operation needs a loaded ROM.
    NoRom,
    /// A memory access outside a mapped region, or a write to a read-only one.
    Memory(String),
    /// A savestate could not be taken or restored.
    State(String),
}

impl fmt::Display for EmuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EmuError::Core(m) => write!(f, "emulator core: {m}"),
            EmuError::Rom(m) => write!(f, "ROM: {m}"),
            EmuError::NoRom => write!(f, "no ROM loaded"),
            EmuError::Memory(m) => write!(f, "memory: {m}"),
            EmuError::State(m) => write!(f, "savestate: {m}"),
        }
    }
}

impl std::error::Error for EmuError {}

/// An emulator core driven one frame at a time.
pub trait Emulator {
    /// Loads a ROM image and resets the console. The bytes are copied.
    fn load_rom(&mut self, rom: &[u8]) -> Result<(), EmuError>;

    /// Resets the console with the loaded ROM.
    fn reset(&mut self) -> Result<(), EmuError>;

    /// Runs until the next frame is complete, with `set_buttons`' buttons held.
    fn run_frame(&mut self) -> Result<(), EmuError>;

    /// Frames emulated since the last reset.
    fn frame_count(&self) -> u32;

    /// Sets the buttons held from the next frame on.
    fn set_buttons(&mut self, buttons: Buttons);

    /// The last completed frame.
    fn frame(&self) -> Frame;

    /// Output sample rate of `drain_audio`, in Hz.
    fn audio_rate(&self) -> u32;

    /// Appends the audio produced since the last call to `out` as interleaved
    /// stereo `i16` samples (left, right) and returns the number of stereo frames.
    fn drain_audio(&mut self, out: &mut Vec<i16>) -> usize;

    /// The mapped regions of the address space.
    fn memory_regions(&self) -> Vec<MemoryRegion>;

    /// Reads `len` bytes at `addr`, without the side effects of a CPU read. The
    /// range must lie inside one region.
    fn read_memory(&self, addr: u32, len: usize) -> Result<Vec<u8>, EmuError>;

    /// Writes `bytes` at `addr` inside one writable region.
    fn write_memory(&mut self, addr: u32, bytes: &[u8]) -> Result<(), EmuError>;

    /// The whole console state, restorable with `load_state` on a core running the
    /// same ROM.
    fn save_state(&mut self) -> Result<Vec<u8>, EmuError>;

    /// Restores a state taken by `save_state`.
    fn load_state(&mut self, state: &[u8]) -> Result<(), EmuError>;

    /// Reads a little-endian `u32`.
    fn read_u32(&self, addr: u32) -> Result<u32, EmuError> {
        Ok(u32::from_le_bytes(self.read_memory(addr, 4)?.try_into().unwrap()))
    }

    /// Reads a little-endian `u16`.
    fn read_u16(&self, addr: u32) -> Result<u16, EmuError> {
        Ok(u16::from_le_bytes(self.read_memory(addr, 2)?.try_into().unwrap()))
    }
}
