//! Paced emulator worker and binary frame transport used by the desktop window.
use crate::launch::PreparedLaunch;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauros_connector_gen3::boot::BootReport;
use tauros_emu::{Button, Buttons};
use tauros_mcp::emulator::EmuSession;

/// GBA refresh rate: 16.78 MHz / 280896 cycles per frame.
const FRAME_RATE: f64 = 59.7275;
/// How often the window gets the current frame while the agent holds the game.
const HELD_REFRESH: Duration = Duration::from_millis(100);
/// Frames a press stays held even if its release arrives sooner. GTK can deliver
/// a key's press and release together when its main thread is busy streaming
/// frames; without this the game, which samples keys once per frame, misses it.
const MIN_HOLD_FRAMES: u32 = 4;
/// Capture a rewind snapshot every 15 frames (~4 snapshots per second at 1x).
const REWIND_INTERVAL_FRAMES: u32 = 15;
/// Retain up to 80 snapshots (~20 seconds of gameplay at 1x).
const MAX_REWIND_SNAPSHOTS: usize = 80;
/// Frames emulated per 60 Hz display tick when speed is set to Turbo (`0`).
const TURBO_FRAMES_PER_TICK: u8 = 12;

/// Turns the requested buttons into the ones the game sees, holding every press
/// for at least `MIN_HOLD_FRAMES` frames.
#[derive(Default)]
struct KeyLatch {
    wanted: u16,
    held: u16,
    /// Frame at which each button (bit) was last pressed.
    pressed_at: [u32; 10],
}

impl KeyLatch {
    /// A new request at `frame`; returns the buttons to hold now.
    fn request(&mut self, wanted: u16, frame: u32) -> u16 {
        for bit in 0..10 {
            if wanted & !self.wanted & 1 << bit != 0 {
                self.pressed_at[bit] = frame;
            }
        }
        self.wanted = wanted;
        self.held |= wanted;
        self.tick(frame)
    }

