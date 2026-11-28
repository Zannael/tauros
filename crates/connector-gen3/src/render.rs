//! Draws a layout as the game and Porymap do (APP-001), read-only.
//!
//! A layout names a primary and a secondary tileset (`gTileset_*`). Their header in
//! `src/data/tilesets/headers.h` names the symbols of the tile sheet, the palettes,
//! the metatiles and their attributes; those symbols are defined in the project's C
//! sources with the path of the file they include (`INCGFX_U32("…/tiles.png", …)`,
//! `INCBIN_U16("…/metatiles.bin")`, a list of `…/NN.pal` or `…/NN.gbapal`). When a
//! symbol is not found the Porymap convention is used:
//! `data/tilesets/{primary|secondary}/<snake_case name>`.
//!
//! A metatile is 2×2 tiles on two layers (bottom, then top; `metatiles.bin`, eight
//! words per metatile). Each tile word holds the tile index (bits 0-9), horizontal and
//! vertical flip (bits 10, 11) and the palette (bits 12-15). Metatile ids, tile
//! indexes and palettes below the primary counts of `include/fieldmap.h` come from
//! the primary tileset, the others from the secondary one (metatile and tile indexes
//! offset by the primary count, palettes at their own index). Colour 0 of every
//! palette is transparent; the backdrop is colour 0 of palette 0, as on the GBA.
//!
//! Not drawn: tileset animations, the day/night tint of the expansion, the third
//! layer of triple-layer metatiles, object sprites.

use crate::maps::{Block, Blockdata, Layout, MetatileAttributes, Metatiles};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub const METATILE_PX: usize = 16;
const TILE_PX: usize = 8;
const TILESET_HEADERS: &str = "src/data/tilesets/headers.h";
/// Files whose `INCGFX`/`INCBIN` definitions give the tileset symbols their paths.
const SYMBOL_SOURCES: &[&str] = &[
    "src/data/tilesets/graphics.h",
    "src/data/tilesets/metatiles.h",
    "src/graphics.c",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderError(pub String);

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn err(message: impl Into<String>) -> RenderError {
    RenderError(message.into())
}

/// Primary/secondary split of include/fieldmap.h, for Emerald or FRLG layouts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub tiles_in_primary: usize,
    pub metatiles_in_primary: usize,
    pub pals_in_primary: usize,
}

impl Limits {
    pub const EMERALD: Self = Self { tiles_in_primary: 512, metatiles_in_primary: 512, pals_in_primary: 6 };
    pub const FRLG: Self = Self { tiles_in_primary: 640, metatiles_in_primary: 640, pals_in_primary: 7 };

    /// The limits the project defines for a layout version (`frlg` or other).
    pub fn read(project: &Path, layout_version: Option<&str>) -> Self {
        let frlg = layout_version == Some("frlg");
        let mut limits = if frlg { Self::FRLG } else { Self::EMERALD };
        let Ok(text) = fs::read_to_string(project.join("include/fieldmap.h")) else { return limits };
        let suffix = if frlg { "_FRLG" } else { "" };
        for line in text.lines() {
            let mut words = line.split_whitespace();
            let (Some("#define"), Some(name), Some(value)) = (words.next(), words.next(), words.next()) else { continue };
            let Some(value) = crate::maps::parse_c_int(value).and_then(|v| usize::try_from(v).ok()) else { continue };
            match name.strip_suffix(suffix).filter(|_| frlg || !name.ends_with("_FRLG")) {
                Some("NUM_TILES_IN_PRIMARY") => limits.tiles_in_primary = value,
                Some("NUM_METATILES_IN_PRIMARY") => limits.metatiles_in_primary = value,
                Some("NUM_PALS_IN_PRIMARY") => limits.pals_in_primary = value,
                _ => {}
            }
        }
        limits
    }
}

/// The files of one tileset, as resolved from the project.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TilesetFiles {
    pub tiles: Option<String>,
    /// Palette files by palette index.
    pub palettes: Vec<String>,
    pub metatiles: Option<String>,
    pub attributes: Option<String>,
}

