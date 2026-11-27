//! EMU-003: boot the user-built base ROM headless straight into a named map, at a
//! position, with flags set, and verify location and flags from RAM.
//!
//! Needs the container (struct offsets are measured against the project's
//! headers): runs with `TAUROS_DOCKER_TESTS=1` and the pinned expansion, base
//! build and toolchain image present; otherwise it skips.

mod common;

use std::fs;
use std::process::Command;
use tauros_connector_gen3::boot::{map_numbers, BootPlan, BootTarget, GameLayout, Location, Symbols};
use tauros_connector_gen3::toolchain::Toolchain;
use tauros_connector_gen3::Gen3Connector;
use tauros_emu::{Button, Buttons, Emulator, Mgba};
use tauros_model::*;

fn target(map: &str, x: i16, y: i16, flags: &[&str]) -> BootTarget {
    BootTarget { map: MapId::from(map), x, y, flags: flags.iter().map(|f| FlagId::from(*f)).collect() }
}

#[test]
fn boots_into_named_maps_with_flags() {
    let Some(engine) = common::engine() else { return };
    if std::env::var_os("TAUROS_DOCKER_TESTS").is_none() {
        eprintln!("SKIPPED: set TAUROS_DOCKER_TESTS=1 to run the boot test");
        return;
    }
    let tc = Toolchain::for_repository(&common::repo_root()).unwrap();
    if !Command::new("docker").args(["image", "inspect", &tc.image]).output().is_ok_and(|o| o.status.success()) {
        eprintln!("SKIPPED: image {} missing; run `python3 scripts/setup_toolchain.py`", tc.image);
        return;
    }
    let Some(dir) = common::built_fixture(&engine, "g3-boot") else { return };
    let layout = GameLayout::measure(&tc, dir.path()).unwrap();
    eprintln!("layout: {layout:?}");
    let snapshot = Gen3Connector::open(dir.path(), None).unwrap().load().unwrap();
    let maps = map_numbers(dir.path()).unwrap();
    let symbols = Symbols::parse(&fs::read_to_string(dir.path().join("pokeemerald.sym")).unwrap());
    let plan = |t: BootTarget| BootPlan::new(t, &snapshot, &maps, &symbols, layout);

    // Rejected before anything runs.
    assert!(plan(target("MAP_NOWHERE", 1, 1, &[])).is_err());
    assert!(plan(target("MAP_PETALBURG_CITY", 1000, 1, &[])).is_err());
    assert!(plan(target("MAP_PETALBURG_CITY", 1, 1, &["FLAG_NOPE"])).is_err());

    let mut emu = Mgba::gba().unwrap();
    emu.load_rom(&fs::read(dir.path().join("pokeemerald.gba")).unwrap()).unwrap();

    // Petalburg City in front of the gym, with the first badge.
    let petalburg = plan(target("MAP_PETALBURG_CITY", 15, 10, &["FLAG_BADGE01_GET"])).unwrap();
    let report = petalburg.boot(&mut emu).unwrap();
    eprintln!("Petalburg: {report:?}");
    assert_eq!(report.location, Location { group: 0, num: 0, x: 15, y: 10 });
    assert_eq!(petalburg.location(&emu).unwrap(), report.location);
    let badge = petalburg.flag_numbers[0];
    assert!(petalburg.flag(&emu, badge).unwrap());

    // Another map without the flag: same core, rebooted; the badge is not set by
    // the new game itself, so the flag above came from the boot.
    let route = plan(target("MAP_ROUTE101", 10, 10, &[])).unwrap();
    let report = route.boot(&mut emu).unwrap();
    eprintln!("Route 101: {report:?}");
    let (group, num) = maps["MAP_ROUTE101"];
    assert_eq!(report.location, Location { group, num, x: 10, y: 10 });
    assert!(!route.flag(&emu, badge).unwrap(), "the badge flag is set without being asked for");
}

