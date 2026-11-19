//! Paced emulator worker and binary frame transport used by the desktop window.
use crate::launch::PreparedLaunch;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauros_connector_gen3::boot::BootReport;
use tauros_emu::{Button, Buttons, Emulator, Mgba};

/// GBA refresh rate: 16.78 MHz / 280896 cycles per frame.
const FRAME_RATE: f64 = 59.7275;
/// Frames a press stays held even if its release arrives sooner. GTK can deliver
/// a key's press and release together when its main thread is busy streaming
/// frames; without this the game, which samples keys once per frame, misses it.
const MIN_HOLD_FRAMES: u32 = 4;

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

pub enum Command {
    Attach(Channel<InvokeResponseBody>),
    Load(PreparedLaunch, Sender<Result<Option<BootReport>, String>>),
    Buttons(Buttons),
    Pause(bool),
}

/// Buttons from a bit mask in `Button::ALL` order (bit 0 = A).
pub fn buttons_from_mask(mask: u16) -> Buttons {
    Button::ALL.into_iter().enumerate().filter(|(i, _)| mask & 1 << i != 0).map(|(_, b)| b).collect()
}

/// The emulator thread: applies commands, runs frames on schedule and streams
/// them to the attached window.
pub fn emulator_thread(commands: Receiver<Command>) {
    let mut emu = match Mgba::gba() {
        Ok(emu) => emu,
        Err(e) => return eprintln!("tauros: {e}"),
    };
    let period = Duration::from_secs_f64(1.0 / FRAME_RATE);
    let (mut channel, mut loaded, mut paused) = (None::<Channel<InvokeResponseBody>>, false, false);
    let mut next = Instant::now();
    let mut audio = Vec::new();
    let log_keys = std::env::var_os("TAUROS_LOG_KEYS").is_some();
    let mut latch = KeyLatch::default();
    loop {
        let running = loaded && !paused && channel.is_some();
        let wait = if running { next.saturating_duration_since(Instant::now()) } else { Duration::from_secs(3600) };
        match commands.recv_timeout(wait) {
            Ok(Command::Attach(c)) => channel = Some(c),
            Ok(Command::Load(launch, reply)) => {
                latch = KeyLatch::default();
                let result = emu
                    .load_rom(&launch.rom)
                    .map_err(|e| e.to_string())
                    .and_then(|()| launch.boot.as_ref().map(|plan| plan.boot(&mut emu).map_err(|e| e.to_string())).transpose());
                loaded = result.is_ok();
                // Boot audio belongs to skipped screens, not to the first displayed frame.
                audio.clear();
                emu.drain_audio(&mut audio);
                emu.set_buttons(Buttons::NONE);
                next = Instant::now();
                let _ = reply.send(result);
            }
            Ok(Command::Buttons(b)) => {
                let wanted = Button::ALL.iter().enumerate().filter(|(_, &x)| b.contains(x)).fold(0u16, |m, (i, _)| m | 1 << i);
                let held = latch.request(wanted, emu.frame_count());
                emu.set_buttons(buttons_from_mask(held));
                if log_keys {
                    eprintln!("tauros: frame {} buttons {:?}", emu.frame_count(), b.iter().collect::<Vec<_>>());
                }
            }
            Ok(Command::Pause(p)) => {
                paused = p;
                next = Instant::now();
            }
            Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {
                if emu.run_frame().is_err() {
                    continue;
                }
                let before = latch.held;
                if latch.tick(emu.frame_count()) != before {
                    emu.set_buttons(buttons_from_mask(latch.held));
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

#[cfg(test)]
mod tests {
    use super::*;
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
}
