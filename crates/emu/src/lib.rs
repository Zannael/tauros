//! Layer 5: the emulator connector. It owns the emulator trait (so a melonDS
//! implementation can follow) and the libmgba-backed implementation (EMU-001, per
//! DEC-002). It depends on no other Tauros crate.
//!
//! The trait is deliberately small and synchronous: the caller owns the loop and
//! decides when to run a frame, what to show and where audio goes. Pacing, threads
//! and windows belong to the app (EMU-004), savestates and memory to EMU-002.

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmuError {
    /// The core could not be created or initialised.
    Core(String),
    /// The bytes are not a ROM this core runs, or loading them failed.
    Rom(String),
    /// An operation needs a loaded ROM.
    NoRom,
}

impl fmt::Display for EmuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EmuError::Core(m) => write!(f, "emulator core: {m}"),
            EmuError::Rom(m) => write!(f, "ROM: {m}"),
            EmuError::NoRom => write!(f, "no ROM loaded"),
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
}
