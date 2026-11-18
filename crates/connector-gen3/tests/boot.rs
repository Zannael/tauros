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
use tauros_emu::{Emulator, Mgba};
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
