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
use std::sync::{Arc, Mutex, RwLock};
use tauri::ipc::{Channel, InvokeBody, InvokeResponseBody, Request};
use tauros_agent_bridge::claude_sidecar::ClaudeSidecarConfig;
use tauros_agent_bridge::AgentEvent;
use tauros_app::chat::{antigravity_backend, claude_backend, codex_backend, Chat};
use tauros_app::emu_config::{
    emulator_data_dir, load_config, save_config, EmuConfig, EmuPrefs, KeyMap,
};
use tauros_app::emu_files::{
    export_save_file, import_save_bytes, list_quick_slots, load_quick_slot,
    load_state_from_bytes, save_quick_slot, save_screenshot, save_state_to_file,
    SavedFileReceipt, SavestateSlotStatus,
};
use tauros_app::emu_inspect::{
    list_memory_regions, read_memory_window, read_oam, read_palettes, read_tiles_rgba,
    MemoryRegionDto, MemoryWindowDto, OamEntryDto, PaletteDump, SymbolMatchDto, TileSheetDto,
};
use tauros_app::history::{list_history, undo_to, HistoryEntry, UndoReceipt};
use tauros_app::launch::{Launch, PreparedLaunch, USAGE};
use tauros_app::mcp_host::{AgentEmuContext, BuildAndBoot, LastBootTarget, ProjectSession};
use tauros_app::playback::{buttons_from_mask, emulator_thread, Command, PlaybackStatus};
use tauros_connector_gen3::toolchain::Toolchain;
use tauros_emu::{Buttons, Mgba};
use tauros_mcp::emulator::EmuSession;
use serde::Serialize;

struct App {
    commands: Mutex<Sender<Command>>,
    /// Shared emulator session (also wired to the project's MCP server when open).
    emu_session: Arc<Mutex<EmuSession>>,
    /// Persisted emulator key bindings and playback/AV preferences.
    emu_config: Arc<RwLock<EmuConfig>>,
    emu_data_dir: PathBuf,
    rom_name: Mutex<String>,
    video_layers_disabled: Mutex<std::collections::HashSet<usize>>,
    audio_channels_disabled: Mutex<std::collections::HashSet<usize>>,
    /// A command-line ROM or project target, loaded when the window attaches.
    initial_launch: Option<Launch>,
    /// The `--project` game project, shared by the window and the agent's tools.
    project: Option<Arc<ProjectSession>>,
    chat: Arc<Chat>,
    chat_provider: Arc<Mutex<String>>,
    /// Model chosen per provider in this run, restored when switching back.
    chat_models: Mutex<std::collections::HashMap<String, String>>,
    codex_login: Mutex<tauros_app::codex_login::CodexLogin>,
    antigravity_login: Mutex<tauros_app::antigravity_login::AntigravityLogin>,
    /// Set while a text field has focus: game keys then go to the field.
    typing: Arc<AtomicBool>,
    shutdown: tauros_app::shutdown::Shutdown,
    voice: tauros_app::voice::Voice,
    claude_voice: tauros_app::claude_dictation::ClaudeDictation,
    mapgen: tauros_app::mapgen::MapGen,
    /// Arguments of the instance to start after exit (opening another project).
    relaunch: Mutex<Option<Vec<std::ffi::OsString>>>,
}

impl App {
    fn project_root(&self) -> Option<PathBuf> {
        self.initial_launch.as_ref().and_then(Launch::project).map(Path::to_path_buf)
    }
}

impl tauros_app::shutdown::Resources for App {
    fn exit_state(&self) -> &tauros_app::shutdown::Shutdown { &self.shutdown }

    fn stop(&self) {
        // A slow provider must not keep microphone capture or login alive.
        std::thread::scope(|scope| {
            scope.spawn(|| self.chat.shutdown());
            scope.spawn(|| self.voice.shutdown());
            scope.spawn(|| self.claude_voice.shutdown());
            scope.spawn(|| self.mapgen.shutdown());
            scope.spawn(|| self.codex_login.lock().unwrap().shutdown());
            scope.spawn(|| self.antigravity_login.lock().unwrap().shutdown());
        });
    }
}

fn send(app: &App, command: Command) -> Result<(), String> {
    app.commands.lock().unwrap().send(command).map_err(|_| "the emulator thread stopped".to_owned())
}

fn load(app: &App, launch: PreparedLaunch) -> Result<(), String> {
    let (reply, result) = mpsc::channel();
    send(app, Command::Load(launch, reply))?;
    let report = result.recv().map_err(|_| "the emulator thread stopped".to_owned())??;
    app.video_layers_disabled.lock().unwrap().clear();
    app.audio_channels_disabled.lock().unwrap().clear();
    if let Some(report) = report {
        eprintln!("tauros: booted {report:?}");
    }
    Ok(())
}

