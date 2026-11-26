//! The libmgba 0.10.5 implementation of [`Emulator`] (DEC-002), GBA core.
//!
//! All unsafe FFI stays in this module. The core owns a copy of the ROM and the
//! video buffer; both live as long as the core and are only replaced after the
//! previous ROM is unloaded.

use crate::{Buttons, ChannelInfo, EmuError, EmuLogEntry, Emulator, Frame, MemoryRegion, RomInfo};
use std::collections::VecDeque;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};
use std::sync::{Mutex, Once, OnceLock};
use tauros_mgba_sys as sys;

/// Output sample rate, the one mGBA's own frontends use.
const AUDIO_RATE: u32 = 32_768;
/// Stereo frames buffered inside the core between two drains (about 4 frames).
const AUDIO_BUFFER: usize = 2048;
/// Maximum number of mGBA log entries retained between UI polls.
const MAX_LOG_ENTRIES: usize = 256;

unsafe extern "C" {
    fn vsnprintf(
        s: *mut c_char,
        n: usize,
        format: *const c_char,
        arg: *mut sys::__va_list_tag,
    ) -> c_int;
    fn free(ptr: *mut c_void);
}

fn log_buffer() -> &'static Mutex<VecDeque<EmuLogEntry>> {
    static LOGS: OnceLock<Mutex<VecDeque<EmuLogEntry>>> = OnceLock::new();
    LOGS.get_or_init(|| Mutex::new(VecDeque::with_capacity(MAX_LOG_ENTRIES)))
}

fn log_level_name(level: sys::mLogLevel) -> &'static str {
    match level {
        sys::mLOG_FATAL => "FATAL",
        sys::mLOG_ERROR => "ERROR",
        sys::mLOG_WARN => "WARN",
        sys::mLOG_INFO => "INFO",
        sys::mLOG_DEBUG => "DEBUG",
        sys::mLOG_STUB => "STUB",
        sys::mLOG_GAME_ERROR => "GAME_ERROR",
        _ => "LOG",
    }
}

pub struct Mgba {
    core: *mut sys::mCore,
    rom: Option<Box<[u8]>>,
    video: Box<[u32]>,
    width: u32,
    height: u32,
    buttons: Buttons,
}

// The core is exclusively owned by this value and libmgba keeps no thread-local
// state for it, so it may move between threads (not be shared: it is not Sync).
unsafe impl Send for Mgba {}

unsafe extern "C" fn capture_log(
    _logger: *mut sys::mLogger,
    category: c_int,
    level: sys::mLogLevel,
    format: *const c_char,
    args: *mut sys::__va_list_tag,
) {
    // Skip high-volume debug/stub spam unless fatal/error/warn/info/game_error.
    if level & (sys::mLOG_FATAL | sys::mLOG_ERROR | sys::mLOG_WARN | sys::mLOG_INFO | sys::mLOG_GAME_ERROR) == 0 {
        return;
    }
    let message = if format.is_null() {
        String::new()
    } else {
        let mut buf = [0u8; 512];
        let written = unsafe { vsnprintf(buf.as_mut_ptr().cast(), buf.len(), format, args) };
        if written < 0 {
            unsafe { CStr::from_ptr(format) }.to_string_lossy().into_owned()
        } else {
            let len = (written as usize).min(buf.len() - 1);
            String::from_utf8_lossy(&buf[..len]).trim_end_matches(['\r', '\n']).to_owned()
        }
    };
    if message.is_empty() {
        return;
    }
    if let Ok(mut logs) = log_buffer().lock() {
        if logs.len() >= MAX_LOG_ENTRIES {
            logs.pop_front();
        }
        logs.push_back(EmuLogEntry {
            level: log_level_name(level).to_owned(),
            category,
            message,
        });
    }
}

/// mGBA logs to stderr by default; Tauros captures logs into an internal ring
/// buffer and surfaces them in the emulator log panel instead.
fn install_logger() {
    static ONCE: Once = Once::new();
    static mut LOGGER: sys::mLogger = sys::mLogger { log: Some(capture_log), filter: std::ptr::null_mut() };
    ONCE.call_once(|| unsafe { sys::mLogSetDefaultLogger(std::ptr::addr_of_mut!(LOGGER)) });
}