/// `gTileset_*` headers and the paths of the symbols they use.
#[derive(Clone, Debug, Default)]
pub struct TilesetIndex {
    headers: BTreeMap<String, [Option<String>; 4]>,
    symbols: BTreeMap<String, Vec<String>>,
}

fn identifier_after<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let rest = text[text.find(key)? + key.len()..].trim_start().strip_prefix('=')?.trim_start();
    let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(rest.len());
    (end > 0).then(|| &rest[..end])
}

/// The first string argument of each `INCGFX_*`/`INCBIN_*` call in `text`; later
/// arguments are build options (`".4bpp.smol"`).
fn included_paths(text: &str) -> Vec<String> {
    text.split("INC")
        .skip(1)
        .filter_map(|call| call.split('"').nth(1))
        .map(str::to_owned)
        .collect()
}

/// The file a graphics include builds from: `tiles.4bpp.lz` and friends come from
/// `tiles.png`, `NN.gbapal` from `NN.pal`.
fn source_file(path: &str) -> String {
    for (built, source) in [(".4bpp.lz", ".png"), (".4bpp.smol", ".png"), (".4bpp.fastSmol", ".png"), (".4bpp", ".png"), (".gbapal", ".pal")] {
        if let Some(stem) = path.strip_suffix(built) {
            return format!("{stem}{source}");
        }
    }
    path.to_owned()
}

impl TilesetIndex {
    pub fn read(project: &Path) -> Self {
        let mut index = Self::default();
        if let Ok(text) = fs::read_to_string(project.join(TILESET_HEADERS)) {
            index.parse_headers(&text);
        }
        for file in SYMBOL_SOURCES {
            if let Ok(text) = fs::read_to_string(project.join(file)) {
                index.parse_symbols(&text);
            }
        }
        index
    }

    pub fn parse_headers(&mut self, text: &str) {
        for chunk in text.split("struct Tileset ").skip(1) {
            let name_end = chunk.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(chunk.len());
            let name = &chunk[..name_end];
            let Some(body) = chunk.find('{').and_then(|s| chunk[s..].find('}').map(|e| &chunk[s..s + e])) else { continue };
            let field = |key| identifier_after(body, key).map(str::to_owned);
            self.headers.insert(name.to_owned(), [field(".tiles"), field(".palettes"), field(".metatiles"), field(".metatileAttributes")]);
        }
    }

    /// Definitions `NAME[] = INC…("path", …);` and `NAME[][16] = { INC…("path"), … };`.
    pub fn parse_symbols(&mut self, text: &str) {
        for statement in text.split(';') {
            let Some(eq) = statement.find("[]") else { continue };
            let head = &statement[..eq];
            let name_start = head.rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).map_or(0, |i| i + 1);
            let name = &head[name_start..];
            let paths = included_paths(&statement[eq..]);
            if !name.is_empty() && !paths.is_empty() {
                self.symbols.insert(name.to_owned(), paths.iter().map(|p| source_file(p)).collect());
            }
        }
    }

    /// The files of `tileset` (`gTileset_General`); `secondary` picks the fallback folder.
    pub fn files(&self, project: &Path, tileset: &str, secondary: bool) -> TilesetFiles {
        let resolve = |slot: usize| -> Option<Vec<String>> {
            let symbol = self.headers.get(tileset)?[slot].as_ref()?;
            self.symbols.get(symbol).cloned()
        };
        let folder = format!("data/tilesets/{}/{}", if secondary { "secondary" } else { "primary" },
            snake_case(tileset.strip_prefix("gTileset_").unwrap_or(tileset)));
        let first = |slot, default: &str| resolve(slot).and_then(|p| p.into_iter().next()).or_else(|| {
            let path = format!("{folder}/{default}");
            project.join(&path).is_file().then_some(path)
        });
        let palettes = resolve(1).unwrap_or_else(|| {
            (0..16).map(|i| format!("{folder}/palettes/{i:02}.pal")).take_while(|p| project.join(p).is_file()).collect()
        });
        TilesetFiles { tiles: first(0, "tiles.png"), palettes, metatiles: first(2, "metatiles.bin"), attributes: first(3, "metatile_attributes.bin") }
    }
}

