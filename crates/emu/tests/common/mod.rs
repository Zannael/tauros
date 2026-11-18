//! Shared test helpers: the probe cartridge and the user-built base ROM.

#![allow(dead_code)]

use std::path::PathBuf;
use tauros_emu::{Emulator, Frame};

/// tests/probe/probe.s assembled by devkitARM: `b start` at 0, game code and the
/// fixed 0x96 byte in the header, code and literal pool from 0xC0.
pub fn probe_rom() -> Vec<u8> {
    const CODE: [u32; 30] = [
        0xe3a00301, 0xe3a01080, 0xe1c018b4, 0xe59f104c, 0xe1c018b0, 0xe3a01002, 0xe1c018b2, 0xe59f1040, 0xe1c016b2,
        0xe59f103c, 0xe1c016b4, 0xe59f1038, 0xe1c010b0, 0xe2807e13, 0xe1d730b0, 0xe3130001, 0x059f4028, 0x159f4028,
        0xe3a06406, 0xe3a05b4b, 0xe4864004, 0xe2555004, 0x1afffffc, 0xeafffff5, 0x00001177, 0x0000f080, 0x000086d6,
        0x00000403, 0x001f001f, 0x7c007c00,
    ];
    let mut rom = vec![0u8; 0xC0];
    rom[..4].copy_from_slice(&0xea00002eu32.to_le_bytes());
    rom[0xAC..0xB2].copy_from_slice(b"TAUR01");
    rom[0xB2] = 0x96;
    rom.extend(CODE.iter().flat_map(|w| w.to_le_bytes()));
    rom
}

/// The single colour of a frame, or `None` if it has more than one.
pub fn uniform(frame: &Frame) -> Option<[u8; 3]> {
    let first = frame.pixel(0, 0);
    frame.rgba.chunks(4).all(|p| p[..3] == first).then_some(first)
}

/// The FND-002 base build directory (pokeemerald-expansion built by the user).
pub fn base_dir() -> PathBuf {
    let data = std::env::var_os("TAUROS_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap()).join(".local/share/tauros"));
    data.join("engines/pokeemerald-expansion-expansion-1.16.4-base")
}

/// The FND-002 base ROM (pokeemerald-expansion built by the user), if present.
pub fn base_rom() -> Option<Vec<u8>> {
    let path = std::env::var_os("TAUROS_BASE_ROM").map(PathBuf::from).unwrap_or_else(|| base_dir().join("pokeemerald.gba"));
    match std::fs::read(&path) {
        Ok(rom) => Some(rom),
        Err(_) => {
            eprintln!("SKIPPED: no base ROM at {}; run `python3 scripts/setup_toolchain.py`", path.display());
            None
        }
    }
}

/// Address of a symbol in the base build's `.sym` (`addr scope size name`).
pub fn symbol(name: &str) -> u32 {
    let syms = std::fs::read_to_string(base_dir().join("pokeemerald.sym")).expect("base build .sym");
    syms.lines()
        .find_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            (f.len() == 4 && f[3] == name).then(|| u32::from_str_radix(f[0], 16).unwrap())
        })
        .unwrap_or_else(|| panic!("symbol {name} not in the base build"))
}

/// Runs `frames` frames, draining audio into `audio`.
pub fn run(emu: &mut impl Emulator, frames: u32, audio: &mut Vec<i16>) {
    for _ in 0..frames {
        emu.run_frame().unwrap();
        emu.drain_audio(audio);
    }
}
