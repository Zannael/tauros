//! EMU-001: the mGBA core through the `Emulator` trait, headless.
//!
//! The always-on tests boot the Tauros probe ROM (tests/probe/probe.s, original
//! code, built into bytes here). The pokeemerald-expansion test boots the base ROM
//! the user built with `python3 scripts/setup_toolchain.py`; it skips when that ROM
//! is missing. No ROM is committed.

use std::path::PathBuf;
use tauros_emu::{Button, Buttons, EmuError, Emulator, Frame, Mgba};

/// tests/probe/probe.s assembled by devkitARM: `b start` at 0, game code and the
/// fixed 0x96 byte in the header, code and literal pool from 0xC0.
fn probe_rom() -> Vec<u8> {
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
fn uniform(frame: &Frame) -> Option<[u8; 3]> {
    let first = frame.pixel(0, 0);
    frame.rgba.chunks(4).all(|p| p[..3] == first).then_some(first)
}

fn run(emu: &mut Mgba, frames: u32, audio: &mut Vec<i16>) {
    for _ in 0..frames {
        emu.run_frame().unwrap();
        emu.drain_audio(audio);
    }
}

/// Runs frames until the screen is one colour satisfying `want`; at most 20.
fn run_until(emu: &mut Mgba, audio: &mut Vec<i16>, want: impl Fn([u8; 3]) -> bool) -> u32 {
    for n in 1..=20 {
        run(emu, 1, audio);
        if uniform(&emu.frame()).is_some_and(&want) {
            return n;
        }
    }
    panic!("the screen did not reach the expected colour in 20 frames: {:?}", emu.frame().pixel(0, 0));
}

#[test]
fn probe_rom_boots_renders_reacts_to_input_and_plays_audio() {
    let rom = probe_rom();
    assert_eq!(rom.len(), 312);
    let mut emu = Mgba::gba().unwrap();
    assert_eq!(Mgba::version(), "0.10.5");
    emu.load_rom(&rom).unwrap();
    let mut audio = Vec::new();

    run(&mut emu, 60, &mut audio);
    assert!(emu.frame_count() >= 60);
    let frame = emu.frame();
    assert_eq!((frame.width, frame.height, frame.rgba.len()), (240, 160, 240 * 160 * 4));
    let blue = uniform(&frame).expect("idle screen is one colour");
    assert!(blue[2] >= 0xF0 && blue[0] == 0 && blue[1] == 0, "idle screen is not blue: {blue:?}");

    // The probe redraws the whole screen in a busy loop that spans several frames,
    // so wait (bounded) for the screen to become the expected colour.
    emu.set_buttons(Buttons::NONE.with(Button::A));
    let frames = run_until(&mut emu, &mut audio, |c| c[0] >= 0xF0 && c[1] == 0 && c[2] == 0);
    eprintln!("screen turned red {frames} frames after pressing A");
    emu.set_buttons(Buttons::NONE);
    let frames = run_until(&mut emu, &mut audio, |c| c == blue);
    eprintln!("screen turned blue {frames} frames after releasing A");

    // Every emulated frame at 59.7275 Hz of 32768 Hz stereo, with a square tone in it.
    let frames = audio.len() / 2;
    let expected = emu.frame_count() as f64 * emu.audio_rate() as f64 / 59.7275;
    eprintln!("audio: {frames} stereo frames, expected about {expected:.0}");
    assert!((frames as f64 - expected).abs() < expected * 0.05, "audio length off: {frames}");
    let peak = audio.iter().map(|s| s.unsigned_abs()).max().unwrap();
    assert!(peak > 1000, "audio is silent (peak {peak})");
    assert!(audio.chunks(2).all(|lr| lr[0] == lr[1]), "the tone is panned to both sides equally");
}

#[test]
fn errors_without_a_rom_and_for_non_roms() {
    let mut emu = Mgba::gba().unwrap();
    assert_eq!(emu.run_frame(), Err(EmuError::NoRom));
    assert_eq!(emu.reset(), Err(EmuError::NoRom));
    assert!(matches!(emu.load_rom(b"not a rom"), Err(EmuError::Rom(_))));
    let mut rom = probe_rom();
    rom[3] = 0; // the entry branch's condition byte, mGBA's ROM signature
    assert!(matches!(emu.load_rom(&rom), Err(EmuError::Rom(_))));
    // A failed load leaves the core usable.
    emu.load_rom(&probe_rom()).unwrap();
    emu.run_frame().unwrap();
}

#[test]
fn reloading_and_reset_restart_the_console() {
    let mut emu = Mgba::gba().unwrap();
    emu.load_rom(&probe_rom()).unwrap();
    run(&mut emu, 10, &mut Vec::new());
    emu.reset().unwrap();
    assert_eq!(emu.frame_count(), 0);
    emu.load_rom(&probe_rom()).unwrap();
    assert_eq!(emu.frame_count(), 0);
    run(&mut emu, 3, &mut Vec::new());
    assert!(uniform(&emu.frame()).is_some());
}

/// The FND-002 base ROM (pokeemerald-expansion built by the user), if present.
fn base_rom() -> Option<Vec<u8>> {
    let path = std::env::var_os("TAUROS_BASE_ROM").map(PathBuf::from).unwrap_or_else(|| {
        let data = std::env::var_os("TAUROS_DATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap()).join(".local/share/tauros"));
        data.join("engines/pokeemerald-expansion-expansion-1.16.4-base/pokeemerald.gba")
    });
    match std::fs::read(&path) {
        Ok(rom) => Some(rom),
        Err(_) => {
            eprintln!("SKIPPED: no base ROM at {}; run `python3 scripts/setup_toolchain.py`", path.display());
            None
        }
    }
}

#[test]
fn user_built_emerald_boots_plays_audio_and_takes_input() {
    let Some(rom) = base_rom() else { return };
    // Two cores run the same frames; only the second presses Start. Emulation is
    // deterministic, so any difference comes from the input.
    let (mut idle, mut pressed) = (Mgba::gba().unwrap(), Mgba::gba().unwrap());
    idle.load_rom(&rom).unwrap();
    pressed.load_rom(&rom).unwrap();
    let (mut idle_audio, mut pressed_audio) = (Vec::new(), Vec::new());
    run(&mut idle, 400, &mut idle_audio);
    run(&mut pressed, 400, &mut pressed_audio);
    assert_eq!(idle.frame(), pressed.frame(), "identical runs diverged");
    assert!(uniform(&idle.frame()).is_none(), "the intro shows a single colour after 400 frames");
    let peak = idle_audio.iter().map(|s| s.unsigned_abs()).max().unwrap();
    eprintln!("Emerald audio peak over 400 frames: {peak}");
    assert!(peak > 1000, "no audio during the intro");

    for frame in 0..200 {
        let start = frame % 20 < 5;
        pressed.set_buttons(if start { Buttons::NONE.with(Button::Start) } else { Buttons::NONE });
        idle.run_frame().unwrap();
        pressed.run_frame().unwrap();
    }
    let differing = idle.frame().rgba.chunks(4).zip(pressed.frame().rgba.chunks(4)).filter(|(a, b)| a != b).count();
    eprintln!("pixels differing after pressing Start: {differing} of {}", 240 * 160);
    assert!(differing > 240 * 160 / 10, "pressing Start did not change the screen");
}