    /// Releases the buttons no longer wanted whose minimum hold is over.
    fn tick(&mut self, frame: u32) -> u16 {
        for bit in 0..10 {
            let mask = 1 << bit;
            if self.held & mask != 0 && self.wanted & mask == 0 && frame.wrapping_sub(self.pressed_at[bit]) >= MIN_HOLD_FRAMES {
                self.held &= !mask;
            }
        }
        self.held
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackMode {
    Idle,
    Playing,
    Paused,
    AgentHeld,
    FastForward,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RomInfoDto {
    pub title: String,
    pub code: String,
    pub rom_size: usize,
    pub save_size: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct PlaybackStatus {
    pub mode: PlaybackMode,
    pub loaded: bool,
    pub paused: bool,
    pub held: bool,
    pub speed: u8,
    pub fps: f32,
    pub frame_count: u32,
    pub rewind_count: usize,
    pub rom_name: String,
    pub rom_sha256: Option<String>,
    pub rom_info: Option<RomInfoDto>,
}

#[allow(clippy::large_enum_variant)]
pub enum Command {
    Attach(Channel<InvokeResponseBody>),
    AttachStatus(Channel<PlaybackStatus>),
    Load(PreparedLaunch, Sender<Result<Option<BootReport>, String>>),
    Buttons(Buttons),
    Pause(bool),
    Reset(Sender<Result<(), String>>),
    StepFrame(Sender<Result<u32, String>>),
    SetSpeed(u8),
    SetRewindEnabled(bool),
    Rewind(Sender<Result<u32, String>>),
    SyncFrame(Option<String>),
    QueryStatus(Sender<PlaybackStatus>),
}

/// Buttons from a bit mask in `Button::ALL` order (bit 0 = A).
pub fn buttons_from_mask(mask: u16) -> Buttons {
    Button::ALL.into_iter().enumerate().filter(|(i, _)| mask & 1 << i != 0).map(|(_, b)| b).collect()
}

struct RewindSnap {
    frame: u32,
    bytes: Vec<u8>,
}

fn snapshot_status(
    session: &EmuSession,
    paused: bool,
    holding: bool,
    speed: u8,
    fps: f32,
    rewind_count: usize,
    rom_name: &str,
) -> PlaybackStatus {
    let loaded = session.has_rom();
    let mode = if !loaded {
        PlaybackMode::Idle
    } else if holding {
        PlaybackMode::AgentHeld
    } else if paused {
        PlaybackMode::Paused
    } else if speed != 1 {
        PlaybackMode::FastForward
    } else {
        PlaybackMode::Playing
    };
    let (frame_count, rom_info) = if loaded {
        let emu = session.emulator();
        let dto = emu.rom_info().map(|info| RomInfoDto {
            title: info.title,
            code: info.code,
            rom_size: info.rom_size,
            save_size: info.save_size,
        });
        (emu.frame_count(), dto)
    } else {
        (0, None)
    };
    PlaybackStatus {
        mode,
        loaded,
        paused,
        held: holding,
        speed,
        fps: if matches!(mode, PlaybackMode::Playing | PlaybackMode::FastForward) {
            fps
        } else {
            0.0
        },
        frame_count,
        rewind_count,
        rom_name: rom_name.to_owned(),
        rom_sha256: session.rom_sha256().map(str::to_owned),
        rom_info,
    }
}

fn send_video(channel: &mut Option<Channel<InvokeResponseBody>>, message: Vec<u8>) {
    if let Some(c) = channel {
        if c.send(InvokeResponseBody::Raw(message)).is_err() {
            *channel = None;
        }
    }
}

fn send_status(status_channel: &mut Option<Channel<PlaybackStatus>>, status: &PlaybackStatus) {
    if let Some(c) = status_channel {
        if c.send(status.clone()).is_err() {
            *status_channel = None;
        }
    }
}

/// The emulator thread: applies commands, runs frames on schedule and streams
/// them to the attached window. The session is shared with the agent's MCP tools.
/// While `held()` is true (the agent is working) the game does not advance here:
/// the window gets the current frame a few times a second, so it shows what the
/// agent's tools do, and play resumes when the hold ends.
pub fn emulator_thread(commands: Receiver<Command>, session: Arc<Mutex<EmuSession>>, held: impl Fn() -> bool) {
    let period = Duration::from_secs_f64(1.0 / FRAME_RATE);
    let mut channel = None::<Channel<InvokeResponseBody>>;
    let mut status_channel = None::<Channel<PlaybackStatus>>;
    let (mut paused, mut was_held) = (false, false);
    let mut speed: u8 = 1;
    let mut rewind_enabled = true;
    let mut rewind_buffer: VecDeque<RewindSnap> = VecDeque::new();
    let mut last_rewind_frame: Option<u32> = None;
    let mut rom_name = String::new();
    let mut next = Instant::now();
    let mut audio = Vec::new();
    let log_keys = std::env::var_os("TAUROS_LOG_KEYS").is_some();
    let mut latch = KeyLatch::default();
    let mut fps_window_start = Instant::now();
    let mut fps_frames: u32 = 0;
    let mut measured_fps: f32 = 0.0;
    let mut last_status_push = Instant::now();

    let emit_status = |session: &EmuSession,
                       status_channel: &mut Option<Channel<PlaybackStatus>>,
                       paused: bool,
                       holding: bool,
                       speed: u8,
                       fps: f32,
                       rewind_count: usize,
                       rom_name: &str| {
        let st = snapshot_status(session, paused, holding, speed, fps, rewind_count, rom_name);
        send_status(status_channel, &st);
    };

    loop {
        let loaded = session.lock().unwrap().has_rom();
        let holding = held();
        if holding != was_held {
            was_held = holding;
            if let Ok(guard) = session.lock() {
                emit_status(&guard, &mut status_channel, paused, holding, speed, measured_fps, rewind_buffer.len(), &rom_name);
            }
            if !holding {
                if let Ok(mut guard) = session.lock() {
                    guard.emulator_mut().set_buttons(buttons_from_mask(latch.held));
                }
                next = Instant::now();
            }
        }
        let running = loaded && !paused && channel.is_some();
        let wait = match (running, holding) {
            (false, _) => Duration::from_millis(250),
            (true, true) => HELD_REFRESH,
            (true, false) => next.saturating_duration_since(Instant::now()),
        };
        match commands.recv_timeout(wait) {
            Ok(Command::Attach(c)) => channel = Some(c),
            Ok(Command::AttachStatus(c)) => {
                status_channel = Some(c);
                let guard = session.lock().unwrap();
                emit_status(&guard, &mut status_channel, paused, holding, speed, measured_fps, rewind_buffer.len(), &rom_name);
            }
            Ok(Command::Load(launch, reply)) => {
                latch = KeyLatch::default();
                rewind_buffer.clear();
                last_rewind_frame = None;
                fps_frames = 0;
                measured_fps = 0.0;
                fps_window_start = Instant::now();
                if !launch.name.is_empty() {
                    rom_name = launch.name.clone();
                }
                let mut session = session.lock().unwrap();
                let result = session.load_rom(launch.rom).and_then(|_| {
                    launch.boot.as_ref().map(|plan| plan.boot(session.emulator_mut()).map_err(|e| e.to_string())).transpose()
                });
                let emu = session.emulator_mut();
                // Boot audio belongs to skipped screens, not to the first displayed frame.
                audio.clear();
                emu.drain_audio(&mut audio);
                emu.set_buttons(Buttons::NONE);
                let first_frame = result.is_ok().then(|| emu.frame().rgba);
                if let Some(rgba) = first_frame {
                    if session.has_rom() {
                        send_video(&mut channel, rgba);
                    }
                }
                emit_status(&session, &mut status_channel, paused, holding, speed, measured_fps, rewind_buffer.len(), &rom_name);
                next = Instant::now();
                let _ = reply.send(result);
            }
            Ok(Command::Buttons(b)) => {
                let mut session = session.lock().unwrap();
                let emu = session.emulator_mut();
                let wanted = Button::ALL.iter().enumerate().filter(|(_, &x)| b.contains(x)).fold(0u16, |m, (i, _)| m | 1 << i);
                let held_mask = latch.request(wanted, emu.frame_count());
                if !holding {
                    emu.set_buttons(buttons_from_mask(held_mask));
                }
                if log_keys {
                    eprintln!("tauros: frame {} buttons {:?}", emu.frame_count(), b.iter().collect::<Vec<_>>());
                }
            }
            Ok(Command::Pause(p)) => {
                paused = p;
                next = Instant::now();
                fps_frames = 0;
                fps_window_start = Instant::now();
                if paused {
                    measured_fps = 0.0;
                }
                let guard = session.lock().unwrap();
                emit_status(&guard, &mut status_channel, paused, holding, speed, measured_fps, rewind_buffer.len(), &rom_name);
            }
            Ok(Command::Reset(reply)) => {
                let mut guard = session.lock().unwrap();
                let res = if !guard.has_rom() {
                    Err("no ROM loaded".into())
                } else {
                    latch = KeyLatch::default();
                    rewind_buffer.clear();
                    last_rewind_frame = None;
                    let emu = guard.emulator_mut();
                    let r = emu.reset().map_err(|e| e.to_string());
                    if r.is_ok() {
                        audio.clear();
                        emu.drain_audio(&mut audio);
                        send_video(&mut channel, emu.frame().rgba);
                    }
                    r
                };
                emit_status(&guard, &mut status_channel, paused, holding, speed, measured_fps, rewind_buffer.len(), &rom_name);
                next = Instant::now();
                let _ = reply.send(res);
            }
            Ok(Command::StepFrame(reply)) => {
                let mut guard = session.lock().unwrap();
                let res = if !guard.has_rom() {
                    Err("no ROM loaded".into())
                } else if holding {
                    Err("emulator is held by the agent".into())
                } else {
                    paused = true;
                    measured_fps = 0.0;
                    let emu = guard.emulator_mut();
                    emu.set_buttons(buttons_from_mask(latch.held));
                    match emu.run_frame() {
                        Ok(()) => {
                            let fc = emu.frame_count();
                            let before = latch.held;
                            if latch.tick(fc) != before {
                                emu.set_buttons(buttons_from_mask(latch.held));
                            }
                            audio.clear();
                            emu.drain_audio(&mut audio);
                            send_video(&mut channel, emu.frame().rgba);
                            Ok(fc)
                        }
                        Err(e) => Err(e.to_string()),
                    }
                };
                emit_status(&guard, &mut status_channel, paused, holding, speed, measured_fps, rewind_buffer.len(), &rom_name);
                let _ = reply.send(res);
            }
            Ok(Command::SetSpeed(next_speed)) => {
                speed = match next_speed {
                    0 | 1 | 2 | 4 | 8 => next_speed,
                    _ => 1,
                };
                fps_frames = 0;
                fps_window_start = Instant::now();
                next = Instant::now();
                let guard = session.lock().unwrap();
                emit_status(&guard, &mut status_channel, paused, holding, speed, measured_fps, rewind_buffer.len(), &rom_name);
            }
            Ok(Command::SetRewindEnabled(enabled)) => {
                rewind_enabled = enabled;
                if !enabled {
                    rewind_buffer.clear();
                    last_rewind_frame = None;
                }
                let guard = session.lock().unwrap();
                emit_status(&guard, &mut status_channel, paused, holding, speed, measured_fps, rewind_buffer.len(), &rom_name);
            }
            Ok(Command::Rewind(reply)) => {
                let mut guard = session.lock().unwrap();
                let res = if !guard.has_rom() {
                    Err("no ROM loaded".into())
                } else if holding {
                    Err("emulator is held by the agent".into())
                } else {
                    let current_frame = guard.emulator().frame_count();
                    while rewind_buffer.len() > 1
                        && rewind_buffer.back().is_some_and(|s| current_frame.saturating_sub(s.frame) <= 2)
                    {
                        rewind_buffer.pop_back();
                    }
                    if let Some(snap) = rewind_buffer.pop_back() {
                        let emu = guard.emulator_mut();
                        match emu.load_state(&snap.bytes) {
                            Ok(()) => {
                                audio.clear();
                                emu.drain_audio(&mut audio);
                                last_rewind_frame = Some(snap.frame);
                                send_video(&mut channel, emu.frame().rgba);
                                Ok(emu.frame_count())
                            }
                            Err(e) => Err(e.to_string()),
                        }
                    } else {
                        Err("no rewind history available yet".into())
                    }
                };
                emit_status(&guard, &mut status_channel, paused, holding, speed, measured_fps, rewind_buffer.len(), &rom_name);
                next = Instant::now();
                let _ = reply.send(res);
            }
            Ok(Command::SyncFrame(name_override)) => {
                if let Some(name) = name_override {
                    rom_name = name;
                }
                let mut guard = session.lock().unwrap();
                if guard.has_rom() {
                    let emu = guard.emulator_mut();
                    audio.clear();
                    emu.drain_audio(&mut audio);
                    send_video(&mut channel, emu.frame().rgba);
                }
                emit_status(&guard, &mut status_channel, paused, holding, speed, measured_fps, rewind_buffer.len(), &rom_name);
            }
            Ok(Command::QueryStatus(reply)) => {
                let holding = held();
                let guard = session.lock().unwrap();
                let _ = reply.send(snapshot_status(&guard, paused, holding, speed, measured_fps, rewind_buffer.len(), &rom_name));
            }
            Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) if !running => {
                if last_status_push.elapsed() >= Duration::from_millis(500) {
                    last_status_push = Instant::now();
                    if let Ok(guard) = session.lock() {
                        emit_status(&guard, &mut status_channel, paused, holding, speed, measured_fps, rewind_buffer.len(), &rom_name);
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                let mut session = session.lock().unwrap();
                let emu = session.emulator_mut();
                if holding {
                    // Show the agent's state; its tools own the input meanwhile.
                    was_held = true;
                    emu.drain_audio(&mut audio);
                    audio.clear();
                    let message = emu.frame().rgba;
                    drop(session);
                    send_video(&mut channel, message);
                    continue;
                }
                if was_held {
                    was_held = false;
                    emu.set_buttons(buttons_from_mask(latch.held));
                    next = Instant::now();
                }
                let steps = if speed == 0 { TURBO_FRAMES_PER_TICK } else { speed };
                let mut stepped = 0u32;
                for _ in 0..steps {
                    if emu.run_frame().is_err() {
                        break;
                    }
                    stepped += 1;
                    let fc = emu.frame_count();
                    let before = latch.held;
                    if latch.tick(fc) != before {
                        emu.set_buttons(buttons_from_mask(latch.held));
                    }
                    if rewind_enabled
                        && last_rewind_frame.is_none_or(|prev| fc.saturating_sub(prev) >= REWIND_INTERVAL_FRAMES)
                    {
                        if let Ok(bytes) = emu.save_state() {
                            if rewind_buffer.len() >= MAX_REWIND_SNAPSHOTS {
                                rewind_buffer.pop_front();
                            }
                            rewind_buffer.push_back(RewindSnap { frame: fc, bytes });
                            last_rewind_frame = Some(fc);
                        }
                    }
                }
                fps_frames += stepped;
                let elapsed = fps_window_start.elapsed();
                if elapsed >= Duration::from_secs(1) {
                    measured_fps = fps_frames as f32 / elapsed.as_secs_f32();
                    fps_frames = 0;
                    fps_window_start = Instant::now();
                }
                audio.clear();
                emu.drain_audio(&mut audio);
                let mut message = emu.frame().rgba;
                if last_status_push.elapsed() >= Duration::from_millis(250) {
                    last_status_push = Instant::now();
                    emit_status(&session, &mut status_channel, paused, holding, speed, measured_fps, rewind_buffer.len(), &rom_name);
                }
                drop(session);
                // Stream audio only at 1x so fast-forward never overflows Web Audio.
                if speed == 1 {
                    message.extend(audio.iter().flat_map(|s| s.to_le_bytes()));
                }
                send_video(&mut channel, message);
                next += period;
                // After a stall (window hidden, machine busy) resume without a burst.
                if Instant::now() > next + period * 4 {
                    next = Instant::now();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tauros_emu::Mgba;

    fn probe_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 0x200];
        rom[0xAC..0xB2].copy_from_slice(b"TAUR01");
        rom[0xB2] = 0x96;
        rom[0..4].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes());
        let sum = rom[0xA0..0xBD].iter().fold(0u8, |a, &b| a.wrapping_add(b));
        rom[0xBD] = 0u8.wrapping_sub(sum.wrapping_add(0x19));
        rom
    }

    #[test]
    fn short_taps_are_held_for_the_minimum_frames() {
        let mut latch = KeyLatch::default();
        // Press and release arrive in the same gap between frames 100 and 101.
        assert_eq!(latch.request(1, 100), 1);
        assert_eq!(latch.request(0, 100), 1, "released before the game saw it");
        assert_eq!(latch.tick(102), 1);
        assert_eq!(latch.tick(104), 0);
        // A long hold is released as soon as asked.
        latch.request(1 << 3, 200);
        assert_eq!(latch.tick(230), 1 << 3);
        assert_eq!(latch.request(0, 230), 0);
        // Other buttons are unaffected by one button's latch.
        latch.request(1, 300);
        assert_eq!(latch.request(1 | 1 << 4, 310), 1 | 1 << 4);
        assert_eq!(latch.request(1, 310), 1 | 1 << 4);
        assert_eq!(latch.tick(314), 1);
    }

    #[test]
    fn step_reset_speed_rewind_and_held_status_work() {
        let session = Arc::new(Mutex::new(EmuSession::new(Mgba::gba().unwrap())));
        let held = Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::channel();
        let (ws, wh) = (Arc::clone(&session), Arc::clone(&held));
        let worker = std::thread::spawn(move || emulator_thread(rx, ws, move || wh.load(Ordering::SeqCst)));

        let (frames_tx, frames_rx) = std::sync::mpsc::channel();
        tx.send(Command::Attach(Channel::new(move |body| {
            if let InvokeResponseBody::Raw(b) = body {
                let _ = frames_tx.send(b);
            }
            Ok(())
        }))).unwrap();

        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        tx.send(Command::Pause(true)).unwrap();
        tx.send(Command::Load(PreparedLaunch::rom_only("probe.gba".into(), probe_rom()), reply_tx)).unwrap();
        reply_rx.recv().unwrap().unwrap();

        // StepFrame advances exactly 1 frame while paused and pushes a video frame.
        let (step_tx, step_rx) = std::sync::mpsc::channel();
        tx.send(Command::StepFrame(step_tx)).unwrap();
        assert_eq!(step_rx.recv().unwrap().unwrap(), 1);
        assert!(!frames_rx.recv_timeout(Duration::from_secs(1)).unwrap().is_empty());

        // Fast forward 4x runs multiple frames and reports FastForward mode, then rewind restores earlier frame.
        tx.send(Command::SetSpeed(4)).unwrap();
        tx.send(Command::Pause(false)).unwrap();
        std::thread::sleep(Duration::from_millis(180));
        tx.send(Command::Pause(true)).unwrap();
        let (st_tx, st_rx) = std::sync::mpsc::channel();
        tx.send(Command::QueryStatus(st_tx)).unwrap();
        let status = st_rx.recv().unwrap();
        assert!(status.paused);
        assert!(status.frame_count >= 20, "expected fast-forward frames, got {}", status.frame_count);
        assert!(status.rewind_count >= 1);

        let before_rewind = status.frame_count;
        let (rw_tx, rw_rx) = std::sync::mpsc::channel();
        tx.send(Command::Rewind(rw_tx)).unwrap();
        let rewound = rw_rx.recv().unwrap().unwrap();
        assert!(rewound < before_rewind);

        // Agent hold reports AgentHeld with 0.0 fps (never 10.0 fps).
        held.store(true, Ordering::SeqCst);
        let (st2_tx, st2_rx) = std::sync::mpsc::channel();
        tx.send(Command::QueryStatus(st2_tx)).unwrap();
        let held_status = st2_rx.recv().unwrap();
        assert_eq!(held_status.mode, PlaybackMode::AgentHeld);
        assert_eq!(held_status.fps, 0.0);
        held.store(false, Ordering::SeqCst);

        // Reset restores frame_count to 0 and clears rewind buffer.
        let (rst_tx, rst_rx) = std::sync::mpsc::channel();
        tx.send(Command::Reset(rst_tx)).unwrap();
        rst_rx.recv().unwrap().unwrap();
        let (st3_tx, st3_rx) = std::sync::mpsc::channel();
        tx.send(Command::QueryStatus(st3_tx)).unwrap();
        let reset_status = st3_rx.recv().unwrap();
        assert_eq!(reset_status.frame_count, 0);
        assert_eq!(reset_status.rewind_count, 0);

        drop(tx);
        worker.join().unwrap();
    }
}