impl Mgba {
    /// A GBA core with no ROM loaded.
    pub fn gba() -> Result<Self, EmuError> {
        install_logger();
        unsafe {
            let core = sys::GBACoreCreate();
            if core.is_null() {
                return Err(EmuError::Core("GBACoreCreate returned null".into()));
            }
            if !((*core).init.unwrap())(core) {
                return Err(EmuError::Core("core init failed".into()));
            }
            sys::mCoreInitConfig(core, std::ptr::null());
            let (mut width, mut height) = (0u32, 0u32);
            ((*core).desiredVideoDimensions.unwrap())(core, &mut width, &mut height);
            let mut video = vec![0u32; (width * height) as usize].into_boxed_slice();
            ((*core).setVideoBuffer.unwrap())(core, video.as_mut_ptr().cast(), width as usize);
            ((*core).setAudioBufferSize.unwrap())(core, AUDIO_BUFFER);
            let clock = ((*core).frequency.unwrap())(core) as f64;
            for channel in 0..2 {
                sys::blip_set_rates(((*core).getAudioChannel.unwrap())(core, channel), clock, AUDIO_RATE as f64);
            }
            Ok(Mgba { core, rom: None, video, width, height, buttons: Buttons::NONE })
        }
    }

    /// The libmgba version this core was built from.
    pub fn version() -> String {
        unsafe { std::ffi::CStr::from_ptr(sys::projectVersion).to_string_lossy().into_owned() }
    }

    /// The region holding `[addr, addr + len)`, if any.
    fn region(&self, addr: u32, len: usize) -> Result<MemoryRegion, EmuError> {
        self.memory_regions()
            .into_iter()
            .find(|r| r.contains(addr, len))
            .ok_or_else(|| EmuError::Memory(format!("{len} bytes at {addr:#010x} are not inside one mapped region")))
    }

    fn key_bits(buttons: Buttons) -> u32 {
        // GBAKey numbers the buttons in Button's order.
        buttons.iter().map(|b| 1u32 << b as u32).sum()
    }
}

impl Drop for Mgba {
    fn drop(&mut self) {
        unsafe {
            if self.rom.is_some() {
                ((*self.core).unloadROM.unwrap())(self.core);
            }
            sys::mCoreConfigDeinit(&mut (*self.core).config);
            ((*self.core).deinit.unwrap())(self.core);
        }
    }
}

impl Emulator for Mgba {
    fn load_rom(&mut self, rom: &[u8]) -> Result<(), EmuError> {
        let mut bytes: Box<[u8]> = rom.into();
        unsafe {
            let vf = sys::VFileFromMemory(bytes.as_mut_ptr().cast(), bytes.len());
            if vf.is_null() {
                return Err(EmuError::Rom("cannot open the ROM bytes".into()));
            }
            if !sys::GBAIsROM(vf) {
                ((*vf).close.unwrap())(vf);
                return Err(EmuError::Rom("not a GBA ROM (no cartridge header signature)".into()));
            }
            if self.rom.take().is_some() {
                ((*self.core).unloadROM.unwrap())(self.core);
            }
            // On success the core owns `vf` and closes it on unload.
            if !((*self.core).loadROM.unwrap())(self.core, vf) {
                ((*vf).close.unwrap())(vf);
                return Err(EmuError::Rom("libmgba rejected the ROM".into()));
            }
        }
        self.rom = Some(bytes);
        self.reset()
    }

    fn reset(&mut self) -> Result<(), EmuError> {
        if self.rom.is_none() {
            return Err(EmuError::NoRom);
        }
        unsafe {
            ((*self.core).reset.unwrap())(self.core);
            for channel in 0..2 {
                sys::blip_clear(((*self.core).getAudioChannel.unwrap())(self.core, channel));
            }
            ((*self.core).setKeys.unwrap())(self.core, Self::key_bits(self.buttons));
        }
        Ok(())
    }

    fn run_frame(&mut self) -> Result<(), EmuError> {
        if self.rom.is_none() {
            return Err(EmuError::NoRom);
        }
        unsafe { ((*self.core).runFrame.unwrap())(self.core) };
        Ok(())
    }

    fn frame_count(&self) -> u32 {
        unsafe { ((*self.core).frameCounter.unwrap())(self.core) }
    }

    fn set_buttons(&mut self, buttons: Buttons) {
        self.buttons = buttons;
        unsafe { ((*self.core).setKeys.unwrap())(self.core, Self::key_bits(buttons)) };
    }

    fn frame(&self) -> Frame {
        // libmgba's 32-bit colour is 0x00BBGGRR (red in the low byte).
        let rgba = self.video.iter().flat_map(|&p| [p as u8, (p >> 8) as u8, (p >> 16) as u8, 255]).collect();
        Frame { width: self.width, height: self.height, rgba }
    }

    fn audio_rate(&self) -> u32 {
        AUDIO_RATE
    }

