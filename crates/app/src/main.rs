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
//! The chat panel talks to the Claude Agent SDK sidecar (`AGT-003`). Its session
//! gets the Tauros MCP tools of the project given with `--project`, served in this
//! process; each turn's events reach the page over a channel as they stream.
//!
//! Usage: `tauros-app --help`; choose a ROM or a project boot target.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use tauri::ipc::{Channel, InvokeBody, InvokeResponseBody, Request};
use tauros_agent_bridge::claude_sidecar::ClaudeSidecarConfig;
use tauros_agent_bridge::AgentEvent;
use tauros_app::chat::{antigravity_backend, claude_backend, codex_backend, Chat};
use tauros_app::history::{list_history, undo_to, HistoryEntry, UndoReceipt};
use tauros_app::launch::{Launch, PreparedLaunch, USAGE};
use tauros_app::mcp_host::{BuildAndBoot, ProjectSession};
use tauros_app::playback::{buttons_from_mask, emulator_thread, Command};
use tauros_connector_gen3::toolchain::Toolchain;
use tauros_emu::{Buttons, Mgba};
use tauros_mcp::emulator::EmuSession;
use serde::Serialize;

struct App {
    commands: Mutex<Sender<Command>>,
    /// A command-line ROM or project target, loaded when the window attaches.
    initial_launch: Option<Launch>,
    /// The `--project` game project, shared by the window and the agent's tools.
    project: Option<Arc<ProjectSession>>,
    chat: Arc<Chat>,
    chat_provider: Arc<Mutex<String>>,
    codex_login: Mutex<tauros_app::codex_login::CodexLogin>,
    antigravity_login: Mutex<tauros_app::antigravity_login::AntigravityLogin>,
    /// Set while a text field has focus: game keys then go to the field.
    typing: Arc<AtomicBool>,
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

/// Sends a chat message; the turn's events stream to `on_event` until it completes.
/// Runs off the main thread: the first message starts the sidecar and MCP server.
#[tauri::command(async)]
fn chat_send(app: tauri::State<'_, App>, text: String, on_event: Channel<AgentEvent>) -> Result<String, String> {
    let login = app.codex_login.lock().unwrap();
    if login.pending() { return Err("finish or cancel ChatGPT sign-in before sending".into()); }
    if app.antigravity_login.lock().unwrap().pending() { return Err("finish or cancel Google sign-in before sending".into()); }
    let turn = app.chat.send(&text, move |event| {
        let _ = on_event.send(event);
    })?;
    Ok(turn.to_string())
}

#[tauri::command(async)]
fn chat_cancel(app: tauri::State<'_, App>) -> Result<(), String> {
    app.chat.cancel()
}

/// Ends the conversation; the next message starts a new one.
#[tauri::command(async)]
fn chat_reset(app: tauri::State<'_, App>) {
    app.chat.reset();
}

/// The page reports whether a text field has focus, so typed keys are not game input.
#[tauri::command]
fn set_typing(app: tauri::State<'_, App>, typing: bool) -> Result<(), String> {
    app.typing.store(typing, Ordering::Relaxed);
    if typing {
        send(&app, Command::Buttons(Buttons::NONE))?;
    }
    Ok(())
}

/// Select a provider for the next conversation; switching closes the old session.
#[tauri::command]
fn chat_select(app: tauri::State<'_, App>, provider: String) -> Result<(), String> {
    if !["claude", "codex_chatgpt", "antigravity"].contains(&provider.as_str()) {
        return Err("unknown agent provider".into());
    }
    if app.chat.busy() { return Err("wait for the current reply or stop it before switching agents".into()); }
    if app.codex_login.lock().unwrap().pending() { return Err("finish or cancel ChatGPT sign-in before switching agents".into()); }
    if app.antigravity_login.lock().unwrap().pending() { return Err("finish or cancel Google sign-in before switching agents".into()); }
    app.chat.reset();
    *app.chat_provider.lock().unwrap() = provider;
    Ok(())
}

/// Start the official browser ceremony without mounting game sources or tools.
#[tauri::command(async)]
fn codex_login_start(app: tauri::State<'_, App>) -> Result<tauros_app::codex_login::LoginStatus, String> {
    if *app.chat_provider.lock().unwrap() != "codex_chatgpt" { return Err("select Codex · ChatGPT first".into()); }
    let mut login = app.codex_login.lock().unwrap();
    if app.chat.busy() { return Err("stop the current reply before signing in".into()); }
    app.chat.reset();
    let project = app.project_root().unwrap_or_else(repo_root);
    login.start(codex_config(), &project, tauros_app::codex_login::open_browser)
}

#[tauri::command(async)]
fn codex_login_status(app: tauri::State<'_, App>) -> Result<tauros_app::codex_login::LoginStatus, String> {
    app.codex_login.lock().unwrap().poll()
}

#[tauri::command(async)]
fn codex_login_cancel(app: tauri::State<'_, App>) -> Result<tauros_app::codex_login::LoginStatus, String> {
    app.codex_login.lock().unwrap().cancel()
}

/// Silent check of the cached Antigravity sign-in when the provider is selected.
#[tauri::command(async)]
fn antigravity_login_check(app: tauri::State<'_, App>) -> Result<tauros_app::antigravity_login::LoginStatus, String> {
    if *app.chat_provider.lock().unwrap() != "antigravity" { return Err("select Antigravity first".into()); }
    let project = app.project_root().unwrap_or_else(repo_root);
    app.antigravity_login.lock().unwrap().check(antigravity_config(), &project)
}

/// The runtime opens the browser and hosts the callback; only a status returns.
#[tauri::command(async)]
fn antigravity_login_start(app: tauri::State<'_, App>) -> Result<tauros_app::antigravity_login::LoginStatus, String> {
    if *app.chat_provider.lock().unwrap() != "antigravity" { return Err("select Antigravity first".into()); }
    let mut login = app.antigravity_login.lock().unwrap();
    if app.chat.busy() { return Err("stop the current reply before signing in".into()); }
    app.chat.reset();
    let project = app.project_root().unwrap_or_else(repo_root);
    login.start(antigravity_config(), &project)
}

#[tauri::command(async)]
fn antigravity_login_status(app: tauri::State<'_, App>) -> Result<tauros_app::antigravity_login::LoginStatus, String> {
    app.antigravity_login.lock().unwrap().poll()
}

#[tauri::command(async)]
fn antigravity_login_cancel(app: tauri::State<'_, App>) -> Result<tauros_app::antigravity_login::LoginStatus, String> {
    app.antigravity_login.lock().unwrap().cancel()
}

fn codex_config() -> tauros_agent_bridge::codex::CodexConfig {
    let data = std::env::var_os("TAUROS_DATA").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share/tauros")
    });
    let home = std::env::var_os("TAUROS_CODEX_HOME").map(PathBuf::from).unwrap_or_else(|| data.join("codex"));
    let mut config = tauros_agent_bridge::codex::CodexConfig::new(home);
    if let Some(binary) = std::env::var_os("TAUROS_CODEX_BIN") { config.binary = binary.into(); }
    config
}