/// `SecretBaseRedCave` → `secret_base_red_cave`, as Porymap names tileset folders.
fn snake_case(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 && !out.ends_with('_') {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

type Palette = [[u8; 4]; 16];

/// A JASC-PAL file: header, version, colour count, then `r g b` lines.
pub fn parse_jasc(text: &str) -> Result<Palette, RenderError> {
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    if lines.next() != Some("JASC-PAL") {
        return Err(err("not a JASC-PAL file"));
    }
    lines.next();
    lines.next();
    let mut palette = [[0, 0, 0, 255]; 16];
    for (slot, line) in palette.iter_mut().zip(lines) {
        let rgb: Vec<u8> = line.split_whitespace().filter_map(|v| v.parse().ok()).collect();
        if rgb.len() < 3 {
            return Err(err(format!("bad colour line {line:?}")));
        }
        // The GBA keeps 5 bits per channel, as Porymap shows them.
        *slot = [rgb[0] & 0xF8, rgb[1] & 0xF8, rgb[2] & 0xF8, 255];
    }
    Ok(palette)
}

/// Colour indexes (0-15) of the 8×8 tiles of an indexed PNG sheet, row by row.
pub fn decode_tiles(bytes: &[u8]) -> Result<Vec<[u8; 64]>, RenderError> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::IDENTITY);
    let mut reader = decoder.read_info().map_err(|e| err(e.to_string()))?;
    let info = reader.info();
    if info.color_type != png::ColorType::Indexed {
        return Err(err(format!("the tile sheet is {:?}, not an indexed PNG", info.color_type)));
    }
    let (width, height, depth) = (info.width as usize, info.height as usize, info.bit_depth as usize);
    let mut buffer = vec![0; reader.output_buffer_size().ok_or_else(|| err("tile sheet too large"))?];
    let frame = reader.next_frame(&mut buffer).map_err(|e| err(e.to_string()))?;
    let stride = frame.line_size;
    let pixel = |x: usize, y: usize| -> u8 {
        let bit = x * depth;
        let byte = buffer[y * stride + bit / 8];
        let shift = 8 - depth - bit % 8;
        (byte >> shift) & ((1u16 << depth) - 1) as u8 & 0x0F
    };
    let (columns, rows) = (width / TILE_PX, height / TILE_PX);
    let mut tiles = Vec::with_capacity(columns * rows);
    for row in 0..rows {
        for column in 0..columns {
            let mut tile = [0u8; 64];
            for (i, value) in tile.iter_mut().enumerate() {
                *value = pixel(column * TILE_PX + i % TILE_PX, row * TILE_PX + i / TILE_PX);
            }
            tiles.push(tile);
        }
    }
    Ok(tiles)
}

/// One tileset loaded for drawing. Missing pieces stay empty and are reported.
#[derive(Clone, Debug, Default)]
pub struct Tileset {
    pub name: String,
    pub tiles: Vec<[u8; 64]>,
    pub palettes: Vec<Option<Palette>>,
    pub metatiles: Vec<u16>,
    pub attributes: Option<MetatileAttributes>,
}