    fn drain_audio(&mut self, out: &mut Vec<i16>) -> usize {
        unsafe {
            let left = ((*self.core).getAudioChannel.unwrap())(self.core, 0);
            let right = ((*self.core).getAudioChannel.unwrap())(self.core, 1);
            let available = sys::blip_samples_avail(left).max(0) as usize;
            let start = out.len();
            out.resize(start + 2 * available, 0);
            let buf = out.as_mut_ptr().add(start);
            let read = sys::blip_read_samples(left, buf, available as c_int, 1);
            sys::blip_read_samples(right, buf.add(1), available as c_int, 1);
            out.truncate(start + 2 * read.max(0) as usize);
            read.max(0) as usize
        }
    }
    fn memory_regions(&self) -> Vec<MemoryRegion> {
        unsafe {
            let mut blocks: *const sys::mCoreMemoryBlock = std::ptr::null();
            let n = ((*self.core).listMemoryBlocks.unwrap())(self.core, &mut blocks);
            std::slice::from_raw_parts(blocks, n)
                .iter()
                .filter(|b| b.flags & sys::mCORE_MEMORY_MAPPED != 0 && b.flags & sys::mCORE_MEMORY_VIRTUAL == 0)
                .map(|b| MemoryRegion {
                    id: CStr::from_ptr(b.internalName).to_string_lossy().into_owned(),
                    name: CStr::from_ptr(b.shortName).to_string_lossy().into_owned(),
                    start: b.start,
                    size: b.size,
                    // libmgba cannot patch I/O registers (GBAPatch8/16 stub them).
                    writable: b.flags & sys::mCORE_MEMORY_WRITE != 0 && CStr::from_ptr(b.internalName).to_bytes() != b"io",
                })
                .collect()
        }
    }

    fn read_memory(&self, addr: u32, len: usize) -> Result<Vec<u8>, EmuError> {
        self.region(addr, len)?;
        let read = unsafe { (*self.core).rawRead8.unwrap() };
        Ok((0..len as u32).map(|i| unsafe { read(self.core, addr + i, -1) } as u8).collect())
    }

    fn write_memory(&mut self, addr: u32, bytes: &[u8]) -> Result<(), EmuError> {
        let region = self.region(addr, bytes.len())?;
        if !region.writable {
            return Err(EmuError::Memory(format!("{} ({}) is read-only", region.name, region.id)));
        }
        // Byte patches are stubbed for video memory, so write aligned halfwords,
        // merging the neighbouring byte read back from the core.
        let (read, write) = unsafe { ((*self.core).rawRead16.unwrap(), (*self.core).rawWrite16.unwrap()) };
        for (i, &b) in bytes.iter().enumerate() {
            let at = addr + i as u32;
            let half = unsafe { read(self.core, at & !1, -1) } as u16;
            let merged = if at & 1 == 0 { half & 0xFF00 | b as u16 } else { half & 0x00FF | (b as u16) << 8 };
            unsafe { write(self.core, at & !1, -1, merged) };
        }
        Ok(())
    }

    fn save_state(&mut self) -> Result<Vec<u8>, EmuError> {
        if self.rom.is_none() {
            return Err(EmuError::NoRom);
        }
        unsafe {
            let mut state = vec![0u8; ((*self.core).stateSize.unwrap())(self.core)];
            if !((*self.core).saveState.unwrap())(self.core, state.as_mut_ptr().cast()) {
                return Err(EmuError::State("libmgba could not save the state".into()));
            }
            Ok(state)
        }
    }

    fn load_state(&mut self, state: &[u8]) -> Result<(), EmuError> {
        if self.rom.is_none() {
            return Err(EmuError::NoRom);
        }
        unsafe {
            let size = ((*self.core).stateSize.unwrap())(self.core);
            if state.len() != size {
                return Err(EmuError::State(format!("state is {} bytes, this core expects {size}", state.len())));
            }
            if !((*self.core).loadState.unwrap())(self.core, state.as_ptr().cast()) {
                return Err(EmuError::State("libmgba rejected the state (different ROM or corrupted)".into()));
            }
            // Keep the held buttons: a state carries the keys of the moment it was taken.
            ((*self.core).setKeys.unwrap())(self.core, Self::key_bits(self.buttons));
        }
        Ok(())
    }

