//! Boot a built ROM straight into a chosen map, position and flags (EMU-003).
//!
//! The game is driven through its own callbacks, without patching the ROM:
//! 1. let the copyright screen initialise the save blocks and heap;
//! 2. set `gMain.callback2 = CB2_NewGame`, which runs `NewGameInitData` and loads
//!    the first map;
//! 3. once in the overworld, write the target into `gSaveBlock1Ptr->location` and
//!    `->pos`, set the flags in `->flags`, and set `gMain.callback2 = CB2_LoadMap`,
//!    which is how the game's own warps end (`WarpIntoMap` then `CB2_LoadMap`);
//! 4. wait for the overworld again and check location, position and flags in RAM.
//!
//! Addresses come from the build's `.sym`. Struct offsets come from compiling a
//! small C file against the project's own headers in the toolchain container, so
//! a project that changes `SaveBlock1` or `Main` still boots correctly.

use crate::toolchain::Toolchain;
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::Path;
use tauros_emu::{EmuError, Emulator};
use tauros_model::{FlagId, MapId, ProjectSnapshot};

/// Where to start the game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BootTarget {
    pub map: MapId,
    /// Metatile coordinates of the player.
    pub x: i16,
    pub y: i16,
    /// Flags to set before the map loads, so its scripts see them.
    pub flags: Vec<FlagId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BootError(pub String);

impl fmt::Display for BootError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "boot: {}", self.0)
    }
}

impl std::error::Error for BootError {}

impl From<EmuError> for BootError {
    fn from(e: EmuError) -> Self {
        BootError(e.to_string())
    }
}

fn err<T>(msg: impl Into<String>) -> Result<T, BootError> {
    Err(BootError(msg.into()))
}

/// Global symbols of a build, from its `.sym` (`addr scope size name`).
#[derive(Clone, Debug, Default)]
pub struct Symbols(BTreeMap<String, u32>);

impl Symbols {
    pub fn parse(text: &str) -> Self {
        let mut map = BTreeMap::new();
        for line in text.lines() {
            let f: Vec<&str> = line.split_whitespace().collect();
            if let (4, Ok(addr)) = (f.len(), u32::from_str_radix(f.first().unwrap_or(&""), 16)) {
                map.entry(f[3].to_owned()).or_insert(addr);
            }
        }
        Symbols(map)
    }

    pub fn get(&self, name: &str) -> Result<u32, BootError> {
        self.0.get(name).copied().ok_or_else(|| BootError(format!("symbol {name} not in the build")))
    }
}

/// Struct offsets the boot needs, measured against the project's headers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GameLayout {
    pub sb1_location: u32,
    pub sb1_pos: u32,
    pub sb1_flags: u32,
    pub sb1_flags_size: u32,
    pub main_callback1: u32,
    pub main_callback2: u32,
    pub main_state: u32,
}

const LAYOUT_MAGIC: u32 = 0x5441_5552; // "TAUR"
const LAYOUT_C: &str = r#"#include "global.h"
#include "main.h"
const u32 gTaurosLayout[] = {
    0x54415552,
    offsetof(struct SaveBlock1, location),
    offsetof(struct SaveBlock1, pos),
    offsetof(struct SaveBlock1, flags),
    sizeof(((struct SaveBlock1 *)0)->flags),
    offsetof(struct Main, callback1),
    offsetof(struct Main, callback2),
    offsetof(struct Main, state),
    sizeof(struct WarpData),
    offsetof(struct WarpData, mapGroup),
    offsetof(struct WarpData, mapNum),
    offsetof(struct WarpData, warpId),
    offsetof(struct WarpData, x),
    offsetof(struct WarpData, y),
};
"#;