impl Tileset {
    pub fn load(project: &Path, index: &TilesetIndex, name: &str, secondary: bool, problems: &mut Vec<String>) -> Self {
        let mut set = Self { name: name.to_owned(), ..Self::default() };
        // `0` or `NULL`: the layout has no such tileset (a NULL pointer in the game).
        if matches!(name, "0" | "NULL" | "") {
            return set;
        }
        let files = index.files(project, name, secondary);
        let read = |path: &str| fs::read(project.join(path)).map_err(|e| format!("{path}: {e}"));
        match files.tiles.as_deref().map(|p| read(p).and_then(|b| decode_tiles(&b).map_err(|e| format!("{p}: {e}")))) {
            Some(Ok(tiles)) => set.tiles = tiles,
            Some(Err(e)) => problems.push(e),
            None => problems.push(format!("{name}: tile sheet not found")),
        }
        for path in &files.palettes {
            match fs::read_to_string(project.join(path)).map_err(|e| e.to_string()).and_then(|t| parse_jasc(&t).map_err(|e| e.0)) {
                Ok(p) => set.palettes.push(Some(p)),
                Err(e) => {
                    problems.push(format!("{path}: {e}"));
                    set.palettes.push(None);
                }
            }
        }
        match files.metatiles.as_deref().map(|p| read(p).and_then(|b| Metatiles::parse(&b).map_err(|e| format!("{p}: {}", e.0)))) {
            Some(Ok(m)) => set.metatiles = m.0,
            Some(Err(e)) => problems.push(e),
            None => problems.push(format!("{name}: metatiles not found")),
        }
        let count = set.metatiles.len() / Metatiles::TILES_PER_METATILE;
        if let Some(path) = files.attributes.as_deref() {
            match read(path).and_then(|b| MetatileAttributes::parse(&b, count).map_err(|e| format!("{path}: {}", e.0))) {
                Ok(a) => set.attributes = Some(a),
                Err(e) => problems.push(e),
            }
        }
        set
    }
}

/// A layout drawn at 16 px per block, with what each block is.
#[derive(Clone, Debug)]
pub struct RenderedLayout {
    pub width: u16,
    pub height: u16,
    /// RGBA, `width * 16` by `height * 16`.
    pub rgba: Vec<u8>,
    pub blocks: Vec<Block>,
    /// Metatile behaviour of each block, when its attributes are known.
    pub behaviors: Vec<Option<u32>>,
    /// Files or entries that could not be read; their tiles stay transparent.
    pub problems: Vec<String>,
}

/// Primary and secondary tilesets with the split between them.
pub struct TilesetPair {
    pub primary: Tileset,
    pub secondary: Tileset,
    pub limits: Limits,
}

impl TilesetPair {
    pub fn load(project: &Path, layout: &Layout, problems: &mut Vec<String>) -> Self {
        let index = TilesetIndex::read(project);
        Self {
            primary: Tileset::load(project, &index, &layout.primary_tileset, false, problems),
            secondary: Tileset::load(project, &index, &layout.secondary_tileset, true, problems),
            limits: Limits::read(project, layout.layout_version.as_deref()),
        }
    }

    fn split(&self, id: usize, primary_count: usize) -> (&Tileset, usize) {
        if id < primary_count { (&self.primary, id) } else { (&self.secondary, id - primary_count) }
    }

    fn tile(&self, index: usize) -> Option<&[u8; 64]> {
        let (set, i) = self.split(index, self.limits.tiles_in_primary);
        set.tiles.get(i)
    }

    fn palette(&self, index: usize) -> Option<&Palette> {
        let set = if index < self.limits.pals_in_primary { &self.primary } else { &self.secondary };
        set.palettes.get(index).and_then(Option::as_ref)
    }

    /// The eight tile words of a metatile.
    pub fn metatile(&self, id: u16) -> Option<&[u16]> {
        let (set, i) = self.split(id as usize, self.limits.metatiles_in_primary);
        set.metatiles.get(i * Metatiles::TILES_PER_METATILE..(i + 1) * Metatiles::TILES_PER_METATILE)
    }

    pub fn behavior(&self, id: u16) -> Option<u32> {
        let (set, i) = self.split(id as usize, self.limits.metatiles_in_primary);
        set.attributes.as_ref()?.behavior(i)
    }

    fn backdrop(&self) -> [u8; 4] {
        self.palette(0).map_or([0, 0, 0, 255], |p| p[0])
    }