/// Dedicated Antigravity runtime state; the subscription sign-in stays in agy's own store.
fn antigravity_config() -> tauros_agent_bridge::antigravity::AntigravityConfig {
    let data = std::env::var_os("TAUROS_DATA").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share/tauros")
    });
    let mut config = tauros_agent_bridge::antigravity::AntigravityConfig::new(data.join("antigravity"));
    if let Some(binary) = std::env::var_os("TAUROS_AGY_BIN") { config.binary = binary.into(); }
    config
}

/// All providers share the launch's project, native guard and emulator.
fn project_chat(project: Option<Arc<ProjectSession>>, provider: Arc<Mutex<String>>) -> Chat {
    let repo = repo_root();
    Chat::new(move || {
        let server = project.as_ref().map(|project| project.mcp_server());
        let working_dir = project.as_ref().map_or_else(|| repo.clone(), |project| project.dir().to_path_buf());
        let provider = provider.lock().unwrap().clone();
        if provider == "claude" {
            return claude_backend(ClaudeSidecarConfig::from_repo_root(&repo), server, working_dir, BTreeMap::new());
        }
        if provider == "antigravity" {
            return antigravity_backend(antigravity_config(), server, working_dir, std::env::var("TAUROS_AGY_MODEL").ok());
        }
        codex_backend(codex_config(), server, working_dir, tauros_agent_bridge::AuthMode::HostSignIn, std::env::var("TAUROS_CODEX_MODEL").ok())
    })
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The open project's maps and the launch's boot point, for the build-and-play bar.
#[derive(Serialize)]
struct ProjectInfo {
    maps: Vec<String>,
    map_bounds: Vec<tauros_app::mcp_host::MapPreviewInfo>,
    map: String,
    x: i16,
    y: i16,
}

#[tauri::command(async)]
fn project_info(app: tauri::State<'_, App>) -> Result<Option<ProjectInfo>, String> {
    let (Some(project), Some(Launch::Point { target, .. })) = (&app.project, &app.initial_launch) else { return Ok(None) };
    let map_bounds = project.preview_maps()?;
    Ok(Some(ProjectInfo { maps: map_bounds.iter().map(|map| map.id.clone()).collect(),
        map_bounds, map: target.map.0.clone(), x: target.x, y: target.y }))
}

/// Builds the project and boots the shared emulator at `map` (`x`, `y`) with the
/// launch's flags. Refused while the agent is working on the same emulator.
#[tauri::command(async)]
fn build_and_play(app: tauri::State<'_, App>, map: String, x: i16, y: i16) -> Result<BuildAndBoot, String> {
    let project = app.project.as_ref().ok_or("no game project is open")?;
    if app.chat.busy() {
        return Err("the agent is working; wait for it or stop it".into());
    }
    let flags = match &app.initial_launch {
        Some(Launch::Point { target, .. }) => target.flags.iter().map(|f| f.0.clone()).collect(),
        _ => Vec::new(),
    };
    let report = project.build_and_boot(&map, x, y, &flags)?;
    eprintln!("tauros: built and booted {map} ({x}, {y}): {report:?}");
    Ok(report)
}

/// Preview the last built ROM without running the ROM build.
#[tauri::command(async)]
fn preview_map(app: tauri::State<'_, App>, map: String, x: i16, y: i16) -> Result<BuildAndBoot, String> {
    let project = app.project.as_ref().ok_or("no game project is open")?;
    if app.chat.busy() { return Err("the agent is working; wait for it or stop it".into()); }
    let flags = match &app.initial_launch {
        Some(Launch::Point { target, .. }) => target.flags.iter().map(|f| f.0.clone()).collect(),
        _ => Vec::new(),
    };
    project.preview_built(&map, x, y, &flags)
}

#[tauri::command]
fn chat_info(app: tauri::State<'_, App>) -> Option<String> {
    app.project_root().map(|root| root.display().to_string())
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
    if app.chat.busy() {
        return Err("wait for the agent to finish before undoing changes".into());
    }
    let gate = app.project.as_ref().map(|project| project.operation_gate());
    let _operation = gate.as_ref().map(|gate| gate.try_lock()
        .map_err(|_| "another project operation is running".to_owned())).transpose()?;
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
fn native_keyboard(window: &tauri::WebviewWindow, commands: Sender<Command>, typing: Arc<AtomicBool>) -> tauri::Result<()> {
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
        if typing.load(Ordering::Relaxed) {
            if mask.replace(0) != 0 {
                let _ = commands.send(Command::Buttons(Buttons::NONE));
            }
            return gtk::glib::Propagation::Proceed;
        }
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
    let typing = Arc::new(AtomicBool::new(false));
    let project = match &initial_launch {
        Some(Launch::Point { project, .. }) => {
            let toolchain = Toolchain::for_repository(&repo_root()).ok();
            match ProjectSession::open(project, toolchain, None) {
                Ok(session) => Some(Arc::new(session)),
                Err(error) => {
                    eprintln!("tauros: {error}");
                    std::process::exit(2);
                }
            }
        }
        _ => None,
    };
    let session = match &project {
        Some(project) => project.emulator(),
        None => match Mgba::gba() {
            Ok(mgba) => Arc::new(Mutex::new(EmuSession::new(mgba))),
            Err(error) => {
                eprintln!("tauros: {error}");
                std::process::exit(1);
            }
        },
    };
    let chat_provider = Arc::new(Mutex::new("claude".to_owned()));
    let chat = Arc::new(project_chat(project.clone(), Arc::clone(&chat_provider)));
    let chat_holds = Arc::clone(&chat);
    #[cfg(target_os = "linux")]
    let (keyboard, keyboard_typing) = (tx.clone(), typing.clone());
    std::thread::Builder::new()
        .name("emulator".into())
        .spawn(move || emulator_thread(rx, session, move || chat_holds.busy()))
        .expect("emulator thread");
    tauri::Builder::default()
        .manage(App { commands: Mutex::new(tx), initial_launch, project, chat, chat_provider, codex_login: Mutex::new(tauros_app::codex_login::CodexLogin::default()), antigravity_login: Mutex::default(), typing })
        .setup(move |app| {
            #[cfg(target_os = "linux")]
            {
                use tauri::Manager;
                let window = app.get_webview_window("main").expect("main window");
                native_keyboard(&window, keyboard.clone(), keyboard_typing.clone())?;
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
            history_undo,
            chat_send,
            chat_cancel,
            codex_login_start,
            codex_login_status,
            codex_login_cancel,
            antigravity_login_check,
            antigravity_login_start,
            antigravity_login_status,
            antigravity_login_cancel,
            chat_reset,
            chat_select,
            chat_info,
            set_typing,
            project_info,
            build_and_play,
            preview_map
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