impl GameLayout {
    /// Compiles the layout probe against `project`'s headers in the container
    /// (output under the project's ignored `build/tauros/`).
    pub fn measure(toolchain: &Toolchain, project: &Path) -> Result<Self, BootError> {
        let dir = project.join("build/tauros");
        fs::create_dir_all(&dir).map_err(|e| BootError(e.to_string()))?;
        fs::write(dir.join("layout.c"), LAYOUT_C).map_err(|e| BootError(e.to_string()))?;
        let makefile = fs::read_to_string(project.join("Makefile")).unwrap_or_default();
        let version = makefile
            .lines()
            .find_map(|l| l.strip_prefix("GAME_VERSION ?=").map(|v| v.trim().to_owned()))
            .unwrap_or_else(|| "EMERALD".into());
        let script = format!(
            "set -e; B=${{DEVKITARM:-/opt/devkitpro/devkitARM}}/bin; \
             $B/arm-none-eabi-gcc -c -mthumb -mabi=apcs-gnu -march=armv4t -O2 -std=gnu17 -iquote include \
             -DMODERN=1 -DTESTING=0 -D{version} -o build/tauros/layout.o build/tauros/layout.c; \
             $B/arm-none-eabi-objcopy -O binary -j .rodata build/tauros/layout.o build/tauros/layout.bin"
        );
        let (ok, log, _) = toolchain.run(Some(project), &script).map_err(BootError)?;
        if !ok {
            return err(format!("measuring struct layout failed:\n{}", log.trim()));
        }
        let bytes = fs::read(dir.join("layout.bin")).map_err(|e| BootError(e.to_string()))?;
        Self::from_words(&bytes.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect::<Vec<_>>())
    }

    fn from_words(w: &[u32]) -> Result<Self, BootError> {
        if w.len() != 14 || w[0] != LAYOUT_MAGIC {
            return err(format!("unexpected layout probe output {w:x?}"));
        }
        // WarpData must keep the vanilla shape the boot writes: s8 group, s8 num,
        // s8 warpId, s16 x at 4, s16 y at 6.
        if w[8..14] != [8, 0, 1, 2, 4, 6] {
            return err(format!("unsupported struct WarpData layout {:?}", &w[8..14]));
        }
        Ok(GameLayout {
            sb1_location: w[1],
            sb1_pos: w[2],
            sb1_flags: w[3],
            sb1_flags_size: w[4],
            main_callback1: w[5],
            main_callback2: w[6],
            main_state: w[7],
        })
    }
}

/// `MAP_*` constants of a built project: (group, number), from the generated
/// `include/constants/map_groups.h` the ROM was built with.
pub fn map_numbers(project: &Path) -> Result<BTreeMap<String, (u8, u8)>, BootError> {
    let text = fs::read_to_string(project.join("include/constants/map_groups.h"))
        .map_err(|e| BootError(format!("include/constants/map_groups.h ({e}); build the project first")))?;
    let mut out = BTreeMap::new();
    for line in text.lines() {
        // `    MAP_ROUTE101            = (16 | (0 << 8)),`
        let Some((name, value)) = line.trim().split_once('=') else { continue };
        let name = name.trim();
        if !name.starts_with("MAP_") {
            continue;
        }
        let digits: Vec<u32> = value.split(|c: char| !c.is_ascii_digit()).filter_map(|d| d.parse().ok()).collect();
        if let [num, group, 8] = digits[..] {
            out.insert(name.to_owned(), (group as u8, num as u8));
        }
    }
    Ok(out)
}

/// Everything resolved before touching the emulator.
#[derive(Clone, Debug)]
pub struct BootPlan {
    pub target: BootTarget,
    pub group: u8,
    pub num: u8,
    pub flag_numbers: Vec<u32>,
    pub layout: GameLayout,
    g_main: u32,
    sb1_ptr: u32,
    cb2_copyright: u32,
    cb2_new_game: u32,
    cb2_load_map: u32,
    cb2_overworld: u32,
}