/// A launch that could not start reaches the terminal with its boot point.
fn log_launch_failure(launch: &Launch, cause: &str) {
    match launch {
        Launch::Point { target, .. } => tauros_app::mcp_host::log_boot_failure(&target.map.0, target.x, target.y, cause),
        _ => eprintln!("tauros: launch failed: {cause}"),
    }
}

/// Attaches the window's frame channel and starts the CLI-selected launch, if any.
/// Off the main thread: the first boot can take a while and the window must paint.
#[tauri::command(async)]
fn attach(
    app: tauri::State<'_, App>,
    on_frame: Channel<InvokeResponseBody>,
) -> Result<Option<String>, String> {
    eprintln!("tauros: window attached");
    send(&app, Command::Attach(on_frame))?;
    let Some(launch) = &app.initial_launch else { return Ok(None) };
    // A project without a build opens without a game; the Project view offers Build.
    if let Launch::Project { project } = launch {
        if !tauros_app::projects::is_built(project) {
            return Ok(None);
        }
    }
    let prepared = launch.prepare().inspect_err(|cause| log_launch_failure(launch, cause))?;
    let name = prepared.name.clone();
    let layout = prepared.layout;
    load(&app, prepared).inspect_err(|cause| log_launch_failure(launch, cause))?;
    *app.rom_name.lock().unwrap() = name.clone();
    if let (Some(project), Some(Launch::Point { target, .. })) = (&app.project, &app.initial_launch) {
        if let Some(l) = layout {
            project.record_layout(l);
        }
        let rom_sha = app.emu_session.lock().unwrap().rom_sha256().unwrap_or_default().to_owned();
        let flags: Vec<String> = target.flags.iter().map(|f| f.0.clone()).collect();
        project.record_boot(&target.map.0, target.x, target.y, &flags, &rom_sha);
    }
    Ok(Some(name))
}

#[tauri::command]
fn attach_status(app: tauri::State<'_, App>, on_status: Channel<PlaybackStatus>) -> Result<(), String> {
    send(&app, Command::AttachStatus(on_status))
}

/// Extracts raw bytes from either Tauri's binary IPC (`InvokeBody::Raw`) or
/// its JSON `postMessage` fallback (`InvokeBody::Json` array of byte values).
fn extract_binary_body(body: &InvokeBody, label: &str) -> Result<Vec<u8>, String> {
    match body {
        InvokeBody::Raw(bytes) => Ok(bytes.clone()),
        InvokeBody::Json(serde_json::Value::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let n = item
                    .as_u64()
                    .and_then(|v| u8::try_from(v).ok())
                    .ok_or_else(|| format!("expected {label} as binary bytes"))?;
                out.push(n);
            }
            Ok(out)
        }
        _ => Err(format!("expected {label} as a binary body")),
    }
}

/// Loads a ROM sent as the raw request body.
#[tauri::command]
fn load_rom(app: tauri::State<'_, App>, request: Request<'_>) -> Result<(), String> {
    let rom = extract_binary_body(request.body(), "the ROM bytes")?;
    let name = request
        .headers()
        .get("x-rom-name")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .unwrap_or_else(|| "rom.gba".to_owned());
    load(&app, PreparedLaunch::rom_only(name.clone(), rom))?;
    *app.rom_name.lock().unwrap() = name;
    Ok(())
}

