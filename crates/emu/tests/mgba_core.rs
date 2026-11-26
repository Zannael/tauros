//! EMU-001: the mGBA core through the `Emulator` trait, headless.
//!
//! The always-on tests boot the Tauros probe ROM (tests/probe/probe.s, original
//! code, embedded as bytes in tests/common). The pokeemerald-expansion test boots the base ROM
//! the user built with `python3 scripts/setup_toolchain.py`; it skips when that ROM
//! is missing. No ROM is committed.

mod common;

use common::{base_rom, probe_rom, run, uniform};
use tauros_emu::{Button, Buttons, EmuError, Emulator, Mgba};

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

    let vlayers = emu.video_layers();
    let achans = emu.audio_channels();
    assert!(vlayers.len() >= 5, "expected GBA BG0-BG3 + OBJ, got {vlayers:?}");
    assert!(achans.len() >= 6, "expected GBA PSG 1-4 + FIFO A/B, got {achans:?}");
    // Test GBA audio channel toggling (0..6: PSG 1-4, FIFO A/B).
    let mut active_audio = Vec::new();
    run(&mut emu, 10, &mut active_audio);
    let active_spread = active_audio.iter().copied().max().unwrap_or(0)
        - active_audio.iter().copied().min().unwrap_or(0);
    for ch in &achans {
        emu.set_audio_channel(ch.id, false);
    }
    // Flush any residual blip_buf decay from before channels were disabled.
    run(&mut emu, 2, &mut Vec::new());
    let mut muted_audio = Vec::new();
    run(&mut emu, 10, &mut muted_audio);
    let muted_spread = muted_audio.iter().copied().max().unwrap_or(0)
        - muted_audio.iter().copied().min().unwrap_or(0);
    assert!(
        muted_spread < active_spread / 2,
        "disabling all audio channels should attenuate output (active {active_spread}, muted {muted_spread})"
    );
    for ch in &achans {
        emu.set_audio_channel(ch.id, true);
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
