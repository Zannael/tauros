//! Original synthetic pixels only: no cartridge, project sources or filesystem.
use std::sync::{Arc, Mutex};
use tauros_emu::{Buttons, EmuError, Emulator, Frame, MemoryRegion};

pub struct SyntheticFramebuffer(pub Arc<Mutex<Frame>>);
impl Emulator for SyntheticFramebuffer {
    fn load_rom(&mut self, _: &[u8]) -> Result<(), EmuError> { Ok(()) }
    fn reset(&mut self) -> Result<(), EmuError> { Ok(()) }
    fn run_frame(&mut self) -> Result<(), EmuError> { Ok(()) }
    fn frame_count(&self) -> u32 { 0 }
    fn set_buttons(&mut self, _: Buttons) {}
    fn frame(&self) -> Frame { self.0.lock().unwrap().clone() }
    fn audio_rate(&self) -> u32 { 32768 }
    fn drain_audio(&mut self, _: &mut Vec<i16>) -> usize { 0 }
    fn memory_regions(&self) -> Vec<MemoryRegion> { vec![] }
    fn read_memory(&self, _: u32, _: usize) -> Result<Vec<u8>, EmuError> {
        Err(EmuError::Memory("synthetic framebuffer has no RAM".into()))
    }
    fn write_memory(&mut self, _: u32, _: &[u8]) -> Result<(), EmuError> {
        Err(EmuError::Memory("synthetic framebuffer has no RAM".into()))
    }
    fn save_state(&mut self) -> Result<Vec<u8>, EmuError> {
        Err(EmuError::State("synthetic framebuffer has no savestates".into()))
    }
    fn load_state(&mut self, _: &[u8]) -> Result<(), EmuError> {
        Err(EmuError::State("synthetic framebuffer has no savestates".into()))
    }
}

pub fn picture(right: bool, fragment: bool) -> Frame {
    let mut frame = Frame { width: 240, height: 160, rgba: vec![245; 240 * 160 * 4] };
    let square_x = if right { 168 } else { 32 };
    let old_x = if right { 32 } else { 168 };
    for y in 0i32..160 {
        for x in 0i32..240 {
            let colour = if (square_x..square_x + 40).contains(&x) && (96..136).contains(&y)
                || fragment && (old_x..old_x + 12).contains(&x) && (96..108).contains(&y) {
                [220, 20, 30, 255]
            } else if (x - 120).pow(2) + (y - 40).pow(2) <= 20 * 20 {
                [20, 70, 220, 255]
            } else { [245, 245, 245, 255] };
            let offset = (y * 240 + x) as usize * 4;
            frame.rgba[offset..offset + 4].copy_from_slice(&colour);
        }
    }
    frame
}