#[tauri::command]
fn emu_set_rom_name(app: tauri::State<'_, App>, name: String) -> Result<(), String> {
    let clean = name.trim();
    if clean.is_empty() {
        return Ok(());
    }
    *app.rom_name.lock().unwrap() = clean.to_owned();
    send(&app, Command::SyncFrame(Some(clean.to_owned())))
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

#[tauri::command(async)]
fn emu_reset(app: tauri::State<'_, App>) -> Result<(), String> {
    if app.chat.busy() { return Err("the agent is working; wait for it or stop it".into()); }
    let (tx, rx) = mpsc::channel();
    send(&app, Command::Reset(tx))?;
    rx.recv().map_err(|_| "the emulator thread stopped".to_owned())?
}

#[tauri::command(async)]
fn emu_step_frame(app: tauri::State<'_, App>) -> Result<u32, String> {
    if app.chat.busy() { return Err("the agent is working; wait for it or stop it".into()); }
    let (tx, rx) = mpsc::channel();
    send(&app, Command::StepFrame(tx))?;
    rx.recv().map_err(|_| "the emulator thread stopped".to_owned())?
}

#[tauri::command]
fn emu_set_speed(app: tauri::State<'_, App>, speed: u8) -> Result<EmuConfig, String> {
    let updated = {
        let mut cfg = app.emu_config.write().unwrap();
        cfg.prefs.speed = speed;
        cfg.prefs.sanitize();
        cfg.clone()
    };
    save_config(&app.emu_data_dir, &updated)?;
    send(&app, Command::SetSpeed(updated.prefs.speed))?;
    Ok(updated)
}

#[tauri::command(async)]
fn emu_rewind(app: tauri::State<'_, App>) -> Result<u32, String> {
    if app.chat.busy() { return Err("the agent is working; wait for it or stop it".into()); }
    let (tx, rx) = mpsc::channel();
    send(&app, Command::Rewind(tx))?;
    rx.recv().map_err(|_| "the emulator thread stopped".to_owned())?
}

#[tauri::command]
fn emu_status(app: tauri::State<'_, App>) -> Result<PlaybackStatus, String> {
    let (tx, rx) = mpsc::channel();
    send(&app, Command::QueryStatus(tx))?;
    rx.recv().map_err(|_| "the emulator thread stopped".to_owned())
}

#[tauri::command(async)]
fn emu_screenshot(app: tauri::State<'_, App>) -> Result<SavedFileReceipt, String> {
    let mut session = app.emu_session.lock().map_err(|_| "emulator lock poisoned".to_owned())?;
    let name = app.rom_name.lock().unwrap().clone();
    save_screenshot(&app.emu_data_dir, &mut session, &name)
}

#[tauri::command(async)]
fn emu_export_save(app: tauri::State<'_, App>) -> Result<SavedFileReceipt, String> {
    if app.chat.busy() { return Err("the agent is working; wait for it or stop it".into()); }
    let mut session = app.emu_session.lock().map_err(|_| "emulator lock poisoned".to_owned())?;
    let name = app.rom_name.lock().unwrap().clone();
    export_save_file(&app.emu_data_dir, &mut session, &name)
}

#[tauri::command(async)]
fn emu_import_save(app: tauri::State<'_, App>, request: Request<'_>) -> Result<u32, String> {
    if app.chat.busy() { return Err("the agent is working; wait for it or stop it".into()); }
    let bytes = extract_binary_body(request.body(), ".sav bytes")?;
    let frame = {
        let mut session = app.emu_session.lock().map_err(|_| "emulator lock poisoned".to_owned())?;
        import_save_bytes(&mut session, &bytes)?
    };
    send(&app, Command::SyncFrame(None))?;
    Ok(frame)
}

#[tauri::command(async)]
fn emu_savestate_slots(app: tauri::State<'_, App>) -> Result<Vec<SavestateSlotStatus>, String> {
    let session = app.emu_session.lock().map_err(|_| "emulator lock poisoned".to_owned())?;
    Ok(list_quick_slots(&app.emu_data_dir, &session))
}

#[tauri::command(async)]
fn emu_save_slot(app: tauri::State<'_, App>, slot: u8) -> Result<SavedFileReceipt, String> {
    if app.chat.busy() { return Err("the agent is working; wait for it or stop it".into()); }
    let mut session = app.emu_session.lock().map_err(|_| "emulator lock poisoned".to_owned())?;
    save_quick_slot(&app.emu_data_dir, &mut session, slot)
}

#[tauri::command(async)]
fn emu_load_slot(app: tauri::State<'_, App>, slot: u8) -> Result<u32, String> {
    if app.chat.busy() { return Err("the agent is working; wait for it or stop it".into()); }
    let frame = {
        let mut session = app.emu_session.lock().map_err(|_| "emulator lock poisoned".to_owned())?;
        load_quick_slot(&app.emu_data_dir, &mut session, slot)?
    };
    send(&app, Command::SyncFrame(None))?;
    Ok(frame)
}

#[tauri::command(async)]
fn emu_save_state_file(app: tauri::State<'_, App>) -> Result<SavedFileReceipt, String> {
    if app.chat.busy() { return Err("the agent is working; wait for it or stop it".into()); }
    let mut session = app.emu_session.lock().map_err(|_| "emulator lock poisoned".to_owned())?;
    let name = app.rom_name.lock().unwrap().clone();
    save_state_to_file(&app.emu_data_dir, &mut session, &name)
}

#[tauri::command(async)]
fn emu_load_state_file(app: tauri::State<'_, App>, request: Request<'_>) -> Result<u32, String> {
    if app.chat.busy() { return Err("the agent is working; wait for it or stop it".into()); }
    let bytes = extract_binary_body(request.body(), "savestate bytes")?;
    let frame = {
        let mut session = app.emu_session.lock().map_err(|_| "emulator lock poisoned".to_owned())?;
        load_state_from_bytes(&mut session, &bytes)?
    };
    send(&app, Command::SyncFrame(None))?;
    Ok(frame)
}

#[derive(Serialize)]
struct EmuLogDto {
    level: String,
    category: i32,
    message: String,
}

#[tauri::command(async)]
fn emu_logs(app: tauri::State<'_, App>) -> Result<Vec<EmuLogDto>, String> {
    let mut session = app.emu_session.lock().map_err(|_| "emulator lock poisoned".to_owned())?;
    Ok(session
        .emulator_mut()
        .take_logs()
        .into_iter()
        .map(|entry| EmuLogDto {
            level: entry.level,
            category: entry.category,
            message: entry.message,
        })
        .collect())
}

#[tauri::command]
fn emu_get_config(app: tauri::State<'_, App>) -> EmuConfig {
    app.emu_config.read().unwrap().clone()
}

#[tauri::command]
fn emu_set_keymap(app: tauri::State<'_, App>, keymap: KeyMap) -> Result<EmuConfig, String> {
    let updated = {
        let mut cfg = app.emu_config.write().unwrap();
        let mut next_map = keymap;
        next_map.normalize_and_validate()?;
        cfg.keymap = next_map;
        cfg.clone()
    };
    save_config(&app.emu_data_dir, &updated)?;
    Ok(updated)
}

#[tauri::command]
fn emu_set_prefs(app: tauri::State<'_, App>, prefs: EmuPrefs) -> Result<EmuConfig, String> {
    let updated = {
        let mut cfg = app.emu_config.write().unwrap();
        let mut next_prefs = prefs;
        next_prefs.sanitize();
        cfg.prefs = next_prefs;
        cfg.clone()
    };
    save_config(&app.emu_data_dir, &updated)?;
    send(&app, Command::SetSpeed(updated.prefs.speed))?;
    send(&app, Command::SetRewindEnabled(updated.prefs.rewind_enabled))?;
    Ok(updated)
}

#[derive(Serialize)]
struct ChannelToggleDto {
    id: usize,
    name: String,
    enabled: bool,
}

#[derive(Serialize)]
struct EmuLayersDto {
    video: Vec<ChannelToggleDto>,
    audio: Vec<ChannelToggleDto>,
}

#[tauri::command(async)]
fn emu_layers(app: tauri::State<'_, App>) -> Result<EmuLayersDto, String> {
    let session = app.emu_session.lock().map_err(|_| "emulator lock poisoned".to_owned())?;
    let v_disabled = app.video_layers_disabled.lock().unwrap();
    let a_disabled = app.audio_channels_disabled.lock().unwrap();
    let video = session
        .emulator()
        .video_layers()
        .into_iter()
        .map(|c| ChannelToggleDto {
            id: c.id,
            name: c.visible_name,
            enabled: !v_disabled.contains(&c.id),
        })
        .collect();
    let audio = session
        .emulator()
        .audio_channels()
        .into_iter()
        .map(|c| ChannelToggleDto {
            id: c.id,
            name: c.visible_name,
            enabled: !a_disabled.contains(&c.id),
        })
        .collect();
    Ok(EmuLayersDto { video, audio })
}

#[tauri::command(async)]
fn emu_set_video_layer(app: tauri::State<'_, App>, id: usize, enabled: bool) -> Result<(), String> {
    {
        let mut session = app.emu_session.lock().map_err(|_| "emulator lock poisoned".to_owned())?;
        session.emulator_mut().set_video_layer(id, enabled);
    }
    let mut disabled = app.video_layers_disabled.lock().unwrap();
    if enabled { disabled.remove(&id); } else { disabled.insert(id); }
    let _ = send(&app, Command::SyncFrame(None));
    Ok(())
}

#[tauri::command(async)]
fn emu_set_audio_channel(app: tauri::State<'_, App>, id: usize, enabled: bool) -> Result<(), String> {
    {
        let mut session = app.emu_session.lock().map_err(|_| "emulator lock poisoned".to_owned())?;
        session.emulator_mut().set_audio_channel(id, enabled);
    }
    let mut disabled = app.audio_channels_disabled.lock().unwrap();
    if enabled { disabled.remove(&id); } else { disabled.insert(id); }
    Ok(())
}

#[tauri::command(async)]
fn emu_inspect_palettes(app: tauri::State<'_, App>) -> Result<PaletteDump, String> {
    let session = app.emu_session.lock().map_err(|_| "emulator lock poisoned".to_owned())?;
    if !session.has_rom() { return Err("no ROM loaded".into()); }
    read_palettes(session.emulator())
}

#[tauri::command(async)]
fn emu_inspect_tiles(
    app: tauri::State<'_, App>,
    base_addr: u32,
    bpp8: bool,
    palette_idx: u8,
    is_obj: bool,
) -> Result<TileSheetDto, String> {
    let session = app.emu_session.lock().map_err(|_| "emulator lock poisoned".to_owned())?;
    if !session.has_rom() { return Err("no ROM loaded".into()); }
    read_tiles_rgba(session.emulator(), base_addr, bpp8, palette_idx, is_obj)
}

#[tauri::command(async)]
fn emu_inspect_oam(app: tauri::State<'_, App>) -> Result<Vec<OamEntryDto>, String> {
    let session = app.emu_session.lock().map_err(|_| "emulator lock poisoned".to_owned())?;
    if !session.has_rom() { return Err("no ROM loaded".into()); }
    read_oam(session.emulator())
}

#[tauri::command(async)]
fn emu_memory_regions(app: tauri::State<'_, App>) -> Result<Vec<MemoryRegionDto>, String> {
    let session = app.emu_session.lock().map_err(|_| "emulator lock poisoned".to_owned())?;
    Ok(list_memory_regions(session.emulator()))
}

#[tauri::command(async)]
fn emu_read_memory(app: tauri::State<'_, App>, addr: u32, len: usize, deref: bool) -> Result<MemoryWindowDto, String> {
    let session = app.emu_session.lock().map_err(|_| "emulator lock poisoned".to_owned())?;
    read_memory_window(session.emulator(), addr, len, deref)
}

#[tauri::command(async)]
fn emu_search_symbols(app: tauri::State<'_, App>, query: String, limit: Option<usize>) -> Result<Vec<SymbolMatchDto>, String> {
    let session = app.emu_session.lock().map_err(|_| "emulator lock poisoned".to_owned())?;
    Ok(session
        .symbols()
        .search(&query, limit.unwrap_or(60))
        .into_iter()
        .map(|(name, addr)| SymbolMatchDto { name, addr })
        .collect())
}

fn launch_target_for_app(app: &App) -> Option<LastBootTarget> {
    let (Some(_), Some(Launch::Point { target, .. })) = (&app.project, &app.initial_launch) else {
        return None;
    };
    let rom_sha256 = app.emu_session.lock().ok().and_then(|s| s.rom_sha256().map(str::to_owned)).unwrap_or_default();
    Some(LastBootTarget {
        map: target.map.0.clone(),
        x: target.x,
        y: target.y,
        flags: target.flags.iter().map(|f| f.0.clone()).collect(),
        rom_sha256,
    })
}

#[tauri::command(async)]
fn project_context(
    app: tauri::State<'_, App>,
    selected_map: Option<String>,
    selected_x: Option<i16>,
    selected_y: Option<i16>,
) -> Option<AgentEmuContext> {
    let project = app.project.as_ref()?;
    Some(project.agent_context(selected_map, selected_x, selected_y, launch_target_for_app(&app)))
}

/// Sends a chat message; the turn's events stream to `on_event` until it completes.
/// Runs off the main thread: the first message starts the sidecar and MCP server.
#[tauri::command(async)]
fn chat_send(
    app: tauri::State<'_, App>,
    text: String,
    selected_map: Option<String>,
    selected_x: Option<i16>,
    selected_y: Option<i16>,
    on_event: Channel<AgentEvent>,
) -> Result<String, String> {
    let login = app.codex_login.lock().unwrap();
    if login.pending() { return Err("finish or cancel ChatGPT sign-in before sending".into()); }
    if app.antigravity_login.lock().unwrap().pending() { return Err("finish or cancel Google sign-in before sending".into()); }
    let payload = if let Some(project) = &app.project {
        let ctx = project.agent_context(selected_map, selected_x, selected_y, launch_target_for_app(&app));
        format!("{}\n{}", ctx.to_prompt_block(), text.trim())
    } else {
        text
    };
    let turn = app.chat.send(&payload, move |event| {
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
    app.chat.preset_model(app.chat_models.lock().unwrap().get(&provider).cloned());
    *app.chat_provider.lock().unwrap() = provider;
    Ok(())
}

/// Models of the selected provider, marking the one in use. Starts the agent
/// session if needed; no model turn runs.
#[tauri::command(async)]
fn chat_models(app: tauri::State<'_, App>) -> Result<Vec<tauros_agent_bridge::ModelOption>, String> {
    if app.codex_login.lock().unwrap().pending() || app.antigravity_login.lock().unwrap().pending() {
        return Err("finish or cancel sign-in first".into());
    }
    app.chat.models()
}

/// Applies `model` to the following turns and to new conversations of this provider.
#[tauri::command(async)]
fn chat_set_model(app: tauri::State<'_, App>, model: String) -> Result<(), String> {
    if model.trim().is_empty() || model.len() > 200 { return Err("invalid model".into()); }
    app.chat.set_model(&model)?;
    let provider = app.chat_provider.lock().unwrap().clone();
    app.chat_models.lock().unwrap().insert(provider, model);
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
    /// The launch's boot point; absent when the project opened without one.
    map: Option<String>,
    x: Option<i16>,
    y: Option<i16>,
    path: String,
    built: bool,
}

#[tauri::command(async)]
fn project_info(app: tauri::State<'_, App>) -> Result<Option<ProjectInfo>, String> {
    let (Some(project), Some(launch)) = (&app.project, &app.initial_launch) else { return Ok(None) };
    let Some(dir) = launch.project() else { return Ok(None) };
    let map_bounds = project.preview_maps()?;
    let point = match launch {
        Launch::Point { target, .. } => Some((target.map.0.clone(), target.x, target.y)),
        _ => None,
    };
    Ok(Some(ProjectInfo {
        maps: map_bounds.iter().map(|map| map.id.clone()).collect(),
        map_bounds,
        map: point.as_ref().map(|p| p.0.clone()),
        x: point.as_ref().map(|p| p.1),
        y: point.as_ref().map(|p| p.2),
        path: dir.display().to_string(),
        built: tauros_app::projects::is_built(dir),
    }))
}

/// `$TAUROS_DATA/projects`, the default projects folder.
#[tauri::command]
fn projects_default_dir() -> String {
    tauros_app::projects::default_projects_dir().display().to_string()
}

/// Game projects directly inside `dir`.
#[tauri::command(async)]
fn projects_list(dir: String) -> Result<Vec<tauros_app::projects::ProjectEntry>, String> {
    tauros_app::projects::list_projects(Path::new(&dir))
}

/// Opens another project by relaunching Tauros on it after the normal shutdown:
/// the project is wired into the emulator, the agent and its tools at startup.
#[tauri::command]
fn project_open(
    app: tauri::State<'_, App>,
    handle: tauri::AppHandle,
    path: String,
    map: Option<String>,
    x: Option<i16>,
    y: Option<i16>,
) -> Result<(), String> {
    let dir = PathBuf::from(&path);
    let point = match (&map, x, y) {
        (Some(map), Some(x), Some(y)) => Some((map.as_str(), x, y)),
        _ => None,
    };
    let args = tauros_app::projects::launch_args(&dir, point)?;
    // Fail here rather than in the relaunched process, which would exit.
    tauros_connector_gen3::Gen3Connector::open(&dir, None).map_err(|e| format!("cannot open {}: {e}", dir.display()))?;
    *app.relaunch.lock().unwrap() = Some(args);
    handle.exit(0);
    Ok(())
}

/// Starts the requested new instance once this one has finished its shutdown.
fn relaunch_after_exit(handle: &tauri::AppHandle, event: &tauri::RunEvent) {
    use tauri::Manager;
    if !matches!(event, tauri::RunEvent::Exit) { return; }
    let Some(args) = handle.state::<App>().relaunch.lock().unwrap().take() else { return };
    match std::env::current_exe().and_then(|exe| std::process::Command::new(exe).args(&args).spawn()) {
        Ok(child) => eprintln!("tauros: relaunched as process {}", child.id()),
        Err(error) => eprintln!("tauros: cannot relaunch: {error}"),
    }
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
    let label = format!("{map} ({x}, {y})");
    *app.rom_name.lock().unwrap() = label.clone();
    let _ = send(&app, Command::SyncFrame(Some(label)));
    eprintln!("tauros: built and booted {map} ({x}, {y}): {report:?}");
    Ok(report)
}

/// Builds the project without booting it. Refused while the agent is working.
#[tauri::command(async)]
fn project_build(app: tauri::State<'_, App>) -> Result<tauros_mcp::domain::BuildToolResult, String> {
    let project = app.project.as_ref().ok_or("no game project is open")?;
    if app.chat.busy() {
        return Err("the agent is working; wait for it or stop it".into());
    }
    project.build_only()
}

/// Runs the project's headless tests. Refused while the agent is working.
#[tauri::command(async)]
fn project_test(app: tauri::State<'_, App>, filter: Option<String>) -> Result<tauros_mcp::domain::RunTestToolResult, String> {
    let project = app.project.as_ref().ok_or("no game project is open")?;
    if app.chat.busy() {
        return Err("the agent is working; wait for it or stop it".into());
    }
    project.run_tests(filter.filter(|f| !f.trim().is_empty()))
}

/// Whether the player can start at a point, and the nearest walkable tile when not.
#[tauri::command(async)]
fn boot_point_check(app: tauri::State<'_, App>, map: String, x: i16, y: i16) -> Result<tauros_app::mcp_host::PointReport, String> {
    app.project.as_ref().ok_or("no game project is open")?.check_point(&map, x, y)
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
    let report = project.preview_built(&map, x, y, &flags)?;
    let label = format!("{map} ({x}, {y})");
    *app.rom_name.lock().unwrap() = label.clone();
    let _ = send(&app, Command::SyncFrame(Some(label)));
    Ok(report)
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

#[tauri::command]
fn voice_status(app: tauri::State<'_, App>) -> tauros_app::voice::VoiceStatus {
    app.voice.status()
}

#[tauri::command]
fn voice_start(app: tauri::State<'_, App>) -> Result<(), String> {
    if app.chat.busy() { return Err("Wait for the agent to finish before recording.".into()); }
    app.voice.start()
}

#[tauri::command(async)]
fn voice_finish(app: tauri::State<'_, App>, language: String) -> Result<String, String> {
    app.voice.finish(&language)
}

#[tauri::command]
fn voice_cancel(app: tauri::State<'_, App>) {
    app.voice.cancel();
}

#[tauri::command(async)]
fn claude_voice_start(app: tauri::State<'_, App>) -> Result<(), String> {
    if app.chat.busy() { return Err("Wait for the agent to finish before recording.".into()); }
    app.claude_voice.start()
}

#[tauri::command(async)]
fn claude_voice_finish(app: tauri::State<'_, App>) -> Result<String, String> {
    app.claude_voice.finish()
}

#[tauri::command(async)]
fn claude_voice_cancel(app: tauri::State<'_, App>) {
    app.claude_voice.cancel();
}

/// Runs one map generator step; its progress and state events stream to `on_event`.
#[tauri::command(async)]
fn mapgen_run(app: tauri::State<'_, App>, args: Vec<String>, on_event: Channel<serde_json::Value>) -> Result<serde_json::Value, String> {
    app.mapgen.run(args, |event| { let _ = on_event.send(event); })
}

#[tauri::command(async)]
fn mapgen_cancel(app: tauri::State<'_, App>) {
    app.mapgen.cancel();
}

/// A generated image from the mapgen data folder, as raw PNG bytes.
#[tauri::command(async)]
fn mapgen_image(app: tauri::State<'_, App>, path: String) -> Result<tauri::ipc::Response, String> {
    app.mapgen.image(&path).map(tauri::ipc::Response::new)
}

/// Reads the keyboard on the GTK window: key events reach the toplevel before
/// WebKit and any input method, so the game gets them whatever the page's focus.
#[cfg(target_os = "linux")]
fn native_keyboard(
    window: &tauri::WebviewWindow,
    commands: Sender<Command>,
    typing: Arc<AtomicBool>,
    emu_config: Arc<RwLock<EmuConfig>>,
) -> tauri::Result<()> {
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
        let mods = event.state();
        if mods.intersects(
            gtk::gdk::ModifierType::CONTROL_MASK
                | gtk::gdk::ModifierType::MOD1_MASK
                | gtk::gdk::ModifierType::SUPER_MASK,
        ) {
            return gtk::glib::Propagation::Proceed;
        }
        let name = event.keyval().name().map(|n| n.to_string()).unwrap_or_default();
        let bit = emu_config.read().unwrap().keymap.bit_for_raw_key(&name);
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
    let emu_data_dir = emulator_data_dir();
    let mut initial_config = load_config(&emu_data_dir);
    // Launch at normal speed; the chosen fast-forward multiplier stays in `ff_speed`.
    initial_config.prefs.start_at_normal_speed();
    let initial_speed = initial_config.prefs.speed;
    let initial_rewind = initial_config.prefs.rewind_enabled;
    let emu_config = Arc::new(RwLock::new(initial_config));
    let (tx, rx) = mpsc::channel();
    let typing = Arc::new(AtomicBool::new(false));
    let project = match initial_launch.as_ref().and_then(Launch::project) {
        Some(project) => {
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
    let worker_session = Arc::clone(&session);
    let chat_provider = Arc::new(Mutex::new("claude".to_owned()));
    let chat = Arc::new(project_chat(project.clone(), Arc::clone(&chat_provider)));
    let chat_holds = Arc::clone(&chat);
    #[cfg(target_os = "linux")]
    let (keyboard, keyboard_typing, keyboard_config) = (tx.clone(), typing.clone(), Arc::clone(&emu_config));
    std::thread::Builder::new()
        .name("emulator".into())
        .spawn(move || emulator_thread(rx, worker_session, move || chat_holds.busy()))
        .expect("emulator thread");
    let _ = tx.send(Command::SetSpeed(initial_speed));
    let _ = tx.send(Command::SetRewindEnabled(initial_rewind));
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(App {
            commands: Mutex::new(tx),
            emu_session: session,
            emu_config,
            emu_data_dir,
            rom_name: Mutex::new(String::new()),
            video_layers_disabled: Mutex::default(),
            audio_channels_disabled: Mutex::default(),
            initial_launch,
            project,
            chat,
            chat_provider,
            chat_models: Mutex::default(),
            codex_login: Mutex::new(tauros_app::codex_login::CodexLogin::default()),
            antigravity_login: Mutex::default(),
            typing,
            shutdown: Default::default(),
            voice: Default::default(),
            claude_voice: Default::default(),
            mapgen: Default::default(),
            relaunch: Mutex::default(),
        })
        .setup(move |app| {
            #[cfg(target_os = "linux")]
            {
                use tauri::Manager;
                let window = app.get_webview_window("main").expect("main window");
                native_keyboard(&window, keyboard.clone(), keyboard_typing.clone(), keyboard_config.clone())?;
            }
            let _ = app;
            Ok(())
        })
        .on_window_event(tauros_app::shutdown::on_window_event)
        .on_page_load(|window, payload| eprintln!("tauros: page {:?} {} in {}", payload.event(), payload.url(), window.label()))
        .invoke_handler(|invoke| {
            use tauri::Manager;
            if invoke.message.webview().state::<App>().shutdown.closing() {
                invoke.resolver.reject("Tauros is shutting down.");
                return true;
            }
            let handler: fn(tauri::ipc::Invoke<tauri::Wry>) -> bool = tauri::generate_handler![
            attach,
            attach_status,
            load_rom,
            emu_set_rom_name,
            log,
            set_buttons,
            set_paused,
            emu_reset,
            emu_step_frame,
            emu_set_speed,
            emu_rewind,
            emu_status,
            emu_screenshot,
            emu_export_save,
            emu_import_save,
            emu_savestate_slots,
            emu_save_slot,
            emu_load_slot,
            emu_save_state_file,
            emu_load_state_file,
            emu_logs,
            emu_get_config,
            emu_set_keymap,
            emu_set_prefs,
            emu_layers,
            emu_set_video_layer,
            emu_set_audio_channel,
            emu_inspect_palettes,
            emu_inspect_tiles,
            emu_inspect_oam,
            emu_memory_regions,
            emu_read_memory,
            emu_search_symbols,
            project_context,
            voice_status,
            voice_start,
            voice_finish,
            voice_cancel,
            claude_voice_start,
            claude_voice_finish,
            claude_voice_cancel,
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
            chat_models,
            chat_set_model,
            chat_info,
            set_typing,
            project_info,
            build_and_play,
            project_build,
            project_test,
            projects_default_dir,
            projects_list,
            project_open,
            preview_map,
            boot_point_check,
            mapgen_run,
            mapgen_cancel,
            mapgen_image
        ];
            handler(invoke)
        })
        .build(tauri::generate_context!())
        .expect("error while building Tauros")
        .run(|handle, event| {
            relaunch_after_exit(handle, &event);
            tauros_app::shutdown::on_event::<App, _>(handle, event)
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauros_emu::Button;

    #[test]
    fn default_keymap_maps_gdk_names_to_buttons() {
        let km = KeyMap::default();
        assert_eq!(km.bit_for_raw_key("x"), Some(0));
        assert_eq!(km.bit_for_raw_key("X"), Some(0));
        assert_eq!(km.bit_for_raw_key("Return"), Some(3));
        assert_eq!(km.bit_for_raw_key("BackSpace"), Some(2));
        assert_eq!(km.bit_for_raw_key("Left"), Some(5));
        assert_eq!(km.bit_for_raw_key("q"), None);
    }

    #[test]
    fn mask_bits_follow_button_order() {
        assert_eq!(buttons_from_mask(0), Buttons::NONE);
        let b = buttons_from_mask(1 | 1 << 3 | 1 << 9);
        assert_eq!(b.iter().collect::<Vec<_>>(), [Button::A, Button::Start, Button::L]);
    }

    #[test]
    fn extract_binary_body_accepts_raw_and_json_array_fallback() {
        assert_eq!(
            extract_binary_body(&InvokeBody::Raw(vec![1, 2, 255]), "rom").unwrap(),
            vec![1, 2, 255]
        );
        assert_eq!(
            extract_binary_body(&InvokeBody::Json(serde_json::json!([10, 20, 30])), "rom").unwrap(),
            vec![10, 20, 30]
        );
        assert!(extract_binary_body(&InvokeBody::Json(serde_json::json!([256])), "rom").is_err());
        assert!(extract_binary_body(&InvokeBody::Json(serde_json::json!({"a": 1})), "rom").is_err());
    }
}