    fn rom_info(&self) -> Option<RomInfo> {
        self.rom.as_ref()?;
        unsafe {
            let mut title_buf = [0 as c_char; 16];
            let mut code_buf = [0 as c_char; 16];
            if let Some(get_title) = (*self.core).getGameTitle {
                get_title(self.core, title_buf.as_mut_ptr());
            }
            if let Some(get_code) = (*self.core).getGameCode {
                get_code(self.core, code_buf.as_mut_ptr());
            }
            let title = CStr::from_ptr(title_buf.as_ptr()).to_string_lossy().trim().to_owned();
            let code = CStr::from_ptr(code_buf.as_ptr()).to_string_lossy().trim().to_owned();
            let rom_size = (*self.core).romSize.map(|f| f(self.core)).unwrap_or(0);
            let mut save_ptr: *mut c_void = std::ptr::null_mut();
            let save_size = (*self.core)
                .savedataClone
                .map(|clone_fn| {
                    let sz = clone_fn(self.core, &mut save_ptr);
                    if !save_ptr.is_null() {
                        free(save_ptr);
                    }
                    sz
                })
                .unwrap_or(0);
            Some(RomInfo {
                title,
                code,
                rom_size,
                frame_count: self.frame_count(),
                save_size,
            })
        }
    }

    fn export_save(&mut self) -> Result<Vec<u8>, EmuError> {
        if self.rom.is_none() {
            return Err(EmuError::NoRom);
        }
        unsafe {
            let clone_fn = (*self.core)
                .savedataClone
                .ok_or_else(|| EmuError::State("libmgba savedataClone unavailable".into()))?;
            let mut ptr: *mut c_void = std::ptr::null_mut();
            let size = clone_fn(self.core, &mut ptr);
            if size == 0 || ptr.is_null() {
                if !ptr.is_null() {
                    free(ptr);
                }
                return Err(EmuError::State("no cartridge save memory is active for this ROM".into()));
            }
            let bytes = std::slice::from_raw_parts(ptr.cast::<u8>(), size).to_vec();
            free(ptr);
            Ok(bytes)
        }
    }

    fn import_save(&mut self, data: &[u8]) -> Result<(), EmuError> {
        if self.rom.is_none() {
            return Err(EmuError::NoRom);
        }
        if data.is_empty() {
            return Err(EmuError::State("save file is empty".into()));
        }
        unsafe {
            let restore_fn = (*self.core)
                .savedataRestore
                .ok_or_else(|| EmuError::State("libmgba savedataRestore unavailable".into()))?;
            if !restore_fn(self.core, data.as_ptr().cast(), data.len(), true) {
                return Err(EmuError::State(format!(
                    "libmgba rejected the {} byte save file",
                    data.len()
                )));
            }
        }
        Ok(())
    }

    fn take_logs(&mut self) -> Vec<EmuLogEntry> {
        log_buffer()
            .lock()
            .map(|mut q| q.drain(..).collect())
            .unwrap_or_default()
    }

    fn video_layers(&self) -> Vec<ChannelInfo> {
        unsafe { read_channel_list(self.core, (*self.core).listVideoLayers) }
    }

    fn set_video_layer(&mut self, id: usize, enabled: bool) {
        unsafe {
            if let Some(enable_fn) = (*self.core).enableVideoLayer {
                enable_fn(self.core, id, enabled);
            }
        }
    }

    fn audio_channels(&self) -> Vec<ChannelInfo> {
        unsafe { read_channel_list(self.core, (*self.core).listAudioChannels) }
    }

    fn set_audio_channel(&mut self, id: usize, enabled: bool) {
        unsafe {
            if let Some(enable_fn) = (*self.core).enableAudioChannel {
                enable_fn(self.core, id, enabled);
            }
        }
    }
}

unsafe fn read_channel_list(
    core: *const sys::mCore,
    list_fn: Option<unsafe extern "C" fn(*const sys::mCore, *mut *const sys::mCoreChannelInfo) -> usize>,
) -> Vec<ChannelInfo> {
    let Some(list_fn) = list_fn else {
        return Vec::new();
    };
    let mut ptr: *const sys::mCoreChannelInfo = std::ptr::null();
    let n = unsafe { list_fn(core, &mut ptr) };
    if ptr.is_null() || n == 0 {
        return Vec::new();
    }
    unsafe { std::slice::from_raw_parts(ptr, n) }
        .iter()
        .map(|c| ChannelInfo {
            id: c.id,
            internal_name: if c.internalName.is_null() {
                String::new()
            } else {
                unsafe { CStr::from_ptr(c.internalName) }.to_string_lossy().into_owned()
            },
            visible_name: if c.visibleName.is_null() {
                String::new()
            } else {
                unsafe { CStr::from_ptr(c.visibleName) }.to_string_lossy().into_owned()
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Button;

    #[test]
    fn key_bits_follow_gba_key_numbers() {
        assert_eq!(sys::GBA_KEY_A, 0);
        assert_eq!(sys::GBA_KEY_L, 9);
        let all: Buttons = Button::ALL.into_iter().collect();
        assert_eq!(Mgba::key_bits(all), 0x3FF);
        assert_eq!(Mgba::key_bits(Buttons::NONE.with(Button::Start).with(Button::Down)), 1 << 3 | 1 << 7);
    }
}
