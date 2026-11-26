//! Disk persistence helpers for screenshots, cartridge `.sav` files and savestate slots.

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tauros_mcp::emulator::{emu_load_state, emu_save_state, emu_screenshot, EmuScreenshotRequest, EmuSession, EmuStateRequest};

pub fn rom_slug(rom_name: &str, rom_sha256: Option<&str>) -> String {
    let base: String = rom_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let trimmed = base.trim_matches('-');
    let head = if trimmed.is_empty() { "rom" } else { trimmed };
    let short_sha = rom_sha256.map(|s| &s[..s.len().min(8)]).unwrap_or("unknown");
    format!("{}-{short_sha}", &head[..head.len().min(32)])
}

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Clone, Debug, Serialize)]
pub struct SavedFileReceipt {
    pub path: String,
    pub bytes: usize,
    pub frame_count: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct SavestateSlotStatus {
    pub slot: u8,
    pub available: bool,
}

fn slot_name(slot: u8) -> Result<String, String> {
    if !(1..=9).contains(&slot) {
        return Err("savestate slot must be between 1 and 9".into());
    }
    Ok(format!("slot-{slot}"))
}

fn slot_path(data_dir: &Path, rom_sha256: &str, slot: u8) -> PathBuf {
    let short_sha = &rom_sha256[..rom_sha256.len().min(12)];
    data_dir.join("states").join(format!("{short_sha}-slot{slot}.ss"))
}

pub fn save_screenshot(
    data_dir: &Path,
    session: &mut EmuSession,
    rom_name: &str,
) -> Result<SavedFileReceipt, String> {
    let dir = data_dir.join("screenshots");
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let slug = rom_slug(rom_name, session.rom_sha256());
    let frame = session.emulator().frame_count();
    let path = dir.join(format!("{slug}-f{frame}-{}.png", unix_secs()));
    let res = emu_screenshot(
        session,
        EmuScreenshotRequest {
            save_path: Some(path.display().to_string()),
            include_base64: false,
        },
    )
    .map_err(|e| e.message)?;
    let bytes = fs::metadata(&path).map(|m| m.len() as usize).unwrap_or(0);
    Ok(SavedFileReceipt {
        path: res.saved_path.unwrap_or_else(|| path.display().to_string()),
        bytes,
        frame_count: res.frame_count,
    })
}

pub fn export_save_file(
    data_dir: &Path,
    session: &mut EmuSession,
    rom_name: &str,
) -> Result<SavedFileReceipt, String> {
    if !session.has_rom() {
        return Err("no ROM is loaded".into());
    }
    let slug = rom_slug(rom_name, session.rom_sha256());
    let frame_count = session.emulator().frame_count();
    let bytes = session.emulator_mut().export_save().map_err(|e| e.to_string())?;
    let dir = data_dir.join("saves");
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let path = dir.join(format!("{slug}-{}.sav", unix_secs()));
    fs::write(&path, &bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(SavedFileReceipt {
        path: path.display().to_string(),
        bytes: bytes.len(),
        frame_count,
    })
}

pub fn import_save_bytes(session: &mut EmuSession, bytes: &[u8]) -> Result<u32, String> {
    if !session.has_rom() {
        return Err("no ROM is loaded".into());
    }
    session.emulator_mut().import_save(bytes).map_err(|e| e.to_string())?;
    Ok(session.emulator().frame_count())
}

pub fn save_quick_slot(
    data_dir: &Path,
    session: &mut EmuSession,
    slot: u8,
) -> Result<SavedFileReceipt, String> {
    let name = slot_name(slot)?;
    let res = emu_save_state(session, EmuStateRequest { name: name.clone() }).map_err(|e| e.message)?;
    let raw = session
        .savestate_bytes(&name)
        .ok_or_else(|| "savestate missing immediately after save".to_owned())?;
    let dir = data_dir.join("states");
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let path = slot_path(data_dir, &res.rom_sha256, slot);
    fs::write(&path, raw).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(SavedFileReceipt {
        path: path.display().to_string(),
        bytes: res.byte_len,
        frame_count: res.frame_count,
    })
}

pub fn load_quick_slot(
    data_dir: &Path,
    session: &mut EmuSession,
    slot: u8,
) -> Result<u32, String> {
    let name = slot_name(slot)?;
    if let Ok(res) = emu_load_state(session, EmuStateRequest { name }) {
        return Ok(res.frame_count);
    }
    let rom_sha = session
        .rom_sha256()
        .ok_or_else(|| "no ROM is loaded".to_owned())?
        .to_owned();
    let path = slot_path(data_dir, &rom_sha, slot);
    if !path.is_file() {
        return Err(format!("slot {slot} is empty for this ROM"));
    }
    let bytes = fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    session.import_savestate_bytes(&bytes).map_err(|e| e.message)
}

pub fn list_quick_slots(data_dir: &Path, session: &EmuSession) -> Vec<SavestateSlotStatus> {
    let mem = session.list_savestates();
    let sha = session.rom_sha256();
    (1..=9)
        .map(|slot| {
            let name = format!("slot-{slot}");
            let in_mem = mem.iter().any(|s| s == &name);
            let on_disk = sha.is_some_and(|s| slot_path(data_dir, s, slot).is_file());
            SavestateSlotStatus {
                slot,
                available: in_mem || on_disk,
            }
        })
        .collect()
}

pub fn save_state_to_file(
    data_dir: &Path,
    session: &mut EmuSession,
    rom_name: &str,
) -> Result<SavedFileReceipt, String> {
    let (bytes, sha, frame_count) = session.export_savestate_bytes().map_err(|e| e.message)?;
    let dir = data_dir.join("states");
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let slug = rom_slug(rom_name, Some(&sha));
    let path = dir.join(format!("{slug}-f{frame_count}-{}.ss", unix_secs()));
    fs::write(&path, &bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(SavedFileReceipt {
        path: path.display().to_string(),
        bytes: bytes.len(),
        frame_count,
    })
}

pub fn load_state_from_bytes(session: &mut EmuSession, bytes: &[u8]) -> Result<u32, String> {
    session.import_savestate_bytes(bytes).map_err(|e| e.message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauros_emu::Mgba;

    fn probe_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 0x200];
        rom[0xAC..0xB2].copy_from_slice(b"TAUR01");
        rom[0xB2] = 0x96;
        // Minimal ARM branch to self at 0x08000000: EAFFFFFE
        rom[0..4].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes());
        // GBA header checksum at 0xBD: -(sum(0xA0..0xBC) + 0x19)
        let sum = rom[0xA0..0xBD].iter().fold(0u8, |a, &b| a.wrapping_add(b));
        rom[0xBD] = 0u8.wrapping_sub(sum.wrapping_add(0x19));
        rom
    }

    #[test]
    fn quick_slots_and_screenshot_persist_under_data_dir() {
        let dir = std::env::temp_dir().join(format!("tauros-emu-files-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mgba = Mgba::gba().unwrap();
        let mut session = EmuSession::new(mgba).with_rom(probe_rom()).unwrap();
        session.emulator_mut().run_frame().unwrap();

        let receipt = save_quick_slot(&dir, &mut session, 2).unwrap();
        assert!(Path::new(&receipt.path).is_file());
        let slots = list_quick_slots(&dir, &session);
        assert!(slots.iter().find(|s| s.slot == 2).unwrap().available);
        assert!(!slots.iter().find(|s| s.slot == 1).unwrap().available);

        session.emulator_mut().run_frame().unwrap();
        assert_eq!(session.emulator().frame_count(), 2);
        let restored_frame = load_quick_slot(&dir, &mut session, 2).unwrap();
        assert_eq!(restored_frame, 1);

        let shot = save_screenshot(&dir, &mut session, "probe.gba").unwrap();
        assert!(Path::new(&shot.path).is_file());
        assert!(shot.bytes > 0);

        let ss_file = save_state_to_file(&dir, &mut session, "probe.gba").unwrap();
        let raw = fs::read(&ss_file.path).unwrap();
        assert_eq!(load_state_from_bytes(&mut session, &raw).unwrap(), 1);
        fs::remove_dir_all(dir).unwrap();
    }
}