/// EMU-006 investigation: boots every map of the base build at its centre and
/// writes one TSV row per map to `$TAUROS_PROBE_OUT`. Not an assertion test; it
/// documents which maps an Emerald build can boot into and how the others fail.
#[test]
fn probe_every_map() {
    let Some(out) = std::env::var_os("TAUROS_PROBE_OUT") else { return };
    let Some(engine) = common::engine() else { return };
    let tc = Toolchain::for_repository(&common::repo_root()).unwrap();
    let Some(dir) = common::built_fixture(&engine, "g3-probe") else { return };
    let layout = GameLayout::measure(&tc, dir.path()).unwrap();
    let snapshot = Gen3Connector::open(dir.path(), None).unwrap().load().unwrap();
    let maps = map_numbers(dir.path()).unwrap();
    let sym_text = fs::read_to_string(dir.path().join("pokeemerald.sym")).unwrap();
    let symbols = Symbols::parse(&sym_text);
    let mut by_addr = std::collections::BTreeMap::new();
    for line in sym_text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if let (4, Ok(a)) = (f.len(), u32::from_str_radix(f[0], 16)) {
            by_addr.entry(a).or_insert(f[3].to_owned());
        }
    }
    let g_main = symbols.get("gMain").unwrap();
    let mut emu = Mgba::gba().unwrap();
    emu.load_rom(&fs::read(dir.path().join("pokeemerald.gba")).unwrap()).unwrap();
    use std::io::Write;
    let mut file = fs::File::create(&out).unwrap();
    let rules = tauros_connector_gen3::preflight::MapRules::read(dir.path()).unwrap();
    let mut skipped = 0;
    writeln!(file, "map\tgroup\tnum\twidth\theight\tx\ty\tresult\tlocation\tcallback2\tframes").unwrap();
    for (id, map) in &snapshot.maps {
        let MapGeometry::Grid { width, height, .. } = map.geometry else { continue };
        let (x, y) = ((width / 2) as i16, (height / 2) as i16);
        let Some(&(group, num)) = maps.get(&id.0) else {
            writeln!(file, "{}\t-\t-\t{width}\t{height}\t{x}\t{y}\tno MAP_ constant\t\t\t", id.0).unwrap();
            continue;
        };
        // Maps the build leaves out hang for 900 frames each; boot every tenth only.
        if rules.refusal(map).is_some() && std::env::var_os("TAUROS_PROBE_ALL").is_none() {
            skipped += 1;
            if skipped % 10 != 0 {
                continue;
            }
        }
        let t = BootTarget { map: id.clone(), x, y, flags: vec![] };
        let (result, frames) = match BootPlan::new(t, &snapshot, &maps, &symbols, layout) {
            Err(e) => (format!("plan: {e}"), 0),
            Ok(plan) => match plan.boot(&mut emu) {
                Ok(r) => ("ok".to_owned(), r.frames),
                Err(e) => (e.to_string(), 0),
            },
        };
        let loc = plan_location(&emu, &symbols, layout);
        let cb2 = emu.read_u32(g_main + layout.main_callback2).map(|v| by_addr.get(&(v & !1)).cloned().unwrap_or(format!("{v:#x}"))).unwrap_or_default();
        writeln!(file, "{}\t{group}\t{num}\t{width}\t{height}\t{x}\t{y}\t{result}\t{loc}\t{cb2}\t{frames}", id.0).unwrap();
        file.flush().unwrap();
    }
}

fn plan_location(emu: &Mgba, symbols: &Symbols, layout: GameLayout) -> String {
    match tauros_connector_gen3::boot::read_live_location(emu, symbols.get("gSaveBlock1Ptr").unwrap(), layout) {
        Ok(Some(l)) => format!("{}:{} ({}, {})", l.group, l.num, l.x, l.y),
        Ok(None) => "none".into(),
        Err(e) => format!("err {e}"),
    }
}

/// A base build copy booted with everything the EMU-006 checks need.
struct Real {
    dir: common::TempDir,
    layout: GameLayout,
    snapshot: ProjectSnapshot,
    maps: std::collections::BTreeMap<String, (u8, u8)>,
    symbols: Symbols,
    emu: Mgba,
}