impl BootPlan {
    /// Resolves `target` against the project's model, its generated map constants
    /// and its build's symbols. Rejects unknown maps and flags and positions
    /// outside the map before anything runs.
    pub fn new(
        target: BootTarget,
        snapshot: &ProjectSnapshot,
        maps: &BTreeMap<String, (u8, u8)>,
        symbols: &Symbols,
        layout: GameLayout,
    ) -> Result<Self, BootError> {
        let &(group, num) = maps.get(&target.map.0).ok_or_else(|| BootError(format!("unknown map {}", target.map.0)))?;
        if let Some(map) = snapshot.maps.get(&target.map) {
            if let tauros_model::MapGeometry::Grid { width, height, .. } = map.geometry {
                if target.x < 0 || target.y < 0 || target.x as u16 >= width || target.y as u16 >= height {
                    return err(format!("({}, {}) is outside {} ({width}x{height})", target.x, target.y, target.map.0));
                }
            }
        }
        let mut flag_numbers = Vec::new();
        for id in &target.flags {
            let flag = snapshot.flags.get(id).ok_or_else(|| BootError(format!("unknown flag {}", id.0)))?;
            let n = flag.value.ok_or_else(|| BootError(format!("flag {} has no numeric value", id.0)))?;
            if n == 0 || n >= layout.sb1_flags_size * 8 {
                return err(format!("flag {} = {n:#x} is not a saved flag (0 < n < {:#x})", id.0, layout.sb1_flags_size * 8));
            }
            flag_numbers.push(n);
        }
        Ok(BootPlan {
            target,
            group,
            num,
            flag_numbers,
            layout,
            g_main: symbols.get("gMain")?,
            sb1_ptr: symbols.get("gSaveBlock1Ptr")?,
            cb2_copyright: symbols.get("CB2_InitCopyrightScreenAfterBootup")?,
            cb2_new_game: symbols.get("CB2_NewGame")?,
            cb2_load_map: symbols.get("CB2_LoadMap")?,
            cb2_overworld: symbols.get("CB2_Overworld")?,
        })
    }
}

/// Where the game is, read from RAM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Location {
    pub group: u8,
    pub num: u8,
    pub x: i16,
    pub y: i16,
}

/// What `boot` observed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BootReport {
    /// Frames emulated from reset to the settled target map.
    pub frames: u32,
    pub location: Location,
}

/// Reads the player's live map group/num and coordinates from `gSaveBlock1Ptr`,
/// returning `Ok(None)` when the pointer is null or outside GBA EWRAM (e.g.
/// on the copyright/title screens before a save or new game is active).
pub fn read_live_location(
    emu: &(impl Emulator + ?Sized),
    sb1_ptr_addr: u32,
    layout: GameLayout,
) -> Result<Option<Location>, BootError> {
    let sb1 = emu.read_u32(sb1_ptr_addr)?;
    if !(0x0200_0000..0x0204_0000).contains(&sb1) {
        return Ok(None);
    }
    let loc = emu.read_memory(sb1 + layout.sb1_location, 2)?;
    let pos = emu.read_memory(sb1 + layout.sb1_pos, 4)?;
    Ok(Some(Location {
        group: loc[0],
        num: loc[1],
        x: i16::from_le_bytes([pos[0], pos[1]]),
        y: i16::from_le_bytes([pos[2], pos[3]]),
    }))
}

/// Frames allowed for each wait; the measured waits are far shorter.
const WAIT_LIMIT: u32 = 900;
/// Frames run after arriving, so the fade-in and map scripts have finished.
const SETTLE: u32 = 60;

fn thumb(addr: u32) -> u32 {
    addr | 1
}

impl BootPlan {
    fn callback2(&self, emu: &(impl Emulator + ?Sized)) -> Result<u32, BootError> {
        Ok(emu.read_u32(self.g_main + self.layout.main_callback2)? & !1)
    }

    fn set_callbacks(&self, emu: &mut (impl Emulator + ?Sized), cb2: u32) -> Result<(), BootError> {
        let main = self.g_main;
        emu.write_memory(main + self.layout.main_callback1, &0u32.to_le_bytes())?;
        emu.write_memory(main + self.layout.main_callback2, &thumb(cb2).to_le_bytes())?;
        emu.write_memory(main + self.layout.main_state, &[0])?;
        Ok(())
    }

    fn wait(&self, emu: &mut (impl Emulator + ?Sized), what: &str, done: impl Fn(u32) -> bool) -> Result<u32, BootError> {
        for n in 0..WAIT_LIMIT {
            if done(self.callback2(emu)?) {
                return Ok(n);
            }
            emu.run_frame()?;
        }
        err(format!("{what} not reached in {WAIT_LIMIT} frames"))
    }

    /// The location and position the game holds in `gSaveBlock1Ptr`.
    pub fn location(&self, emu: &(impl Emulator + ?Sized)) -> Result<Location, BootError> {
        read_live_location(emu, self.sb1_ptr, self.layout)?
            .ok_or_else(|| BootError("gSaveBlock1Ptr is not initialized in EWRAM".into()))
    }

