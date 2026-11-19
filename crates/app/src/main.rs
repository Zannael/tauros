//! Layer 1: the Tauros desktop app. It wires connectors, emulator, MCP server and
//! agent bridge together. For now it is a minimal shell: one window that
//! runs a ROM with video, keyboard input and audio, and no editor features.
//!
//! The emulator runs on its own thread, paced at the GBA frame rate. Each frame is
//! sent to the window over a Tauri channel as one binary message: the RGBA
//! framebuffer followed by the frame's interleaved stereo `i16` samples. The page
//! draws it on a canvas and plays the audio with the Web Audio API, so the app
//! needs no native audio library.
//!
//! Usage: `tauros-app --help`; choose a ROM or a project boot target.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::Mutex;
use tauri::ipc::{Channel, InvokeBody, InvokeResponseBody, Request};
use tauros_app::history::{list_history, undo_to, HistoryEntry, UndoReceipt};
use tauros_app::launch::{Launch, PreparedLaunch, USAGE};
use tauros_app::playback::{buttons_from_mask, emulator_thread, Command};
use tauros_emu::Buttons;

struct App {
    commands: Mutex<Sender<Command>>,
    /// A command-line ROM or project target, loaded when the window attaches.
    initial_launch: Option<Launch>,
}

impl App {
    fn project_root(&self) -> Option<PathBuf> {
        match &self.initial_launch {
            Some(Launch::Point { project, .. }) => Some(project.clone()),
            _ => None,
        }
    }
}

fn send(app: &App, command: Command) -> Result<(), String> {
    app.commands.lock().unwrap().send(command).map_err(|_| "the emulator thread stopped".to_owned())
}

fn load(app: &App, launch: PreparedLaunch) -> Result<(), String> {
    let (reply, result) = mpsc::channel();
    send(app, Command::Load(launch, reply))?;
    let report = result.recv().map_err(|_| "the emulator thread stopped".to_owned())??;
    if let Some(report) = report {
        eprintln!("tauros: booted {report:?}");
    }
    Ok(())
}

/// Attaches the window's frame channel and starts the CLI-selected launch, if any.
#[tauri::command]
fn attach(app: tauri::State<'_, App>, on_frame: Channel<InvokeResponseBody>) -> Result<Option<String>, String> {
    eprintln!("tauros: window attached");
    send(&app, Command::Attach(on_frame))?;
    let Some(launch) = &app.initial_launch else { return Ok(None) };
    let prepared = launch.prepare()?;
    let name = prepared.name.clone();
    load(&app, prepared)?;
    Ok(Some(name))
}