    /// Draws metatile `id` into `out` (RGBA, `stride` pixels wide) at (`ox`, `oy`).
    pub fn draw_metatile(&self, id: u16, out: &mut [u8], stride: usize, ox: usize, oy: usize) {
        let backdrop = self.backdrop();
        for y in 0..METATILE_PX {
            for x in 0..METATILE_PX {
                let at = ((oy + y) * stride + ox + x) * 4;
                out[at..at + 4].copy_from_slice(&backdrop);
            }
        }
        let Some(words) = self.metatile(id) else { return };
        for (n, word) in words.iter().enumerate() {
            let (layer_x, layer_y) = ((n % 4) % 2 * TILE_PX, (n % 4) / 2 * TILE_PX);
            let (Some(tile), Some(palette)) = (self.tile((word & 0x3FF) as usize), self.palette((word >> 12) as usize)) else { continue };
            let (hflip, vflip) = (word & 0x400 != 0, word & 0x800 != 0);
            for py in 0..TILE_PX {
                for px in 0..TILE_PX {
                    let sx = if hflip { TILE_PX - 1 - px } else { px };
                    let sy = if vflip { TILE_PX - 1 - py } else { py };
                    let colour = tile[sy * TILE_PX + sx] as usize;
                    if colour == 0 {
                        continue;
                    }
                    let at = ((oy + layer_y + py) * stride + ox + layer_x + px) * 4;
                    out[at..at + 4].copy_from_slice(&palette[colour]);
                }
            }
        }
    }
}

/// Draws `layout` from the project's files.
pub fn render_layout(project: &Path, layout: &Layout) -> Result<RenderedLayout, RenderError> {
    let bytes = fs::read(project.join(&layout.blockdata_filepath)).map_err(|e| err(format!("{}: {e}", layout.blockdata_filepath)))?;
    let data = Blockdata::parse(&bytes).map_err(|e| err(format!("{}: {}", layout.blockdata_filepath, e.0)))?;
    let (width, height) = (layout.width as usize, layout.height as usize);
    // The game reads the first width × height words; some files carry padding after them.
    if data.0.len() < width * height {
        return Err(err(format!("{} has {} blocks, the layout is {width}x{height}", layout.blockdata_filepath, data.0.len())));
    }
    let mut problems = Vec::new();
    let pair = TilesetPair::load(project, layout, &mut problems);
    Ok(render_blocks(&pair, layout.width, layout.height, &data.0[..width * height], problems))
}

pub fn render_blocks(pair: &TilesetPair, width: u16, height: u16, words: &[u16], problems: Vec<String>) -> RenderedLayout {
    let stride = width as usize * METATILE_PX;
    let mut rgba = vec![0; stride * height as usize * METATILE_PX * 4];
    let blocks: Vec<Block> = words.iter().map(|w| Block::unpack(*w)).collect();
    for (i, block) in blocks.iter().enumerate() {
        let (x, y) = (i % width as usize, i / width as usize);
        pair.draw_metatile(block.metatile, &mut rgba, stride, x * METATILE_PX, y * METATILE_PX);
    }
    let mut problems = problems;
    let missing = blocks.iter().filter(|b| pair.metatile(b.metatile).is_none()).count();
    if missing > 0 {
        problems.push(format!("{missing} blocks use metatiles their tilesets do not define; they show the backdrop"));
    }
    let behaviors = blocks.iter().map(|b| pair.behavior(b.metatile)).collect();
    RenderedLayout { width, height, rgba, blocks, behaviors, problems }
}

/// `MB_*` names by value from include/constants/metatile_behaviors.h.
pub fn behavior_names(project: &Path) -> BTreeMap<u32, String> {
    let Ok(text) = fs::read_to_string(project.join("include/constants/metatile_behaviors.h")) else { return BTreeMap::new() };
    parse_enum(&text, "MB_")
}