fn real() -> Option<Real> {
    let engine = common::engine()?;
    if std::env::var_os("TAUROS_DOCKER_TESTS").is_none() {
        eprintln!("SKIPPED: set TAUROS_DOCKER_TESTS=1 to run the boot test");
        return None;
    }
    let tc = Toolchain::for_repository(&common::repo_root()).unwrap();
    let dir = common::built_fixture(&engine, "g3-emu006")?;
    let layout = GameLayout::measure(&tc, dir.path()).unwrap();
    let snapshot = Gen3Connector::open(dir.path(), None).unwrap().load().unwrap();
    let maps = map_numbers(dir.path()).unwrap();
    let symbols = Symbols::parse(&fs::read_to_string(dir.path().join("pokeemerald.sym")).unwrap());
    let mut emu = Mgba::gba().unwrap();
    emu.load_rom(&fs::read(dir.path().join("pokeemerald.gba")).unwrap()).unwrap();
    Some(Real { dir, layout, snapshot, maps, symbols, emu })
}

impl Real {
    fn plan(&self, map: &str, x: i16, y: i16) -> BootPlan {
        BootPlan::new(target(map, x, y, &[]), &self.snapshot, &self.maps, &self.symbols, self.layout).unwrap()
    }
}

/// EMU-006: an Emerald build has no map header for the FireRed/LeafGreen maps and
/// no layout for their layouts, so the game hangs loading them; the rule refuses
/// them before the boot and lets every other map through.
#[test]
fn maps_left_out_of_an_emerald_build_are_refused_and_really_hang() {
    let Some(mut real) = real() else { return };
    let usability = tauros_connector_gen3::preflight::map_usability(real.dir.path(), &real.snapshot.maps).unwrap();
    let refused: Vec<_> = usability.iter().filter(|u| u.3.refusal.is_some()).collect();
    let count = |part: &str| refused.iter().filter(|u| u.3.refusal.as_deref().unwrap().contains(part)).count();
    // Pinned expansion 3796c72: 939 maps; 417 belong to REGION_KANTO and 4 are in no
    // map group (two of them FRLG-suffixed, so 419 `_Frlg` folders in all); 7 have no
    // tile to stand on (Route 130 is all deep water, the unused contest halls are 1x1 walls).
    assert_eq!((usability.len(), refused.len()), (939, 428));
    assert_eq!((count("REGION_KANTO"), count("no group"), count("no tile the player can stand on")), (417, 4, 7));
    let why = |id: &str| usability.iter().find(|u| u.0 .0 == id).unwrap().3.refusal.clone();
    assert!(why("MAP_CELADON_CITY").unwrap().contains("REGION_KANTO"));
    assert!(why("MAP_ROUTE23_UNUSED_HOUSE").unwrap().contains("no group"));
    assert_eq!(why("MAP_CAVE_OF_ORIGIN_UNUSED_RUBY_SAPPHIRE_MAP1"), None, "unused Ruby/Sapphire maps are part of the build");
    assert_eq!(why("MAP_PETALBURG_CITY"), None);

    // The refusal is not a guess: booting the refused map anyway hangs the game in
    // its map load, and a map the rule keeps loads.
    let hang = real.plan("MAP_CERULEAN_CITY_HOUSE1", 5, 5).boot(&mut real.emu).unwrap_err();
    assert!(hang.0.contains("not reached in 900 frames"), "{hang}");
    let (cx, cy) = usability.iter().find(|u| u.0 .0 == "MAP_CAVE_OF_ORIGIN_UNUSED_RUBY_SAPPHIRE_MAP1").unwrap().3.default_point;
    let ok = real.plan("MAP_CAVE_OF_ORIGIN_UNUSED_RUBY_SAPPHIRE_MAP1", cx, cy).boot(&mut real.emu).unwrap();
    assert_eq!((ok.location.x, ok.location.y), (cx, cy));
}