/// Loads a ROM sent as the raw request body.
#[tauri::command]
fn load_rom(app: tauri::State<'_, App>, request: Request<'_>) -> Result<(), String> {
    let InvokeBody::Raw(rom) = request.body() else { return Err("expected the ROM bytes as a binary body".into()) };
    load(&app, PreparedLaunch { name: String::new(), rom: rom.clone(), boot: None })
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

#[tauri::command]
fn history_list(
    app: tauri::State<'_, App>,
    project: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<HistoryEntry>, String> {
    let root = project
        .map(PathBuf::from)
        .or_else(|| app.project_root())
        .ok_or_else(|| "no game project is open".to_owned())?;
    list_history(&root, limit.unwrap_or(50))
}

#[tauri::command]
fn history_undo(
    app: tauri::State<'_, App>,
    target_commit: String,
    project: Option<String>,
) -> Result<UndoReceipt, String> {
    let root = project
        .map(PathBuf::from)
        .or_else(|| app.project_root())
        .ok_or_else(|| "no game project is open".to_owned())?;
    undo_to(&root, &target_commit)
}

/// The button bit (in `Button::ALL` order) for a GDK key name, case-insensitive:
/// arrows, X = A, Z = B, Enter = Start, Backspace = Select, A = L, S = R.
fn bit_for_key_name(name: &str) -> Option<u16> {
    Some(match name.to_ascii_lowercase().as_str() {
        "x" => 0,
        "z" => 1,
        "backspace" => 2,
        "return" | "kp_enter" => 3,
        "right" => 4,
        "left" => 5,
        "up" => 6,
        "down" => 7,
        "s" => 8,
        "a" => 9,
        _ => return None,
    })
}

/// Reads the keyboard on the GTK window: key events reach the toplevel before
/// WebKit and any input method, so the game gets them whatever the page's focus.
#[cfg(target_os = "linux")]
fn native_keyboard(window: &tauri::WebviewWindow, commands: Sender<Command>) -> tauri::Result<()> {
    use gtk::prelude::*;
    use std::cell::Cell;
    use std::rc::Rc;
    let gtk_window = window.gtk_window()?;
    let mask = Rc::new(Cell::new(0u16));
    let logged = Rc::new(Cell::new(0u32));
    // TAUROS_LOG_KEYS=1 logs every key event, not only the first four.
    let verbose = std::env::var_os("TAUROS_LOG_KEYS").is_some();
    let (clear_mask, clear_commands) = (mask.clone(), commands.clone());
    let handler = move |event: &gtk::gdk::EventKey, down: bool| {
        let name = event.keyval().name().map(|n| n.to_string()).unwrap_or_default();
        let bit = bit_for_key_name(&name);
        if logged.get() < 4 || verbose {
            logged.set(logged.get() + 1);
            eprintln!("tauros: key {name:?} {} -> {bit:?}", if down { "down" } else { "up" });
        }
        let Some(bit) = bit else { return gtk::glib::Propagation::Proceed };
        let next = if down { mask.get() | 1 << bit } else { mask.get() & !(1 << bit) };
        if next != mask.get() {
            mask.set(next);
            let _ = commands.send(Command::Buttons(buttons_from_mask(next)));
        }
        gtk::glib::Propagation::Stop
    };
    let press = Rc::new(handler);
    let release = press.clone();
    gtk_window.connect_key_press_event(move |_, e| press(e, true));
    gtk_window.connect_key_release_event(move |_, e| release(e, false));
    // Release everything when the window loses focus, so no key stays held.
    gtk_window.connect_focus_out_event(move |_, _| {
        if clear_mask.replace(0) != 0 {
            let _ = clear_commands.send(Command::Buttons(Buttons::NONE));
        }
        gtk::glib::Propagation::Proceed
    });
    Ok(())
}

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 1 && args[0] == "--help" {
        println!("{USAGE}");
        return;
    }
    let initial_launch = match Launch::parse(args) {
        Ok(launch) => launch,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let (tx, rx) = mpsc::channel();
    #[cfg(target_os = "linux")]
    let keyboard = tx.clone();
    std::thread::Builder::new().name("emulator".into()).spawn(move || emulator_thread(rx)).expect("emulator thread");
    tauri::Builder::default()
        .manage(App { commands: Mutex::new(tx), initial_launch })
        .setup(move |app| {
            #[cfg(target_os = "linux")]
            {
                use tauri::Manager;
                let window = app.get_webview_window("main").expect("main window");
                native_keyboard(&window, keyboard.clone())?;
            }
            let _ = app;
            Ok(())
        })
        .on_page_load(|window, payload| eprintln!("tauros: page {:?} {} in {}", payload.event(), payload.url(), window.label()))
        .invoke_handler(tauri::generate_handler![
            attach,
            load_rom,
            log,
            set_buttons,
            set_paused,
            history_list,
            history_undo
        ])
        .run(tauri::generate_context!())
        .expect("error while running Tauros");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauros_emu::Button;

    #[test]
    fn gdk_key_names_map_to_buttons() {
        assert_eq!(bit_for_key_name("x"), Some(0));
        assert_eq!(bit_for_key_name("X"), Some(0));
        assert_eq!(bit_for_key_name("Return"), Some(3));
        assert_eq!(bit_for_key_name("BackSpace"), Some(2));
        assert_eq!(bit_for_key_name("Left"), Some(5));
        assert_eq!(bit_for_key_name("q"), None);
    }

    #[test]
    fn mask_bits_follow_button_order() {
        assert_eq!(buttons_from_mask(0), Buttons::NONE);
        let b = buttons_from_mask(1 | 1 << 3 | 1 << 9);
        assert_eq!(b.iter().collect::<Vec<_>>(), [Button::A, Button::Start, Button::L]);
    }
}