    /// Whether saved flag `n` is set.
    pub fn flag(&self, emu: &(impl Emulator + ?Sized), n: u32) -> Result<bool, BootError> {
        let sb1 = emu.read_u32(self.sb1_ptr)?;
        Ok(emu.read_memory(sb1 + self.layout.sb1_flags + n / 8, 1)?[0] & 1 << (n % 8) != 0)
    }

    /// Resets the console and drives the game into the target; the ROM must
    /// already be loaded in `emu` and be the build the plan was made from.
    pub fn boot(&self, emu: &mut (impl Emulator + ?Sized)) -> Result<BootReport, BootError> {
        emu.reset()?;
        let copyright = self.cb2_copyright;
        let mut frames = self.wait(emu, "the end of the copyright screen", |cb| cb != 0 && cb != copyright)?;

        self.set_callbacks(emu, self.cb2_new_game)?;
        let overworld = self.cb2_overworld;
        frames += self.wait(emu, "the new-game overworld", |cb| cb == overworld)?;

        let sb1 = emu.read_u32(self.sb1_ptr)?;
        let t = &self.target;
        let mut warp = [self.group, self.num, 0xFF, 0, 0, 0, 0, 0];
        warp[4..6].copy_from_slice(&t.x.to_le_bytes());
        warp[6..8].copy_from_slice(&t.y.to_le_bytes());
        emu.write_memory(sb1 + self.layout.sb1_location, &warp)?;
        let pos: Vec<u8> = t.x.to_le_bytes().into_iter().chain(t.y.to_le_bytes()).collect();
        emu.write_memory(sb1 + self.layout.sb1_pos, &pos)?;
        for &n in &self.flag_numbers {
            let at = sb1 + self.layout.sb1_flags + n / 8;
            let byte = emu.read_memory(at, 1)?[0] | 1 << (n % 8);
            emu.write_memory(at, &[byte])?;
        }
        self.set_callbacks(emu, self.cb2_load_map)?;
        emu.run_frame()?;
        frames += 1 + self.wait(emu, "the target map", |cb| cb == overworld)?;
        for _ in 0..SETTLE {
            emu.run_frame()?;
        }
        frames += SETTLE;

        let location = self.location(emu)?;
        let want = Location { group: self.group, num: self.num, x: t.x, y: t.y };
        if location != want {
            return err(format!("arrived at {location:?}, wanted {want:?}"));
        }
        for (&n, id) in self.flag_numbers.iter().zip(&t.flags) {
            if !self.flag(emu, n)? {
                return err(format!("flag {} was cleared while loading the map", id.0));
            }
        }
        Ok(BootReport { frames, location })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_and_map_constants_parse() {
        let s = Symbols::parse("030066c8 g 0000043c gMain\n08196704 g 00000078 CB2_NewGame\nbad line\n");
        assert_eq!(s.get("gMain").unwrap(), 0x030066c8);
        assert!(s.get("gNope").is_err());

        let dir = std::env::temp_dir().join(format!("tauros-boot-maps-{}", std::process::id()));
        fs::create_dir_all(dir.join("include/constants")).unwrap();
        fs::write(
            dir.join("include/constants/map_groups.h"),
            "enum {\n    MAP_PETALBURG_CITY      = (0 | (0 << 8)),\n    MAP_ROUTE101            = (16 | (0 << 8)),\n    MAP_INSIDE_OF_TRUCK     = (40 | (25 << 8)),\n};\n#define MAP_GROUPS_COUNT 34\n",
        )
        .unwrap();
        let maps = map_numbers(&dir).unwrap();
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(maps["MAP_ROUTE101"], (0, 16));
        assert_eq!(maps["MAP_INSIDE_OF_TRUCK"], (25, 40));
        assert_eq!(maps.len(), 3);
    }

    #[test]
    fn layout_rejects_unexpected_shapes() {
        let good = [LAYOUT_MAGIC, 4, 0, 0x1270, 0x12c, 0, 4, 0x438, 8, 0, 1, 2, 4, 6];
        assert_eq!(GameLayout::from_words(&good).unwrap().sb1_flags, 0x1270);
        let mut bad = good;
        bad[12] = 3;
        assert!(GameLayout::from_words(&bad).is_err());
        assert!(GameLayout::from_words(&good[1..]).is_err());
    }
}
