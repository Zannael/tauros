//! EMU-002: savestates and memory access through the `Emulator` trait.
//!
//! A restored state must replay to the same framebuffer and RAM as the original
//! run, even after a detour with different input; reads of known addresses (the
//! probe's header and registers, the base build's symbols) must match.

mod common;

use common::{base_rom, probe_rom, run, symbol, uniform};
use tauros_emu::{Button, Buttons, EmuError, Emulator, Frame, Mgba};

const EWRAM: u32 = 0x0200_0000;
const IWRAM: u32 = 0x0300_0000;
const VRAM: u32 = 0x0600_0000;

/// What a replay must reproduce: the screen and the RAM the game runs in.
#[derive(PartialEq, Eq)]
struct Snapshot {
    frame: Frame,
    ewram: Vec<u8>,
    iwram: Vec<u8>,
    vram: Vec<u8>,
}

impl std::fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Snapshot(frame {:?})", self.frame.pixel(0, 0))
    }
}

fn snapshot(emu: &Mgba) -> Snapshot {
    Snapshot {
        frame: emu.frame(),
        ewram: emu.read_memory(EWRAM, 0x40000).unwrap(),
        iwram: emu.read_memory(IWRAM, 0x8000).unwrap(),
        vram: emu.read_memory(VRAM, 0x18000).unwrap(),
    }
}

/// Saves a state, runs `replay` idle frames (the reference), restores, takes a
/// `detour` with `held` pressed, restores again and replays: the replay must match
/// the reference, and the detour must have changed something.
fn assert_state_replays(emu: &mut Mgba, held: Buttons, detour: u32, replay: u32) {
    let state = emu.save_state().unwrap();
    let at_save = (emu.read_memory(EWRAM, 0x40000).unwrap(), emu.read_memory(IWRAM, 0x8000).unwrap());
    run(emu, replay, &mut Vec::new());
    let reference = snapshot(emu);

    emu.load_state(&state).unwrap();
    emu.set_buttons(held);
    run(emu, detour, &mut Vec::new());
    emu.set_buttons(Buttons::NONE);
    run(emu, replay, &mut Vec::new());
    assert_ne!(snapshot(emu), reference, "the detour changed nothing; the test would prove nothing");

    emu.load_state(&state).unwrap();
    let restored = (emu.read_memory(EWRAM, 0x40000).unwrap(), emu.read_memory(IWRAM, 0x8000).unwrap());
    assert!(restored == at_save, "RAM right after load_state differs from RAM at save_state");
    run(emu, replay, &mut Vec::new());
    assert_eq!(snapshot(emu), reference, "replay from the restored state diverged");
}

#[test]
fn probe_memory_map_reads_and_writes() {
    let rom = probe_rom();
    let mut emu = Mgba::gba().unwrap();
    emu.load_rom(&rom).unwrap();
    run(&mut emu, 10, &mut Vec::new());

    let regions = emu.memory_regions();
    let find = |id: &str| regions.iter().find(|r| r.id == id).unwrap_or_else(|| panic!("no {id} region"));
    let wram = find("wram");
    assert_eq!((wram.name.as_str(), wram.start, wram.size, wram.writable), ("EWRAM", EWRAM, 0x40000, true));
    assert_eq!((find("iwram").start, find("iwram").size), (IWRAM, 0x8000));
    assert!(!find("cart0").writable && !find("io").writable && find("vram").writable);
    assert!(regions.iter().all(|r| r.id != "mem"), "the virtual whole-bus block is not a region");

    // Known addresses: the cartridge as loaded, and the registers the probe set.
    assert_eq!(emu.read_memory(0x0800_0000, rom.len()).unwrap(), rom);
    assert_eq!(emu.read_memory(0x0800_00AC, 6).unwrap(), b"TAUR01");
    assert_eq!(emu.read_u16(0x0400_0000).unwrap(), 0x0403, "DISPCNT");
    assert_eq!(emu.read_u16(0x0400_0080).unwrap(), 0x1177, "SOUNDCNT_L");
    // VRAM holds the probe's fill colour, blue (BGR555 0x7C00).
    assert_eq!(emu.read_u32(VRAM).unwrap(), 0x7C00_7C00);

    // Writes land and are read back; ROM, unmapped and straddling ranges are refused.
    emu.write_memory(EWRAM + 0x100, &[1, 2, 3, 4]).unwrap();
    assert_eq!(emu.read_u32(EWRAM + 0x100).unwrap(), 0x0403_0201);
    assert!(matches!(emu.write_memory(0x0800_0000, &[0]), Err(EmuError::Memory(_))));
    assert!(matches!(emu.read_memory(0x0100_0000, 1), Err(EmuError::Memory(_))));
    assert!(matches!(emu.read_memory(EWRAM + 0x3FFFE, 4), Err(EmuError::Memory(_))));
    assert_eq!(emu.read_memory(rom.len() as u32 + 0x0800_0000, 0).unwrap(), Vec::<u8>::new());

    // Video memory is writable too (the probe redraws over it on the next frame).
    emu.write_memory(VRAM, &[0x1F, 0x00]).unwrap();
    assert_eq!(emu.read_u16(VRAM).unwrap(), 0x001F);
}

#[test]
fn probe_state_restores_and_replays() {
    let mut emu = Mgba::gba().unwrap();
    emu.load_rom(&probe_rom()).unwrap();
    run(&mut emu, 30, &mut Vec::new());
    // A redraw spans about two frames, so one frame after releasing A the screen
    // is still red: the detour leaves a trace that only the restore removes.
    assert_state_replays(&mut emu, Buttons::NONE.with(Button::A), 10, 1);
    assert!(uniform(&emu.frame()).is_some());

    // Wrong-sized states are refused and leave the core running.
    let state = emu.save_state().unwrap();
    eprintln!("probe state: {} bytes", state.len());
    assert!(matches!(emu.load_state(&state[1..]), Err(EmuError::State(_))));
    emu.run_frame().unwrap();
    assert!(matches!(Mgba::gba().unwrap().save_state(), Err(EmuError::NoRom)));
}

#[test]
fn emerald_state_restores_and_symbols_read_back() {
    let Some(rom) = base_rom() else { return };
    let mut emu = Mgba::gba().unwrap();
    emu.load_rom(&rom).unwrap();
    run(&mut emu, 300, &mut Vec::new());
    assert_state_replays(&mut emu, Buttons::NONE.with(Button::Start), 60, 120);

    // ROM through the bus equals the file, at a symbol from the build.
    let trainers = symbol("gTrainers");
    let at = (trainers - 0x0800_0000) as usize;
    assert_eq!(emu.read_memory(trainers, 52 * 4).unwrap(), rom[at..at + 52 * 4]);

    // RAM: the save block pointers in IWRAM point to their EWRAM blocks, moved by
    // the same random offset (SetSaveBlocksPointers, multiple of 4 below 128).
    let sb2 = emu.read_u32(symbol("gSaveBlock2Ptr")).unwrap();
    let sb1 = emu.read_u32(symbol("gSaveBlock1Ptr")).unwrap();
    let offset = sb2.wrapping_sub(symbol("gSaveblock2"));
    eprintln!("gSaveBlock2Ptr {sb2:#x}, gSaveBlock1Ptr {sb1:#x}, offset {offset}");
    assert!(offset < 128 && offset.is_multiple_of(4), "gSaveBlock2Ptr {sb2:#x} is not inside gSaveblock2's move range");
    assert_eq!(sb1.wrapping_sub(symbol("gSaveblock1")), offset, "save blocks moved by different offsets");
}