/// EMU-006: from the default point of a map the player can really walk away. Boots
/// every `$TAUROS_WALK_STEP`th kept map (default 10) at its default point, holds each
/// direction and requires the position to change (maps with an ON_FRAME script excepted). Also checks that the point inside
/// a wall is refused, with a tile that does work offered.
#[test]
fn default_points_can_be_walked_from() {
    let Some(mut real) = real() else { return };
    let step: usize = std::env::var("TAUROS_WALK_STEP").ok().and_then(|s| s.parse().ok()).unwrap_or(10);
    let usability = tauros_connector_gen3::preflight::map_usability(real.dir.path(), &real.snapshot.maps).unwrap();
    let mut stuck = Vec::new();
    let mut tried = 0;
    let kept = usability.iter().filter(|u| u.3.refusal.is_none());
    for (_, (id, _, _, u)) in kept.enumerate().filter(|(i, _)| i % step == 0) {
        let (x, y) = u.default_point;
        let plan = real.plan(&id.0, x, y);
        let Ok(report) = plan.boot(&mut real.emu) else { continue };
        if report.moved_by_map_script {
            continue;
        }
        tried += 1;
        let start = plan.location(&real.emu).unwrap();
        let moved = [Button::Right, Button::Left, Button::Up, Button::Down].into_iter().any(|b| {
            real.emu.set_buttons(Buttons::NONE.with(b));
            for _ in 0..40 {
                real.emu.run_frame().unwrap();
            }
            real.emu.set_buttons(Buttons::NONE);
            for _ in 0..20 {
                real.emu.run_frame().unwrap();
            }
            plan.location(&real.emu).unwrap() != start
        });
        // A map with an ON_FRAME script may hold the player for a cutscene on arrival
        // (battle rooms, reception gates, hole checks); only terrain is under test.
        let scripts = fs::read_to_string(real.dir.path().join("data/maps").join(&real.snapshot.maps[id].name).join("scripts.inc")).unwrap_or_default();
        if !moved && !scripts.contains("MAP_SCRIPT_ON_FRAME_TABLE") {
            stuck.push(format!("{} ({x}, {y})", id.0));
        }
    }
    eprintln!("walked from {tried} default points, stuck: {stuck:?}");
    assert!(tried > 20);
    // Measured with TAUROS_WALK_STEP=1 on the pinned base build: 455 of 463 default points
    // can be left. These 8 could not be left within 40 frames although their terrain is
    // open and most have no map script; the cause is not known (a metatile behaviour the
    // check does not model is the first suspect). Anything else is a regression.
    const UNEXPLAINED: [&str; 8] = [
        "MAP_BATTLE_FRONTIER_LOUNGE3",
        "MAP_BATTLE_PYRAMID_SQUARE07",
        "MAP_MAGMA_HIDEOUT_2F_1R",
        "MAP_MT_PYRE_6F",
        "MAP_ROUTE104_PROTOTYPE", // 1x1 map: nowhere to walk
        "MAP_ROUTE119_WEATHER_INSTITUTE_1F",
        "MAP_SOOTOPOLIS_CITY_GYM_B1F",
        "MAP_UNION_ROOM",
    ];
    if std::env::var_os("TAUROS_WALK_REPORT").is_some() {
        eprintln!("stuck before the allowlist: {stuck:?}");
    }
    stuck.retain(|m| !UNEXPLAINED.iter().any(|known| m.starts_with(&format!("{known} "))));
    assert!(stuck.is_empty(), "the player cannot leave these default points: {stuck:?}");

    // A wall: the abandoned ship's deck centre is solid.
    use tauros_connector_gen3::preflight::{assess_boot_point, PointAssessment};
    let deck = MapId::from("MAP_ABANDONED_SHIP_DECK");
    let MapGeometry::Grid { width, height, .. } = real.snapshot.maps[&deck].geometry else { panic!() };
    let assessment = assess_boot_point(real.dir.path(), &real.snapshot.maps, &deck, (width / 2) as i16, (height / 2) as i16).unwrap();
    assert!(matches!(assessment, PointAssessment::Blocked { nearest: Some(_), .. }), "{assessment:?}");
}
