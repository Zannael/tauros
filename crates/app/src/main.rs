//! Layer 1: the Tauros desktop app. It wires connectors, emulator, MCP server and
//! agent bridge together. For now (EMU-004) it is a minimal shell: one window that
//! runs a ROM with video, keyboard input and audio, and no editor features.
//!
//! The emulator runs on its own thread, paced at the GBA frame rate. Each frame is
//! sent to the window over a Tauri channel as one binary message: the RGBA
//! framebuffer followed by the frame's interleaved stereo `i16` samples. The page
//! draws it on a canvas and plays the audio with the Web Audio API, so the app
//! needs no native audio library.
//!
//! Usage: `tauros-app [ROM.gba]`, or open a ROM from the window.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::ipc::{Channel, InvokeBody, InvokeResponseBody, Request};
use tauros_emu::{Button, Buttons, Emulator, Mgba};

/// GBA refresh rate: 16.78 MHz / 280896 cycles per frame.
const FRAME_RATE: f64 = 59.7275;

enum Command {
    Attach(Channel<InvokeResponseBody>),
    Load(Vec<u8>, Sender<Result<(), String>>),
    Buttons(Buttons),
    Pause(bool),
}

struct App {
    commands: Mutex<Sender<Command>>,
    /// A ROM path given on the command line, loaded when the window attaches.
    initial_rom: Option<PathBuf>,
}

/// Buttons from a bit mask in `Button::ALL` order (bit 0 = A).
fn buttons_from_mask(mask: u16) -> Buttons {
    Button::ALL.into_iter().enumerate().filter(|(i, _)| mask & 1 << i != 0).map(|(_, b)| b).collect()
}

/// The emulator thread: applies commands, runs frames on schedule and streams
/// them to the attached window.
fn emulator_thread(commands: Receiver<Command>) {
    let mut emu = match Mgba::gba() {
        Ok(emu) => emu,
        Err(e) => return eprintln!("tauros: {e}"),
    };
    let period = Duration::from_secs_f64(1.0 / FRAME_RATE);
    let (mut channel, mut loaded, mut paused) = (None::<Channel<InvokeResponseBody>>, false, false);
    let mut next = Instant::now();
    let mut audio = Vec::new();
    loop {
        let running = loaded && !paused && channel.is_some();
        let wait = if running { next.saturating_duration_since(Instant::now()) } else { Duration::from_secs(3600) };
        match commands.recv_timeout(wait) {
            Ok(Command::Attach(c)) => channel = Some(c),
            Ok(Command::Load(rom, reply)) => {
                let result = emu.load_rom(&rom).map_err(|e| e.to_string());
                loaded |= result.is_ok();
                next = Instant::now();
                let _ = reply.send(result);
            }
            Ok(Command::Buttons(b)) => emu.set_buttons(b),
            Ok(Command::Pause(p)) => {
                paused = p;
                next = Instant::now();
            }
            Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {
                if emu.run_frame().is_err() {
                    continue;
                }
                audio.clear();
                emu.drain_audio(&mut audio);
                let frame = emu.frame();
                let mut message = frame.rgba;
                message.extend(audio.iter().flat_map(|s| s.to_le_bytes()));
                if let Some(c) = &channel {
                    if c.send(InvokeResponseBody::Raw(message)).is_err() {
                        channel = None;
                    }
                }
                next += period;
                // After a stall (window hidden, machine busy) resume without a burst.
                if Instant::now() > next + period * 4 {
                    next = Instant::now();
                }
            }
        }
    }
}

fn send(app: &App, command: Command) -> Result<(), String> {
    app.commands.lock().unwrap().send(command).map_err(|_| "the emulator thread stopped".to_owned())
}

fn load(app: &App, rom: Vec<u8>) -> Result<(), String> {
    let (reply, result) = mpsc::channel();
    send(app, Command::Load(rom, reply))?;
    result.recv().map_err(|_| "the emulator thread stopped".to_owned())?
}

/// Attaches the window's frame channel; loads the command-line ROM, if any, and
/// returns its file name.
#[tauri::command]
fn attach(app: tauri::State<'_, App>, on_frame: Channel<InvokeResponseBody>) -> Result<Option<String>, String> {
    eprintln!("tauros: window attached");
    send(&app, Command::Attach(on_frame))?;
    let Some(path) = &app.initial_rom else { return Ok(None) };
    let rom = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    load(&app, rom)?;
    Ok(Some(path.file_name().unwrap_or_default().to_string_lossy().into_owned()))
}

/// Loads a ROM sent as the raw request body.
#[tauri::command]
fn load_rom(app: tauri::State<'_, App>, request: Request<'_>) -> Result<(), String> {
    let InvokeBody::Raw(rom) = request.body() else { return Err("expected the ROM bytes as a binary body".into()) };
    load(&app, rom.clone())
}

/// Prints a diagnostic from the page (first frame, audio state, script errors) on
/// the terminal that started the app.
#[tauri::command]
fn log(message: String) {
    eprintln!("tauros window: {message}");
}

#[tauri::command]
fn set_buttons(app: tauri::State<'_, App>, mask: u16) -> Result<(), String> {
    send(&app, Command::Buttons(buttons_from_mask(mask)))
}

#[tauri::command]
fn set_paused(app: tauri::State<'_, App>, paused: bool) -> Result<(), String> {
    send(&app, Command::Pause(paused))
}

fn main() {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new().name("emulator".into()).spawn(move || emulator_thread(rx)).expect("emulator thread");
    let initial_rom = std::env::args_os().nth(1).map(PathBuf::from);
    tauri::Builder::default()
        .manage(App { commands: Mutex::new(tx), initial_rom })
        .on_page_load(|window, payload| eprintln!("tauros: page {:?} {} in {}", payload.event(), payload.url(), window.label()))
        .invoke_handler(tauri::generate_handler![attach, load_rom, log, set_buttons, set_paused])
        .run(tauri::generate_context!())
        .expect("error while running Tauros");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_bits_follow_button_order() {
        assert_eq!(buttons_from_mask(0), Buttons::NONE);
        let b = buttons_from_mask(1 | 1 << 3 | 1 << 9);
        assert_eq!(b.iter().collect::<Vec<_>>(), [Button::A, Button::Start, Button::L]);
    }
}
