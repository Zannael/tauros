//! Phase 1 integration (G3-006): open the pinned project, run the MOD-001 contract
//! suite, change a trainer from Rust, build in the container and observe the change
//! in the built ROM.
//!
//! The ROM check is a byte-level test runner, not an emulator: the edited ROM must
//! differ from the base ROM in exactly one byte, holding the new level, inside the
//! party that `gTrainers[DIFFICULTY_NORMAL][TRAINER_ROXANNE_1]` points to. The table
//! address comes from the build's `.sym`; the row size is derived from the symbol
//! size and the counts in the project's headers, not hard-coded.
//!
//! Runs with `TAUROS_DOCKER_TESTS=1` and the pinned expansion, base build and
//! toolchain image present; otherwise it skips.

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;
use tauros_connector_gen3::toolchain::Toolchain;
use tauros_connector_gen3::Gen3Connector;
use tauros_model::contract::assert_contract;
use tauros_model::*;

const ROM_BASE: u32 = 0x0800_0000;
const TRAINER: &str = "TRAINER_ROXANNE_1";

/// The value of `#define name <int>` in a project header.
fn define(dir: &Path, header: &str, name: &str) -> u32 {
    let text = fs::read_to_string(dir.join(header)).unwrap();
    text.lines()
        .find_map(|l| {
            let mut words = l.split_whitespace();
            (words.next() == Some("#define") && words.next() == Some(name)).then(|| words.next()?.parse().ok()).flatten()
        })
        .unwrap_or_else(|| panic!("{name} not defined as an integer in {header}"))
}

/// Position of `name` in `enum DifficultyLevel`, and the count of entries before
/// `DIFFICULTY_COUNT` outside `#if TESTING`.
fn difficulty(dir: &Path, name: &str) -> (u32, u32) {
    let text = fs::read_to_string(dir.join("include/constants/difficulty.h")).unwrap();
    let body = &text[text.find("enum DifficultyLevel").unwrap()..];
    let (mut index, mut count, mut testing) = (None, 0, false);
    for line in body.lines().skip(1) {
        let word = line.trim().split([',', ' ', '/']).next().unwrap_or("");
        match word {
            "#if" => testing = line.contains("TESTING"),
            "#endif" => testing = false,
            "DIFFICULTY_COUNT" => break,
            w if w.starts_with("DIFFICULTY_") && !testing => {
                if w == name {
                    index = Some(count);
                }
                count += 1;
            }
            _ => {}
        }
    }
    (index.unwrap_or_else(|| panic!("{name} not in enum DifficultyLevel")), count)
}

/// Address and size of a global symbol in a `.sym` file (`addr g size name`).
fn symbol(syms: &str, name: &str) -> (u32, u32) {
    syms.lines()
        .find_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            (f.len() == 4 && f[3] == name).then(|| (u32::from_str_radix(f[0], 16).unwrap(), u32::from_str_radix(f[2], 16).unwrap()))
        })
        .unwrap_or_else(|| panic!("symbol {name} not found"))
}

fn word(rom: &[u8], addr: u32) -> u32 {
    let at = (addr - ROM_BASE) as usize;
    u32::from_le_bytes(rom[at..at + 4].try_into().unwrap())
}

#[test]
fn phase1_trainer_edit_reaches_the_built_rom() {
    let Some(engine) = common::engine() else { return };
    if std::env::var_os("TAUROS_DOCKER_TESTS").is_none() {
        eprintln!("SKIPPED: set TAUROS_DOCKER_TESTS=1 to run the Phase 1 integration test");
        return;
    }
    let tc = Toolchain::for_repository(&common::repo_root()).unwrap();
    if !Command::new("docker").args(["image", "inspect", &tc.image]).output().is_ok_and(|o| o.status.success()) {
        eprintln!("SKIPPED: image {} missing; run `python3 scripts/setup_toolchain.py`", tc.image);
        return;
    }

    // The MOD-001 contract suite on fresh copies of the same pinned project.
    let copies = std::cell::RefCell::new(Vec::new());
    assert_contract(|| {
        let dir = common::fixture(&engine, "g3-phase1-contract", &["src", "include", "data", "tools/trainerproc"]);
        let connector = Gen3Connector::open(dir.path(), None).unwrap();
        copies.borrow_mut().push(dir);
        connector
    });
    drop(copies);

    // Open the built project and change one trainer from Rust.
    let Some(dir) = common::built_fixture(&engine, "g3-phase1") else { return };
    let base_rom = fs::read(dir.path().join("pokeemerald.gba")).unwrap();
    let mut c = Gen3Connector::open(dir.path(), Some(tc)).unwrap();
    let mut trainer = c.load().unwrap().trainers[&TrainerId::from(TRAINER)].clone();
    let old_level = trainer.party.members[0].level;
    trainer.party.members[0].level += 1;
    let new_level = trainer.party.members[0].level;
    let set = ChangeSet::new("Phase 1 integration: Roxanne's lead +1 level", Provenance::AiAssisted)
        .with(Change::UpsertTrainer { trainer });
    let applied = c.apply(&set).unwrap();
    assert_eq!(applied.touched, ["src/data/trainers.party"]);
    assert!(applied.commit.is_some(), "the change was not committed");

    let out = c.build().unwrap_or_else(|e| panic!("build failed: {e:?}"));
    let rom = fs::read(&out.rom).unwrap();
    let syms = fs::read_to_string(out.symbols.expect("build produced no .sym")).unwrap();

    // The built ROM differs from the base ROM in exactly one byte: the level.
    assert_eq!(rom.len(), base_rom.len(), "ROM size changed");
    let diffs: Vec<usize> = (0..rom.len()).filter(|&i| rom[i] != base_rom[i]).collect();
    eprintln!("differing ROM offsets: {:x?}", diffs);
    assert_eq!(diffs.len(), 1, "expected a single changed byte");
    let at = diffs[0];
    assert_eq!((base_rom[at], rom[at]), (old_level, new_level));

    // And that byte lies in the party the trainer's gTrainers row points to.
    let (table, size) = symbol(&syms, "gTrainers");
    let (normal, difficulties) = difficulty(dir.path(), "DIFFICULTY_NORMAL");
    let count = define(dir.path(), "include/constants/opponents.h", "TRAINERS_COUNT_EMERALD");
    let id = define(dir.path(), "include/constants/opponents.h", TRAINER);
    assert_eq!(size % (difficulties * count), 0, "gTrainers size is not a whole table");
    let row_size = size / (difficulties * count);
    let row = table + (normal * count + id) * row_size;
    // struct Trainer starts with `u64 aiFlags`, then `const struct TrainerMon *party`.
    let party = word(&rom, row + 8);
    let next = word(&rom, row + row_size + 8);
    let byte = ROM_BASE + at as u32;
    eprintln!(
        "gTrainers {table:#x}, {difficulties}x{count} rows of {row_size} bytes; {TRAINER} party {party:#x}; level byte {byte:#x}: {old_level} -> {new_level}"
    );
    assert!(party >= ROM_BASE && byte >= party, "level byte precedes the party");
    let member_bound = if next > party { (next - party).min(row_size) } else { row_size };
    assert!(byte < party + member_bound, "level byte is not in the first party member");
}
