//! GBA hardware state decoders for the Palette, Tile/VRAM, OAM/Sprite, and
//! Memory & Symbol inspectors (`APP-003` Block 2).

use serde::Serialize;
use tauros_emu::Emulator;

const PALETTE_RAM_BASE: u32 = 0x0500_0000;
const PALETTE_RAM_SIZE: usize = 0x400;
const VRAM_BASE: u32 = 0x0600_0000;
const VRAM_END: u32 = 0x0601_8000;
const OAM_BASE: u32 = 0x0700_0000;
const OAM_SIZE: usize = 0x400;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PaletteColorDto {
    pub index: usize,
    pub bgr555: u16,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub hex: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PaletteDump {
    pub bg: Vec<PaletteColorDto>,
    pub obj: Vec<PaletteColorDto>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TileSheetDto {
    pub base_addr: u32,
    pub bpp8: bool,
    pub palette_idx: u8,
    pub is_obj: bool,
    pub width: u32,
    pub height: u32,
    pub tile_count: usize,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OamEntryDto {
    pub id: usize,
    pub obj_mode: &'static str,
    pub gfx_mode: &'static str,
    pub mosaic: bool,
    pub bpp: u8,
    pub x: i16,
    pub y: u8,
    pub width: u8,
    pub height: u8,
    pub tile: u16,
    pub palette: u8,
    pub priority: u8,
    pub hflip: bool,
    pub vflip: bool,
    pub affine_idx: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MemoryRegionDto {
    pub id: String,
    pub name: String,
    pub start: u32,
    pub size: u32,
    pub writable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MemoryWindowDto {
    pub requested_addr: u32,
    pub resolved_addr: u32,
    pub deref_ptr: Option<u32>,
    pub region_id: String,
    pub region_name: String,
    pub bytes: Vec<u8>,
    pub u8_val: Option<u8>,
    pub u16_val: Option<u16>,
    pub u32_val: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SymbolMatchDto {
    pub name: String,
    pub addr: u32,
}

fn expand_5bit(v: u16) -> u8 {
    let c = (v & 0x1F) as u8;
    (c << 3) | (c >> 2)
}

pub fn decode_bgr555(bgr555: u16) -> (u8, u8, u8) {
    (
        expand_5bit(bgr555),
        expand_5bit(bgr555 >> 5),
        expand_5bit(bgr555 >> 10),
    )
}

fn decode_palette_bank(bytes: &[u8]) -> Vec<PaletteColorDto> {
    bytes
        .chunks_exact(2)
        .enumerate()
        .map(|(index, pair)| {
            let bgr555 = u16::from_le_bytes([pair[0], pair[1]]) & 0x7FFF;
            let (r, g, b) = decode_bgr555(bgr555);
            PaletteColorDto {
                index,
                bgr555,
                r,
                g,
                b,
                hex: format!("#{r:02X}{g:02X}{b:02X}"),
            }
        })
        .collect()
}

pub fn read_palettes(emu: &dyn Emulator) -> Result<PaletteDump, String> {
    let raw = emu
        .read_memory(PALETTE_RAM_BASE, PALETTE_RAM_SIZE)
        .map_err(|e| e.to_string())?;
    if raw.len() < PALETTE_RAM_SIZE {
        return Err("incomplete Palette RAM read".into());
    }
    Ok(PaletteDump {
        bg: decode_palette_bank(&raw[..512]),
        obj: decode_palette_bank(&raw[512..1024]),
    })
}

pub fn read_tiles_rgba(
    emu: &dyn Emulator,
    base_addr: u32,
    bpp8: bool,
    palette_idx: u8,
    is_obj: bool,
) -> Result<TileSheetDto, String> {
    if !(VRAM_BASE..VRAM_END).contains(&base_addr) || !base_addr.is_multiple_of(32) {
        return Err(format!("invalid VRAM base address {base_addr:#010x}"));
    }
    let avail = (VRAM_END - base_addr) as usize;
    let read_len = avail.min(0x4000);
    let vram = emu.read_memory(base_addr, read_len).map_err(|e| e.to_string())?;
    let pal_addr = PALETTE_RAM_BASE + if is_obj { 0x200 } else { 0 };
    let pal_bytes = emu.read_memory(pal_addr, 512).map_err(|e| e.to_string())?;
    let pal_colors: Vec<(u8, u8, u8)> = pal_bytes
        .chunks_exact(2)
        .map(|p| decode_bgr555(u16::from_le_bytes([p[0], p[1]])))
        .collect();

    let bytes_per_tile = if bpp8 { 64 } else { 32 };
    let tile_count = vram.len() / bytes_per_tile;
    let cols = 16usize;
    let rows = tile_count.div_ceil(cols).max(1);
    let width = (cols * 8) as u32;
    let height = (rows * 8) as u32;
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    let pal_bank = (palette_idx & 0x0F) as usize * 16;

    for tile in 0..tile_count {
        let tx = (tile % cols) * 8;
        let ty = (tile / cols) * 8;
        let src = &vram[tile * bytes_per_tile..(tile + 1) * bytes_per_tile];
        for py in 0..8usize {
            for px in 0..8usize {
                let color_idx = if bpp8 {
                    src[py * 8 + px] as usize
                } else {
                    let byte = src[py * 4 + px / 2];
                    let nibble = if px & 1 == 0 { byte & 0x0F } else { byte >> 4 };
                    if nibble == 0 {
                        0
                    } else {
                        pal_bank + nibble as usize
                    }
                };
                if color_idx == 0 {
                    continue;
                }
                let (r, g, b) = pal_colors.get(color_idx).copied().unwrap_or((255, 0, 255));
                let out_idx = ((ty + py) * width as usize + (tx + px)) * 4;
                rgba[out_idx] = r;
                rgba[out_idx + 1] = g;
                rgba[out_idx + 2] = b;
                rgba[out_idx + 3] = 255;
            }
        }
    }

    Ok(TileSheetDto {
        base_addr,
        bpp8,
        palette_idx: palette_idx & 0x0F,
        is_obj,
        width,
        height,
        tile_count,
        rgba,
    })
}

fn sprite_dimensions(shape: u16, size: u16) -> (u8, u8) {
    match (shape, size) {
        (0, 0) => (8, 8),
        (0, 1) => (16, 16),
        (0, 2) => (32, 32),
        (0, 3) => (64, 64),
        (1, 0) => (16, 8),
        (1, 1) => (32, 8),
        (1, 2) => (32, 16),
        (1, 3) => (64, 32),
        (2, 0) => (8, 16),
        (2, 1) => (8, 32),
        (2, 2) => (16, 32),
        (2, 3) => (32, 64),
        _ => (8, 8),
    }
}

pub fn read_oam(emu: &dyn Emulator) -> Result<Vec<OamEntryDto>, String> {
    let raw = emu.read_memory(OAM_BASE, OAM_SIZE).map_err(|e| e.to_string())?;
    if raw.len() < OAM_SIZE {
        return Err("incomplete OAM read".into());
    }
    Ok(raw
        .chunks_exact(8)
        .enumerate()
        .map(|(id, entry)| {
            let attr0 = u16::from_le_bytes([entry[0], entry[1]]);
            let attr1 = u16::from_le_bytes([entry[2], entry[3]]);
            let attr2 = u16::from_le_bytes([entry[4], entry[5]]);
            let obj_mode_bits = (attr0 >> 8) & 3;
            let obj_mode = match obj_mode_bits {
                0 => "Normal",
                1 => "Affine",
                2 => "Hidden",
                _ => "Affine 2x",
            };
            let gfx_mode = match (attr0 >> 10) & 3 {
                0 => "Normal",
                1 => "Semi-transparent",
                2 => "OBJ Window",
                _ => "Prohibited",
            };
            let is_affine = obj_mode_bits == 1 || obj_mode_bits == 3;
            let raw_x = attr1 & 0x1FF;
            let x = if raw_x >= 256 { raw_x as i16 - 512 } else { raw_x as i16 };
            let (width, height) = sprite_dimensions((attr0 >> 14) & 3, (attr1 >> 14) & 3);
            OamEntryDto {
                id,
                obj_mode,
                gfx_mode,
                mosaic: (attr0 >> 12) & 1 != 0,
                bpp: if (attr0 >> 13) & 1 != 0 { 8 } else { 4 },
                x,
                y: (attr0 & 0xFF) as u8,
                width,
                height,
                tile: attr2 & 0x3FF,
                palette: ((attr2 >> 12) & 0xF) as u8,
                priority: ((attr2 >> 10) & 3) as u8,
                hflip: !is_affine && (attr1 >> 12) & 1 != 0,
                vflip: !is_affine && (attr1 >> 13) & 1 != 0,
                affine_idx: is_affine.then_some(((attr1 >> 9) & 0x1F) as u8),
            }
        })
        .collect())
}

pub fn list_memory_regions(emu: &dyn Emulator) -> Vec<MemoryRegionDto> {
    emu.memory_regions()
        .into_iter()
        .map(|r| MemoryRegionDto {
            id: r.id,
            name: r.name,
            start: r.start,
            size: r.size,
            writable: r.writable,
        })
        .collect()
}

pub fn read_memory_window(
    emu: &dyn Emulator,
    addr: u32,
    len: usize,
    deref: bool,
) -> Result<MemoryWindowDto, String> {
    let mut resolved_addr = addr;
    let mut deref_ptr = None;
    if deref {
        let ptr = emu.read_u32(addr).map_err(|e| e.to_string())?;
        deref_ptr = Some(ptr);
        resolved_addr = ptr;
    }
    let regions = emu.memory_regions();
    let region = regions
        .into_iter()
        .find(|r| r.contains(resolved_addr, 1))
        .ok_or_else(|| format!("address {resolved_addr:#010x} is not in a mapped memory region"))?;
    let remaining = (region.start as u64 + region.size as u64 - resolved_addr as u64) as usize;
    let clamped_len = len.clamp(1, 512).min(remaining);
    let bytes = emu
        .read_memory(resolved_addr, clamped_len)
        .map_err(|e| e.to_string())?;
    let u8_val = bytes.first().copied();
    let u16_val = (bytes.len() >= 2).then(|| u16::from_le_bytes([bytes[0], bytes[1]]));
    let u32_val = (bytes.len() >= 4).then(|| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
    Ok(MemoryWindowDto {
        requested_addr: addr,
        resolved_addr,
        deref_ptr,
        region_id: region.id,
        region_name: region.name,
        bytes,
        u8_val,
        u16_val,
        u32_val,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn palette_tile_oam_and_memory_decoders_read_gba_hardware_ram() {
        let mut emu = Mgba::gba().unwrap();
        emu.load_rom(&probe_rom()).unwrap();

        // Write pure blue (BGR555 0x7C00) to BG palette 0 index 1 and pure red (0x001F) to OBJ palette 1 index 2.
        emu.write_memory(0x0500_0002, &0x7C00u16.to_le_bytes()).unwrap();
        emu.write_memory(0x0500_0224, &0x001Fu16.to_le_bytes()).unwrap();
        let pals = read_palettes(&emu).unwrap();
        assert_eq!(pals.bg.len(), 256);
        assert_eq!(pals.obj.len(), 256);
        assert_eq!((pals.bg[1].r, pals.bg[1].g, pals.bg[1].b), (0, 0, 255));
        assert_eq!(pals.bg[1].hex, "#0000FF");
        assert_eq!((pals.obj[18].r, pals.obj[18].g, pals.obj[18].b), (255, 0, 0));

        // Write pixel (0,0) = color index 1 in tile 0 (4bpp) at 0x0600_0000.
        emu.write_memory(0x0600_0000, &[0x01]).unwrap();
        let sheet = read_tiles_rgba(&emu, 0x0600_0000, false, 0, false).unwrap();
        assert_eq!(sheet.width, 128);
        assert_eq!(&sheet.rgba[0..4], &[0, 0, 255, 255]);
        assert_eq!(&sheet.rgba[4..8], &[0, 0, 0, 0]);

        // Write OAM sprite 3: normal 16x32 vertical, x=-4 (508), y=24, tile=42, palette=7, hflip=true.
        let attr0: u16 = 24 | (2 << 14);
        let attr1: u16 = 508 | (1 << 12) | (2 << 14);
        let attr2: u16 = 42 | (2 << 10) | (7 << 12);
        let mut oam_bytes = [0u8; 8];
        oam_bytes[0..2].copy_from_slice(&attr0.to_le_bytes());
        oam_bytes[2..4].copy_from_slice(&attr1.to_le_bytes());
        oam_bytes[4..6].copy_from_slice(&attr2.to_le_bytes());
        emu.write_memory(0x0700_0000 + 3 * 8, &oam_bytes).unwrap();
        let oam = read_oam(&emu).unwrap();
        assert_eq!(oam.len(), 128);
        assert_eq!(oam[3].x, -4);
        assert_eq!(oam[3].y, 24);
        assert_eq!((oam[3].width, oam[3].height), (16, 32));
        assert_eq!(oam[3].tile, 42);
        assert_eq!(oam[3].palette, 7);
        assert!(oam[3].hflip && !oam[3].vflip);

        // Pointer deref read from IWRAM -> EWRAM.
        emu.write_memory(0x0200_0100, &0x1234_5678u32.to_le_bytes()).unwrap();
        emu.write_memory(0x0300_0040, &0x0200_0100u32.to_le_bytes()).unwrap();
        let win = read_memory_window(&emu, 0x0300_0040, 16, true).unwrap();
        assert_eq!(win.deref_ptr, Some(0x0200_0100));
        assert_eq!(win.resolved_addr, 0x0200_0100);
        assert_eq!(win.u32_val, Some(0x1234_5678));
    }
}