/// Values of the `prefix` entries of the first C `enum { … }` in `text`.
pub fn parse_enum(text: &str, prefix: &str) -> BTreeMap<u32, String> {
    let mut names = BTreeMap::new();
    let Some(body) = text.find("enum").and_then(|s| text[s..].find('{').map(|b| s + b + 1)) else { return names };
    let body = &text[body..body + text[body..].find('}').unwrap_or(text.len() - body)];
    let mut next = 0i64;
    for entry in body.lines().map(|l| l.split("//").next().unwrap_or("")).flat_map(|l| l.split(',')) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let (name, value) = match entry.split_once('=') {
            Some((n, v)) => (n.trim(), crate::maps::parse_c_int(v.trim()).unwrap_or(next)),
            None => (entry, next),
        };
        if name.starts_with(prefix) {
            names.entry(value as u32).or_insert_with(|| name.to_owned());
        }
        next = value + 1;
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(colour: u8) -> [u8; 64] {
        [colour; 64]
    }

    fn pair(primary_tiles: Vec<[u8; 64]>, secondary_tiles: Vec<[u8; 64]>, metatiles: Vec<u16>) -> TilesetPair {
        let palette = |base: u8| -> Palette { std::array::from_fn(|i| [base, i as u8 * 8, 0, 255]) };
        TilesetPair {
            primary: Tileset { tiles: primary_tiles, palettes: vec![Some(palette(0)), Some(palette(8))], metatiles, ..Tileset::default() },
            secondary: Tileset { tiles: secondary_tiles, palettes: vec![None, None, Some(palette(16))], ..Tileset::default() },
            limits: Limits { tiles_in_primary: 2, metatiles_in_primary: 4, pals_in_primary: 2 },
        }
    }

    fn px(out: &[u8], stride: usize, x: usize, y: usize) -> [u8; 4] {
        let at = (y * stride + x) * 4;
        out[at..at + 4].try_into().unwrap()
    }

    #[test]
    fn layers_flips_palettes_and_transparency() {
        let mut gradient = [0u8; 64];
        gradient[0] = 3; // top-left pixel only
        // Metatile 0: bottom layer tile 0 (colour 1) everywhere; top layer: tile 1
        // (transparent but its corner) h-flipped in the first quarter, a secondary tile
        // (index 2) with palette 2 in the second, nothing (tile 0 is colour 1, so use
        // a transparent tile 3 out of range) elsewhere.
        let words = vec![0x0000, 0x0000, 0x0000, 0x1000, 0x0401, 0x2002, 0x03FF, 0x03FF];
        let pair = pair(vec![solid(1), gradient], vec![solid(5)], words);
        let stride = 16;
        let mut out = vec![0; 16 * 16 * 4];
        pair.draw_metatile(0, &mut out, stride, 0, 0);
        // Bottom-right quarter uses palette 1 on the bottom layer, nothing on top.
        assert_eq!(px(&out, stride, 12, 12), [8, 8, 0, 255]);
        // First quarter: bottom colour 1 of palette 0, top tile corner flipped to x=7.
        assert_eq!(px(&out, stride, 0, 0), [0, 8, 0, 255]);
        assert_eq!(px(&out, stride, 7, 0), [0, 24, 0, 255]);
        // Second quarter: secondary tile 0 (index 2 - 2 primary tiles) with secondary palette 2.
        assert_eq!(px(&out, stride, 8, 0), [16, 40, 0, 255]);
        // Bottom-left: only the bottom layer.
        assert_eq!(px(&out, stride, 0, 8), [0, 8, 0, 255]);
    }

    #[test]
    fn missing_metatile_draws_the_backdrop_and_secondary_ids_are_offset() {
        let mut pair = pair(vec![solid(2)], vec![], vec![0x0000; 8]);
        pair.secondary.metatiles = vec![0x0000; 8];
        pair.secondary.tiles = vec![];
        let mut out = vec![0; 16 * 16 * 4];
        pair.draw_metatile(9, &mut out, 16, 0, 0); // no such metatile
        assert_eq!(px(&out, 16, 5, 5), [0, 0, 0, 255]);
        pair.draw_metatile(4, &mut out, 16, 0, 0); // secondary metatile 0, primary tile 0
        assert_eq!(px(&out, 16, 5, 5), [0, 16, 0, 255]);
        assert!(pair.metatile(4).is_some() && pair.metatile(5).is_none());
    }

    #[test]
    fn resolves_tileset_symbols_and_built_names() {
        let mut index = TilesetIndex::default();
        index.parse_headers("const struct Tileset gTileset_General =\n{\n    .isCompressed = TRUE,\n    .tiles = gTilesetTiles_General,\n    .palettes = gTilesetPalettes_General,\n    .metatiles = gMetatiles_General,\n    .metatileAttributes = gMetatileAttributes_General,\n};\n");
        index.parse_symbols("const u32 gTilesetTiles_General[] = INCGFX_U32(\"data/tilesets/primary/general/tiles.png\", \".4bpp.smol\");\nconst u16 ALIGNED(4) gTilesetPalettes_General[][16] =\n{\n    INCBIN_U16(\"data/tilesets/primary/general/palettes/00.gbapal\"),\n    INCGFX_U16(\"data/tilesets/primary/general/palettes/01.pal\", \".gbapal\"),\n};\nconst u16 gMetatiles_General[] = INCBIN_U16(\"data/tilesets/primary/general/metatiles.bin\");\n");
        let files = index.files(Path::new("/nonexistent"), "gTileset_General", false);
        assert_eq!(files.tiles.as_deref(), Some("data/tilesets/primary/general/tiles.png"));
        assert_eq!(files.palettes, vec!["data/tilesets/primary/general/palettes/00.pal", "data/tilesets/primary/general/palettes/01.pal"]);
        assert_eq!(files.metatiles.as_deref(), Some("data/tilesets/primary/general/metatiles.bin"));
        assert_eq!(files.attributes, None);
        assert_eq!(snake_case("SecretBaseRedCave"), "secret_base_red_cave");
        assert_eq!(source_file("a/tiles.4bpp.lz"), "a/tiles.png");
    }

    #[test]
    fn decodes_indexed_sheets_and_jasc_palettes() {
        let mut png_bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png_bytes, 16, 8);
            encoder.set_color(png::ColorType::Indexed);
            encoder.set_depth(png::BitDepth::Four);
            encoder.set_palette(vec![0u8; 48]);
            let mut writer = encoder.write_header().unwrap();
            // Row-major 4bpp: first tile all colour 1, second tile all colour 0xF.
            let row = [0x11, 0x11, 0x11, 0x11, 0xFF, 0xFF, 0xFF, 0xFF];
            writer.write_image_data(&row.repeat(8)).unwrap();
        }
        let tiles = decode_tiles(&png_bytes).unwrap();
        assert_eq!(tiles.len(), 2);
        assert!(tiles[0].iter().all(|c| *c == 1) && tiles[1].iter().all(|c| *c == 15));
        let palette = parse_jasc("JASC-PAL\r\n0100\r\n16\r\n24 41 82\r\n255 255 255\r\n").unwrap();
        assert_eq!(palette[0], [24, 40, 80, 255]);
        assert_eq!(palette[1], [248, 248, 248, 255]);
        assert!(parse_jasc("nope").is_err());
    }

    #[test]
    fn reads_enum_values_and_limits() {
        let names = parse_enum("enum {\n    MB_NORMAL,\n    MB_TALL_GRASS, // grass\n    MB_X = 0x10,\n    MB_Y,\n    NUM_METATILE_BEHAVIORS\n};", "MB_");
        assert_eq!(names.get(&0).map(String::as_str), Some("MB_NORMAL"));
        assert_eq!(names.get(&1).map(String::as_str), Some("MB_TALL_GRASS"));
        assert_eq!(names.get(&0x11).map(String::as_str), Some("MB_Y"));
        assert!(!names.values().any(|n| n.starts_with("NUM_")));
        assert_eq!(Limits::read(Path::new("/nonexistent"), Some("frlg")), Limits::FRLG);
    }
}
